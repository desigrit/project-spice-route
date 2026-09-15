use crate::codex::{self, CodexExport, PendingFileKind};
use crate::error::{Result, SpiceError};
use crate::models::{
    AppConfig, GitDescriptor, ObjectEntry, ObjectKind, ProjectExport, ProjectMode, SelectionRules,
    SnapshotManifest, SnapshotSummary,
};
use crate::settings::{self, cloud_store_root};
use crate::util::{hash_json, paths_overlap, read_json, safe_relative, sha256_bytes};
use chrono::Utc;
use globset::{Glob, GlobSet, GlobSetBuilder};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use uuid::Uuid;
use walkdir::{DirEntry, WalkDir};

pub const SNAPSHOT_SCHEMA: u32 = 2;

fn supported_snapshot_schema(version: u32) -> bool {
    matches!(version, 1 | 2)
}

pub struct ObjectStore {
    root: PathBuf,
    preview_only: bool,
}

impl ObjectStore {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        Ok(Self {
            root,
            preview_only: false,
        })
    }

    /// Compute content identities for comparisons without compressing, syncing, or
    /// retaining another copy of every selected file. Preview objects cannot publish.
    pub fn for_preview(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            preview_only: true,
        }
    }

    pub fn object_path(&self, hash: &str) -> Result<PathBuf> {
        if hash.len() != 64 || !hash.chars().all(|value| value.is_ascii_hexdigit()) {
            return Err(SpiceError::CorruptSnapshot(format!(
                "Invalid object hash {hash}"
            )));
        }
        Ok(self.root.join(&hash[..2]).join(format!("{hash}.zst")))
    }

    pub fn put_file(
        &self,
        source: &Path,
        logical_path: String,
        kind: ObjectKind,
        owner_id: String,
    ) -> Result<ObjectEntry> {
        self.put_file_cancellable(source, logical_path, kind, owner_id, None)
    }

    pub fn put_file_cancellable(
        &self,
        source: &Path,
        logical_path: String,
        kind: ObjectKind,
        owner_id: String,
        cancel: Option<&AtomicBool>,
    ) -> Result<ObjectEntry> {
        let metadata_before = fs::metadata(source)?;
        if !metadata_before.is_file() {
            return Err(SpiceError::User(format!(
                "Expected a regular file: {}",
                source.display()
            )));
        }
        if self.preview_only {
            let (hash, raw_size) = hash_file_cancellable(source, cancel)?;
            let metadata_after = fs::metadata(source)?;
            let (confirmed_hash, confirmed_size) = hash_file_cancellable(source, cancel)?;
            if metadata_before.len() != metadata_after.len()
                || metadata_before.modified().ok() != metadata_after.modified().ok()
                || hash != confirmed_hash
                || raw_size != confirmed_size
            {
                return Err(SpiceError::User(format!(
                    "{} changed while it was being inspected. Wait for writes to finish and try again.",
                    source.display()
                )));
            }
            return Ok(ObjectEntry {
                hash,
                logical_path,
                kind,
                owner_id,
                raw_size,
                stored_size: 0,
                executable: is_executable(&metadata_before),
            });
        }
        let temporary = self.root.join(format!(".{}.partial", Uuid::new_v4()));
        let captured = (|| -> Result<(String, u64)> {
            let input = File::open(source)?;
            let output = BufWriter::new(File::create(&temporary)?);
            let mut encoder = zstd::stream::write::Encoder::new(output, 6)?;
            let mut reader = BufReader::new(input);
            let mut buffer = vec![0_u8; 1024 * 1024];
            let mut hasher = Sha256::new();
            let mut raw_size = 0_u64;
            loop {
                check_cancel(cancel)?;
                let count = reader.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                hasher.update(&buffer[..count]);
                encoder.write_all(&buffer[..count])?;
                raw_size += count as u64;
            }
            let mut output = encoder.finish()?;
            output.flush()?;
            output.get_ref().sync_all()?;
            Ok((format!("{:x}", hasher.finalize()), raw_size))
        })();
        let (hash, raw_size) = match captured {
            Ok(value) => value,
            Err(error) => {
                let _ = fs::remove_file(&temporary);
                return Err(error);
            }
        };
        let metadata_after = fs::metadata(source)?;
        let (hash_after, size_after) = hash_file_cancellable(source, cancel)?;
        if metadata_before.len() != metadata_after.len()
            || metadata_before.modified().ok() != metadata_after.modified().ok()
            || raw_size != size_after
            || hash != hash_after
        {
            let _ = fs::remove_file(&temporary);
            return Err(SpiceError::User(format!(
                "{} changed while it was being captured. Wait for writes to finish and try again.",
                source.display()
            )));
        }
        let destination = self.object_path(&hash)?;
        fs::create_dir_all(destination.parent().expect("object path has parent"))?;
        if destination.exists() {
            fs::remove_file(&temporary)?;
        } else {
            fs::rename(&temporary, &destination)?;
        }
        let stored_size = fs::metadata(&destination)?.len();
        Ok(ObjectEntry {
            hash,
            logical_path,
            kind,
            owner_id,
            raw_size,
            stored_size,
            executable: is_executable(&metadata_before),
        })
    }

    pub fn put_bytes(
        &self,
        bytes: &[u8],
        logical_path: String,
        kind: ObjectKind,
        owner_id: String,
    ) -> Result<ObjectEntry> {
        if self.preview_only {
            return Ok(ObjectEntry {
                hash: sha256_bytes(bytes),
                logical_path,
                kind,
                owner_id,
                raw_size: bytes.len() as u64,
                stored_size: 0,
                executable: false,
            });
        }
        let source = tempfile::NamedTempFile::new_in(&self.root)?;
        fs::write(source.path(), bytes)?;
        self.put_file(source.path(), logical_path, kind, owner_id)
    }

    pub fn verify(&self, object: &ObjectEntry) -> Result<()> {
        self.verify_cancellable(object, None)
    }

    pub fn verify_cancellable(
        &self,
        object: &ObjectEntry,
        cancel: Option<&AtomicBool>,
    ) -> Result<()> {
        let path = self.object_path(&object.hash)?;
        if !path.is_file() {
            return Err(SpiceError::CorruptSnapshot(format!(
                "Missing object {} ({})",
                object.hash, object.logical_path
            )));
        }
        let input = BufReader::new(File::open(&path)?);
        let mut decoder = zstd::stream::read::Decoder::new(input)?;
        let mut hasher = Sha256::new();
        let mut size = 0_u64;
        let mut buffer = vec![0_u8; 1024 * 1024];
        loop {
            check_cancel(cancel)?;
            let count = decoder.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
            size += count as u64;
        }
        let actual = format!("{:x}", hasher.finalize());
        if actual != object.hash || size != object.raw_size {
            return Err(SpiceError::CorruptSnapshot(format!(
                "Hash mismatch for {}",
                object.logical_path
            )));
        }
        Ok(())
    }

    pub fn materialize(&self, object: &ObjectEntry, destination: &Path) -> Result<()> {
        self.materialize_cancellable(object, destination, None)
    }

    pub fn materialize_cancellable(
        &self,
        object: &ObjectEntry,
        destination: &Path,
        cancel: Option<&AtomicBool>,
    ) -> Result<()> {
        let parent = destination.parent().ok_or_else(|| {
            SpiceError::User(format!(
                "Destination has no parent: {}",
                destination.display()
            ))
        })?;
        fs::create_dir_all(parent)?;
        let partial = parent.join(format!(
            ".{}.{}.partial",
            destination
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("file"),
            Uuid::new_v4()
        ));
        let restored = (|| -> Result<()> {
            let input = BufReader::new(File::open(self.object_path(&object.hash)?)?);
            let mut decoder = zstd::stream::read::Decoder::new(input)?;
            let mut output = BufWriter::new(File::create(&partial)?);
            let mut buffer = vec![0_u8; 1024 * 1024];
            let mut hasher = Sha256::new();
            let mut size = 0_u64;
            loop {
                check_cancel(cancel)?;
                let count = decoder.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                hasher.update(&buffer[..count]);
                size += count as u64;
                output.write_all(&buffer[..count])?;
            }
            if format!("{:x}", hasher.finalize()) != object.hash || size != object.raw_size {
                return Err(SpiceError::CorruptSnapshot(format!(
                    "Hash mismatch for {}",
                    object.logical_path
                )));
            }
            output.flush()?;
            output.get_ref().sync_all()?;
            Ok(())
        })();
        if let Err(error) = restored {
            let _ = fs::remove_file(&partial);
            return Err(error);
        }
        if destination.exists() {
            fs::remove_file(destination)?;
        }
        fs::rename(&partial, destination)?;
        set_executable(destination, object.executable)?;
        Ok(())
    }

    pub fn import_from(&self, source: &ObjectStore, object: &ObjectEntry) -> Result<()> {
        self.import_from_cancellable(source, object, None)
    }

    pub fn import_from_cancellable(
        &self,
        source: &ObjectStore,
        object: &ObjectEntry,
        cancel: Option<&AtomicBool>,
    ) -> Result<()> {
        let source_path = source.object_path(&object.hash)?;
        let destination = self.object_path(&object.hash)?;
        if destination.is_file() {
            match self.verify_cancellable(object, cancel) {
                Ok(()) => return Ok(()),
                Err(SpiceError::Cancelled) => return Err(SpiceError::Cancelled),
                Err(_) => {}
            }
        }
        source.verify_cancellable(object, cancel)?;
        if destination.is_file() {
            fs::remove_file(&destination)?;
        }
        fs::create_dir_all(destination.parent().expect("object path has parent"))?;
        let partial = destination.with_extension(format!("zst.{}.partial", Uuid::new_v4()));
        let copied = (|| -> Result<()> {
            let mut input = BufReader::new(File::open(source_path)?);
            let mut output = BufWriter::new(File::create(&partial)?);
            let mut buffer = vec![0_u8; 1024 * 1024];
            loop {
                check_cancel(cancel)?;
                let count = input.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                output.write_all(&buffer[..count])?;
            }
            output.flush()?;
            output.get_ref().sync_all()?;
            Ok(())
        })();
        if let Err(error) = copied {
            let _ = fs::remove_file(&partial);
            return Err(error);
        }
        if let Err(error) = fs::rename(&partial, &destination) {
            if destination.is_file() {
                let _ = fs::remove_file(&partial);
            } else {
                return Err(error.into());
            }
        }
        self.verify_cancellable(object, cancel)
    }
}

pub fn build_manifest(
    config: &AppConfig,
    export: CodexExport,
    store: &ObjectStore,
    parent_id: Option<String>,
) -> Result<SnapshotManifest> {
    build_manifest_cancellable(config, export, store, parent_id, None, false)
}

pub fn build_manifest_cancellable(
    config: &AppConfig,
    mut export: CodexExport,
    store: &ObjectStore,
    parent_id: Option<String>,
    cancel: Option<&AtomicBool>,
    require_project_sources: bool,
) -> Result<SnapshotManifest> {
    validate_source_folders(config, &export.projects)?;
    if require_project_sources {
        validate_push_source_roots(config, &export.projects)?;
    }
    let mut objects = Vec::new();
    let mut warnings = export.warnings;
    for mut pending in export.pending_files {
        check_cancel(cancel)?;
        if matches!(pending.kind, PendingFileKind::Attachment) {
            if let Some(project) = export
                .threads
                .iter()
                .find(|thread| thread.id == pending.owner_id)
                .and_then(|thread| thread.project_id.as_ref())
                .and_then(|id| export.projects.iter().find(|project| &project.id == id))
            {
                for (index, discovered) in project.source_roots.iter().enumerate() {
                    if let Some(relative) =
                        codex::relative_under(&pending.source, Path::new(discovered))
                    {
                        pending.source =
                            settings::source_project_folder(config, &project.id, index, discovered)
                                .join(relative);
                        break;
                    }
                }
            }
            if !pending.source.is_file() {
                warnings.push(format!(
                    "An attachment for chat {} is unavailable at its local source folder and was not captured: {}",
                    pending.owner_id, pending.source.display()
                ));
                if let Some(thread) = export
                    .threads
                    .iter_mut()
                    .find(|thread| thread.id == pending.owner_id)
                {
                    thread
                        .attachments
                        .retain(|reference| reference.logical_path != pending.logical_path);
                }
                continue;
            }
        }
        if !config.cloud_root.is_empty()
            && paths_overlap(&pending.source, Path::new(&config.cloud_root))
        {
            return Err(SpiceError::User(format!(
                "Active workspace files must stay outside the cloud transport folder: {}",
                pending.source.display()
            )));
        }
        match pending.kind {
            PendingFileKind::Rollout => {
                let rollout_fingerprint = codex::portable_rollout_fingerprint(&pending.source)?;
                let object = store.put_file_cancellable(
                    &pending.source,
                    pending.logical_path,
                    ObjectKind::Rollout,
                    pending.owner_id.clone(),
                    cancel,
                )?;
                if let Some(thread) = export
                    .threads
                    .iter_mut()
                    .find(|thread| thread.id == pending.owner_id)
                {
                    thread.fingerprint = sha256_bytes(
                        format!("{}:{rollout_fingerprint}", thread.fingerprint).as_bytes(),
                    );
                }
                objects.push(object);
            }
            PendingFileKind::ProjectlessRoot => {
                capture_tree(
                    &pending.source,
                    &format!("projectless/{}/files", pending.owner_id),
                    &pending.owner_id,
                    ObjectKind::ProjectlessFile,
                    config,
                    store,
                    &mut objects,
                    &mut warnings,
                    cancel,
                )?;
            }
            PendingFileKind::Attachment => {
                let object = store.put_file_cancellable(
                    &pending.source,
                    pending.logical_path,
                    ObjectKind::Artifact,
                    pending.owner_id.clone(),
                    cancel,
                )?;
                if let Some(thread) = export
                    .threads
                    .iter_mut()
                    .find(|thread| thread.id == pending.owner_id)
                {
                    thread.fingerprint =
                        sha256_bytes(format!("{}:{}", thread.fingerprint, object.hash).as_bytes());
                }
                objects.push(object);
            }
        }
    }
    for project in export
        .projects
        .iter_mut()
        .filter(|project| project.mode == ProjectMode::Full)
    {
        for (index, root) in project.source_roots.clone().into_iter().enumerate() {
            check_cancel(cancel)?;
            let root = settings::source_project_folder(config, &project.id, index, &root);
            if !config.cloud_root.is_empty() && paths_overlap(&root, Path::new(&config.cloud_root))
            {
                return Err(SpiceError::User(format!("Project {} is inside the cloud transport folder. Move it outside that folder before syncing.", project.name)));
            }
            if !root.is_dir() {
                if require_project_sources {
                    return Err(format!("Cannot Push: the folder for {} became unavailable during capture: {}. Check its local folder in What to sync and preview again.", project.name, root.display()).into());
                }
                warnings.push(format!(
                    "Project {} root is missing and was not captured: {}",
                    project.name,
                    root.display()
                ));
                continue;
            }
            if let Some(descriptor) = capture_git(
                &root,
                &project.id,
                index,
                store,
                &mut objects,
                &mut warnings,
                cancel,
            )? {
                project.git.push(descriptor);
            }
            report_external_git_resources(&root, &mut warnings);
            capture_tree(
                &root,
                &format!("projects/{}/{index}/files", project.id),
                &project.id,
                ObjectKind::ProjectFile,
                config,
                store,
                &mut objects,
                &mut warnings,
                cancel,
            )?;
        }
    }
    if require_project_sources {
        validate_push_source_roots(config, &export.projects)?;
    }
    objects.sort_by(|a, b| a.logical_path.cmp(&b.logical_path));
    let id = format!(
        "{}-{}",
        Utc::now().format("%Y%m%dT%H%M%SZ"),
        Uuid::new_v4().simple()
    );
    Ok(SnapshotManifest {
        schema_version: SNAPSHOT_SCHEMA,
        id,
        created_at: Utc::now().to_rfc3339(),
        device_id: config.device_id.clone(),
        device_name: config.device_name.clone(),
        parent_id,
        additional_parent_ids: Vec::new(),
        selection_revision: config.selection.revision.clone(),
        selection: config.selection.clone(),
        compatibility: export.compatibility,
        codex_version: None,
        threads: export.threads,
        projects: export.projects,
        objects,
        ui_state: export.ui_state,
        session_index_lines: export.session_index_lines,
        warnings,
    })
}

pub(crate) fn validate_push_source_roots(
    config: &AppConfig,
    projects: &[ProjectExport],
) -> Result<()> {
    for project in projects
        .iter()
        .filter(|project| project.mode == ProjectMode::Full)
    {
        if project.source_roots.is_empty() {
            return Err(format!("Cannot Push: {} has no project folder recorded in Codex. Add its project folder or choose History only in What to sync.", project.name).into());
        }
        for (index, discovered) in project.source_roots.iter().enumerate() {
            let root = settings::source_project_folder(config, &project.id, index, discovered);
            if !root.is_dir() {
                return Err(format!("Cannot Push: the folder for {} is unavailable: {}. Check its local folder in What to sync, reconnect its drive, or choose History only.", project.name, root.display()).into());
            }
        }
    }
    Ok(())
}

fn validate_source_folders(config: &AppConfig, projects: &[ProjectExport]) -> Result<()> {
    let mut roots: Vec<(&str, PathBuf, &str)> = Vec::new();
    for project in projects
        .iter()
        .filter(|project| project.mode == ProjectMode::Full)
    {
        for (index, discovered) in project.source_roots.iter().enumerate() {
            let local = settings::source_project_folder(config, &project.id, index, discovered);
            settings::validate_project_folder(&local, config)?;
            for (previous_source, previous_local, previous_name) in &roots {
                let shared = settings::same_project_folder(
                    Path::new(discovered),
                    Path::new(previous_source),
                );
                if shared && !settings::same_project_folder(&local, previous_local) {
                    return Err(format!("{} and {} share one workspace. Choose the same local folder for both projects.", project.name, previous_name).into());
                }
                if !shared
                    && paths_overlap(
                        &settings::resolved_project_folder(&local),
                        &settings::resolved_project_folder(previous_local),
                    )
                {
                    return Err(format!("The local folders for {} and {} overlap. Choose separate folders for distinct project roots.", project.name, previous_name).into());
                }
            }
            roots.push((discovered, local, &project.name));
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn capture_tree(
    root: &Path,
    logical_root: &str,
    owner_id: &str,
    kind: ObjectKind,
    config: &AppConfig,
    store: &ObjectStore,
    objects: &mut Vec<ObjectEntry>,
    warnings: &mut Vec<String>,
    cancel: Option<&AtomicBool>,
) -> Result<()> {
    let patterns = build_exclusions(&config.selection.extra_exclude_patterns)?;
    let mut case_paths: HashMap<String, String> = HashMap::new();
    let mut excluded_count = 0_usize;
    let mut excluded_examples = Vec::new();
    let walker = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| should_enter(entry, root, &config.selection));
    for entry in walker {
        check_cancel(cancel)?;
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                warnings.push(format!("Could not inspect a project path: {error}"));
                continue;
            }
        };
        if entry.path() == root || entry.file_type().is_dir() {
            continue;
        }
        let relative = entry.path().strip_prefix(root).map_err(|_| {
            SpiceError::User(format!(
                "Path escaped project root: {}",
                entry.path().display()
            ))
        })?;
        let portable = relative.to_string_lossy().replace('\\', "/");
        if excluded_relative(&portable, config, &patterns) {
            excluded_count += 1;
            if excluded_examples.len() < 5 {
                excluded_examples.push(portable);
            }
            continue;
        }
        if entry.file_type().is_symlink() {
            let target = fs::read_link(entry.path())
                .map(|path| path.display().to_string())
                .unwrap_or_else(|_| "unreadable target".to_string());
            warnings.push(format!(
                "Symbolic link was not transferred: {} -> {}",
                entry.path().display(),
                target
            ));
            continue;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let folded = portable.to_lowercase();
        if let Some(existing) = case_paths.insert(folded, portable.clone()) {
            return Err(SpiceError::User(format!("Case-colliding files cannot be restored safely on Windows: {existing} and {portable}")));
        }
        let logical = format!("{logical_root}/{portable}");
        objects.push(store.put_file_cancellable(
            entry.path(),
            logical,
            kind.clone(),
            owner_id.to_string(),
            cancel,
        )?);
    }
    if excluded_count > 0 {
        warnings.push(format!(
            "{} file{} excluded from {} by safety or custom rules{}.",
            excluded_count,
            if excluded_count == 1 {
                " was"
            } else {
                "s were"
            },
            root.display(),
            if excluded_examples.is_empty() {
                String::new()
            } else {
                format!(" (for example: {})", excluded_examples.join(", "))
            }
        ));
    }
    Ok(())
}

/// Metadata-only estimate of the selected working files. Git archives are made
/// during capture; this avoids traversing dependencies and build caches merely
/// to populate project rows in the interface.
pub fn estimate_workspace_bytes(root: &Path, selection: &SelectionRules) -> Result<u64> {
    let patterns = build_exclusions(&selection.extra_exclude_patterns)?;
    let mut size = 0_u64;
    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| should_enter(entry, root, selection))
        .filter_map(|entry| entry.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let Ok(relative) = entry.path().strip_prefix(root) else {
            continue;
        };
        let portable = relative.to_string_lossy().replace('\\', "/");
        if !excluded_by_selection(&portable, selection, &patterns) {
            if let Ok(metadata) = entry.metadata() {
                size = size.saturating_add(metadata.len());
            }
        }
    }
    Ok(size)
}

fn should_enter(entry: &DirEntry, root: &Path, selection: &SelectionRules) -> bool {
    if entry.path() == root {
        return true;
    }
    let name = entry.file_name().to_string_lossy().to_lowercase();
    if name == ".git" {
        return false;
    }
    if !entry.file_type().is_dir() {
        return true;
    }
    if !selection.include_build_outputs && is_build_output_component(&name) {
        return false;
    }
    true
}

fn is_build_output_component(name: &str) -> bool {
    matches!(
        name,
        "node_modules"
            | "target"
            | "dist"
            | "build"
            | ".next"
            | ".nuxt"
            | ".cache"
            | ".gradle"
            | "vendor"
            | "__pycache__"
            | ".venv"
            | "venv"
            | "bin"
            | "obj"
    )
}

fn build_exclusions(values: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for value in values {
        builder.add(Glob::new(value)?);
    }
    Ok(builder.build()?)
}

fn excluded_relative(relative: &str, config: &AppConfig, patterns: &GlobSet) -> bool {
    excluded_by_selection(relative, &config.selection, patterns)
}

fn excluded_by_selection(relative: &str, selection: &SelectionRules, patterns: &GlobSet) -> bool {
    if patterns.is_match(relative) {
        return true;
    }
    if relative
        .split('/')
        .any(|component| component.eq_ignore_ascii_case(".git"))
    {
        return true;
    }
    if selection.include_sensitive_files {
        return false;
    }
    is_sensitive_relative(relative)
}

fn is_sensitive_relative(relative: &str) -> bool {
    let normalized = relative.to_ascii_lowercase();
    let name = relative
        .rsplit('/')
        .next()
        .unwrap_or(relative)
        .to_lowercase();
    let is_env = name == ".env"
        || (name.starts_with(".env.") && !name.ends_with(".example") && !name.ends_with(".sample"));
    let is_key = [
        ".pem",
        ".key",
        ".p12",
        ".pfx",
        ".p8",
        ".jks",
        ".keystore",
        ".kdbx",
        ".snk",
        ".ovpn",
    ]
    .iter()
    .any(|extension| name.ends_with(extension));
    let credential = matches!(
        name.as_str(),
        ".npmrc"
            | ".pypirc"
            | ".netrc"
            | "_netrc"
            | "auth.json"
            | "credentials.json"
            | "service-account.json"
            | "client-secret.json"
            | "client_secret.json"
            | "application-default-credentials.json"
            | "secrets.json"
            | "secrets.yml"
            | "secrets.yaml"
            | "kubeconfig"
            | "id_rsa"
            | "id_dsa"
            | "id_ecdsa"
            | "id_ed25519"
    ) || name.ends_with(".credentials")
        || name.ends_with(".tfvars")
        || name.ends_with(".tfstate")
        || (name.starts_with("service-account") && name.ends_with(".json"))
        || (name.starts_with("client-secret") && name.ends_with(".json"))
        || (name.starts_with("client_secret") && name.ends_with(".json"))
        || (name.starts_with("id_") && !name.ends_with(".pub"))
        || normalized == ".aws/credentials"
        || normalized.ends_with("/.aws/credentials")
        || normalized == ".docker/config.json"
        || normalized.ends_with("/.docker/config.json");
    is_env || is_key || credential
}

pub(crate) fn selection_allows_object_path(
    object: &ObjectEntry,
    selection: &SelectionRules,
) -> bool {
    let segments: Vec<_> = object.logical_path.split('/').collect();
    let relative = match object.kind {
        ObjectKind::ProjectFile if segments.len() >= 5 => segments[4..].join("/"),
        ObjectKind::ProjectlessFile if segments.len() >= 4 => segments[3..].join("/"),
        _ => return true,
    };
    if relative
        .split('/')
        .any(|component| component.eq_ignore_ascii_case(".git"))
    {
        return false;
    }
    if !selection.include_build_outputs
        && relative
            .split('/')
            .map(str::to_ascii_lowercase)
            .any(|component| is_build_output_component(&component))
    {
        return false;
    }
    let patterns = match build_exclusions(&selection.extra_exclude_patterns) {
        Ok(patterns) => patterns,
        Err(_) => return false,
    };
    !patterns.is_match(&relative)
        && (selection.include_sensitive_files || !is_sensitive_relative(&relative))
}

fn report_external_git_resources(root: &Path, warnings: &mut Vec<String>) {
    if root.join(".gitmodules").is_file() {
        warnings.push(format!(
            "{} uses Git submodules. Their checked-out files are captured, but their separate history still requires the configured submodule remotes.",
            root.display()
        ));
    }
    let attributes = root.join(".gitattributes");
    let uses_lfs = fs::metadata(&attributes)
        .ok()
        .filter(|metadata| metadata.len() <= 1024 * 1024)
        .and_then(|_| fs::read_to_string(attributes).ok())
        .map(|content| {
            content.lines().any(|line| {
                let compact: String = line
                    .chars()
                    .filter(|character| !character.is_whitespace())
                    .collect();
                compact.to_ascii_lowercase().contains("filter=lfs")
            })
        })
        .unwrap_or(false);
    if uses_lfs {
        warnings.push(format!(
            "{} uses Git LFS. Current working files are captured; older LFS content still requires the configured LFS remote.",
            root.display()
        ));
    }
}

pub(crate) fn capture_git(
    root: &Path,
    project_id: &str,
    root_index: usize,
    store: &ObjectStore,
    objects: &mut Vec<ObjectEntry>,
    warnings: &mut Vec<String>,
    cancel: Option<&AtomicBool>,
) -> Result<Option<GitDescriptor>> {
    check_cancel(cancel)?;
    if !root.join(".git").exists() {
        return Ok(None);
    }
    warnings.push(format!(
        "{} includes complete Git history. Working-file exclusions do not remove secrets that were already committed.",
        root.display()
    ));
    let common = git_output(root, &["rev-parse", "--git-common-dir"]);
    let git_dir = git_output(root, &["rev-parse", "--git-dir"]);
    let (Some(common), Some(git_dir)) = (common, git_dir) else {
        return Err(SpiceError::User(format!(
            "Git metadata could not be read for {}. Install Git or repair the repository before syncing this full project.",
            root.display()
        )));
    };
    let common_path = absolute_git_path(root, &common);
    let git_dir_path = absolute_git_path(root, &git_dir);
    let common_id = sha256_bytes(common_path.to_string_lossy().to_lowercase().as_bytes());
    let linked_worktree = root.join(".git").is_file();
    let head = git_output(root, &["rev-parse", "HEAD"]);
    let branch = git_output(root, &["symbolic-ref", "--quiet", "--short", "HEAD"]);
    let temp = tempfile::tempdir()?;
    let bundle_path = temp.path().join("repository.bundle");
    let has_refs = git_output(root, &["for-each-ref", "--format=%(refname)"]).is_some();
    let mut bundle_command = Command::new("git");
    bundle_command
        .arg("-C")
        .arg(root)
        .args(["bundle", "create"])
        .arg(&bundle_path)
        .arg("--all");
    if head.is_some() {
        bundle_command.arg("HEAD");
    }
    let bundle_output = bundle_command.output()?;
    let bundle_object = if bundle_output.status.success() {
        let object = store.put_file_cancellable(
            &bundle_path,
            format!("projects/{project_id}/{root_index}/git/repository.bundle"),
            ObjectKind::GitBundle,
            project_id.to_string(),
            cancel,
        )?;
        let hash = object.hash.clone();
        objects.push(object);
        Some(hash)
    } else if head.is_none() && !has_refs {
        None
    } else {
        return Err(SpiceError::User(format!(
            "Git history could not be captured for {}: {}",
            root.display(),
            String::from_utf8_lossy(&bundle_output.stderr).trim()
        )));
    };
    let index_path = git_dir_path.join("index");
    let (index_object, index_pack_object) = if index_path.is_file() {
        let portable_index_path = temp.path().join("portable.index");
        fs::copy(&index_path, &portable_index_path)?;
        let normalize_index = Command::new("git")
            .arg("-C")
            .arg(root)
            .env("GIT_INDEX_FILE", &portable_index_path)
            .args([
                "update-index",
                "--no-split-index",
                "--no-fsmonitor",
                "--no-untracked-cache",
                "--force-write-index",
            ])
            .output()?;
        if !normalize_index.status.success() {
            return Err(SpiceError::User(format!(
                "The Git index could not be made portable for {}: {}",
                root.display(),
                String::from_utf8_lossy(&normalize_index.stderr).trim()
            )));
        }
        let index_pack_path = temp.path().join("index-objects.pack");
        let pack_output = Command::new("git")
            .arg("-C")
            .arg(root)
            .env("GIT_INDEX_FILE", &portable_index_path)
            .args(["pack-objects", "--stdout", "--indexed-objects"])
            .stdout(Stdio::from(File::create(&index_pack_path)?))
            .output()?;
        check_cancel(cancel)?;
        if !pack_output.status.success() {
            return Err(SpiceError::User(format!(
                "Staged Git objects could not be captured for {}: {}",
                root.display(),
                String::from_utf8_lossy(&pack_output.stderr).trim()
            )));
        }
        let pack = store.put_file_cancellable(
            &index_pack_path,
            format!("projects/{project_id}/{root_index}/git/index-objects.pack"),
            ObjectKind::GitObjectPack,
            project_id.to_string(),
            cancel,
        )?;
        let pack_hash = pack.hash.clone();
        objects.push(pack);
        let object = store.put_file_cancellable(
            &portable_index_path,
            format!("projects/{project_id}/{root_index}/git/index"),
            ObjectKind::GitIndex,
            project_id.to_string(),
            cancel,
        )?;
        let hash = object.hash.clone();
        objects.push(object);
        (Some(hash), Some(pack_hash))
    } else {
        (None, None)
    };
    Ok(Some(GitDescriptor {
        project_id: project_id.to_string(),
        root_index,
        common_id,
        linked_worktree,
        head,
        branch,
        bundle_object,
        index_object,
        index_pack_object,
    }))
}

fn absolute_git_path(root: &Path, value: &str) -> PathBuf {
    let path = PathBuf::from(value.trim());
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

fn git_output(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|value| !value.is_empty())
}

pub fn manifest_fingerprint(manifest: &SnapshotManifest) -> Result<String> {
    // Compression is a storage detail. A hash-only preview and a captured object
    // must compare equally, including when compression encodings change.
    let objects: Vec<_> = manifest
        .objects
        .iter()
        .map(|object| {
            (
                &object.hash,
                &object.logical_path,
                &object.kind,
                &object.owner_id,
                object.raw_size,
                object.executable,
            )
        })
        .collect();
    hash_json(&(
        manifest.selection_revision.as_str(),
        &manifest.threads,
        &manifest.projects,
        objects,
        &manifest.ui_state,
    ))
}

pub fn publish(
    config: &AppConfig,
    local_store: &ObjectStore,
    manifest: &SnapshotManifest,
) -> Result<SnapshotSummary> {
    publish_cancellable(config, local_store, manifest, None)
}

pub fn publish_cancellable(
    config: &AppConfig,
    local_store: &ObjectStore,
    manifest: &SnapshotManifest,
    cancel: Option<&AtomicBool>,
) -> Result<SnapshotSummary> {
    if local_store.preview_only {
        return Err(SpiceError::User(
            "A preview has no captured content and cannot be published.".into(),
        ));
    }
    let root = cloud_store_root(config);
    let cloud_store = ObjectStore::new(root.join("objects"))?;
    let mut imported = HashSet::new();
    for object in &manifest.objects {
        check_cancel(cancel)?;
        if imported.insert(&object.hash) {
            cloud_store.import_from_cancellable(local_store, object, cancel)?;
        }
    }
    let manifests = root.join("snapshots");
    fs::create_dir_all(&manifests)?;
    let final_path = manifests.join(format!("{}.json", manifest.id));
    if final_path.exists() {
        return Err(SpiceError::User(format!(
            "Snapshot {} already exists.",
            manifest.id
        )));
    }
    let partial = manifests.join(format!(".{}.partial", manifest.id));
    let bytes = serde_json::to_vec_pretty(manifest)?;
    {
        let mut file = File::create(&partial)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    fs::rename(partial, &final_path)?;
    Ok(manifest.summary(
        manifest
            .objects
            .iter()
            .map(|object| object.stored_size)
            .sum(),
        true,
    ))
}

fn check_cancel(cancel: Option<&AtomicBool>) -> Result<()> {
    if cancel
        .map(|flag| flag.load(Ordering::SeqCst))
        .unwrap_or(false)
    {
        Err(SpiceError::Cancelled)
    } else {
        Ok(())
    }
}

fn hash_file_cancellable(path: &Path, cancel: Option<&AtomicBool>) -> Result<(String, u64)> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut hasher = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        check_cancel(cancel)?;
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
        size += count as u64;
    }
    Ok((format!("{:x}", hasher.finalize()), size))
}

pub fn list_manifests(config: &AppConfig) -> Result<Vec<SnapshotManifest>> {
    if config.cloud_root.trim().is_empty() {
        return Ok(Vec::new());
    }
    let directory = cloud_store_root(config).join("snapshots");
    if !directory.is_dir() {
        return Ok(Vec::new());
    }
    let mut manifests = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let manifest = read_json::<SnapshotManifest>(&path).map_err(|error| {
            SpiceError::CorruptSnapshot(format!(
                "Could not read completion manifest {}: {error}",
                path.display()
            ))
        })?;
        if !supported_snapshot_schema(manifest.schema_version)
            || path.file_stem().and_then(|value| value.to_str()) != Some(&manifest.id)
        {
            return Err(SpiceError::CorruptSnapshot(format!(
                "Manifest identity or format mismatch in {}",
                path.display()
            )));
        }
        manifests.push(manifest);
    }
    manifests.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
    Ok(manifests)
}

pub fn load_manifest(config: &AppConfig, id: &str) -> Result<SnapshotManifest> {
    if id.contains(['/', '\\']) || id.contains("..") {
        return Err(SpiceError::CorruptSnapshot(
            "Unsafe snapshot id".to_string(),
        ));
    }
    let path = cloud_store_root(config)
        .join("snapshots")
        .join(format!("{id}.json"));
    let manifest: SnapshotManifest = read_json(&path)?;
    if manifest.id != id || !supported_snapshot_schema(manifest.schema_version) {
        return Err(SpiceError::CorruptSnapshot(format!(
            "Manifest identity mismatch for {id}"
        )));
    }
    Ok(manifest)
}

pub fn verify_manifest(config: &AppConfig, manifest: &SnapshotManifest) -> Result<SnapshotSummary> {
    verify_manifest_cancellable(config, manifest, None)
}

/// Check the completion manifest's structure without hydrating or reading its
/// content objects. This reports visibility only; Pull must still verify bytes.
pub fn inspect_manifest(
    config: &AppConfig,
    manifest: &SnapshotManifest,
) -> Result<SnapshotSummary> {
    validate_manifest(config, manifest, None, false)
}

pub fn verify_manifest_cancellable(
    config: &AppConfig,
    manifest: &SnapshotManifest,
    cancel: Option<&AtomicBool>,
) -> Result<SnapshotSummary> {
    validate_manifest(config, manifest, cancel, true)
}

fn validate_manifest(
    config: &AppConfig,
    manifest: &SnapshotManifest,
    cancel: Option<&AtomicBool>,
    verify_content: bool,
) -> Result<SnapshotSummary> {
    if !supported_snapshot_schema(manifest.schema_version) {
        return Err(SpiceError::UnsupportedCodex(format!("Snapshot format {} requires a newer Spice Route version. This release reads formats 1 and 2.", manifest.schema_version)));
    }
    let store = ObjectStore {
        root: cloud_store_root(config).join("objects"),
        preview_only: false,
    };
    let thread_ids: HashSet<&str> = manifest
        .threads
        .iter()
        .map(|thread| thread.id.as_str())
        .collect();
    if thread_ids.len() != manifest.threads.len() {
        return Err(SpiceError::CorruptSnapshot(
            "The manifest contains duplicate chat identities.".to_string(),
        ));
    }
    let project_ids: HashSet<&str> = manifest
        .projects
        .iter()
        .map(|project| project.id.as_str())
        .collect();
    if project_ids.len() != manifest.projects.len() {
        return Err(SpiceError::CorruptSnapshot(
            "The manifest contains duplicate project identities.".to_string(),
        ));
    }
    let mut unique = HashSet::new();
    let mut content_sizes = HashMap::new();
    let artifact_objects: HashSet<(&str, &str)> = manifest
        .objects
        .iter()
        .filter(|object| matches!(object.kind, ObjectKind::Artifact))
        .map(|object| (object.owner_id.as_str(), object.logical_path.as_str()))
        .collect();
    for object in &manifest.objects {
        check_cancel(cancel)?;
        safe_relative(Path::new(&object.logical_path))?;
        store.object_path(&object.hash)?;
        validate_object_layout(manifest, object)?;
        if !unique.insert(&object.logical_path) {
            return Err(SpiceError::CorruptSnapshot(format!(
                "Duplicate logical path {}",
                object.logical_path
            )));
        }
        if matches!(object.kind, ObjectKind::Artifact) {
            let thread = manifest
                .threads
                .iter()
                .find(|thread| thread.id == object.owner_id)
                .ok_or_else(|| {
                    SpiceError::CorruptSnapshot(format!(
                        "Attachment {} belongs to missing chat {}.",
                        object.logical_path, object.owner_id
                    ))
                })?;
            if !thread
                .attachments
                .iter()
                .any(|reference| reference.logical_path == object.logical_path)
            {
                return Err(SpiceError::CorruptSnapshot(format!(
                    "Attachment {} has no chat reference.",
                    object.logical_path
                )));
            }
        }
        match content_sizes.insert(&object.hash, object.raw_size) {
            Some(size) if size != object.raw_size => {
                return Err(SpiceError::CorruptSnapshot(format!(
                    "Inconsistent content size for {}",
                    object.logical_path
                )));
            }
            None if verify_content => store.verify_cancellable(object, cancel)?,
            _ => {}
        }
    }
    for project in &manifest.projects {
        for descriptor in &project.git {
            if descriptor.project_id != project.id
                || descriptor.root_index >= project.source_roots.len()
            {
                return Err(SpiceError::CorruptSnapshot(format!(
                    "Project {} contains invalid Git root metadata.",
                    project.id
                )));
            }
            for (hash, kind, label) in [
                (
                    descriptor.bundle_object.as_deref(),
                    ObjectKind::GitBundle,
                    "history bundle",
                ),
                (
                    descriptor.index_object.as_deref(),
                    ObjectKind::GitIndex,
                    "index",
                ),
                (
                    descriptor.index_pack_object.as_deref(),
                    ObjectKind::GitObjectPack,
                    "index-object pack",
                ),
            ] {
                if let Some(hash) = hash {
                    let valid = manifest.objects.iter().any(|object| {
                        object.hash == hash && object.kind == kind && object.owner_id == project.id
                    });
                    if !valid {
                        return Err(SpiceError::CorruptSnapshot(format!(
                            "Project {} is missing its Git {label}.",
                            project.id
                        )));
                    }
                }
            }
        }
    }
    for object in manifest.objects.iter().filter(|object| {
        matches!(
            object.kind,
            ObjectKind::GitBundle | ObjectKind::GitIndex | ObjectKind::GitObjectPack
        )
    }) {
        let referenced = manifest
            .projects
            .iter()
            .flat_map(|project| &project.git)
            .any(|descriptor| match object.kind {
                ObjectKind::GitBundle => descriptor.bundle_object.as_deref() == Some(&object.hash),
                ObjectKind::GitIndex => descriptor.index_object.as_deref() == Some(&object.hash),
                ObjectKind::GitObjectPack => {
                    descriptor.index_pack_object.as_deref() == Some(&object.hash)
                }
                _ => false,
            });
        if !referenced {
            return Err(SpiceError::CorruptSnapshot(format!(
                "Git object {} is not referenced by a project.",
                object.logical_path
            )));
        }
    }
    for thread in &manifest.threads {
        let mut source_paths = HashSet::new();
        let mut logical_paths = HashSet::new();
        for reference in &thread.attachments {
            safe_relative(Path::new(&reference.logical_path))?;
            if reference.source_path.trim().is_empty()
                || !source_paths.insert(reference.source_path.to_lowercase())
                || !logical_paths.insert(reference.logical_path.as_str())
            {
                return Err(SpiceError::CorruptSnapshot(format!(
                    "Chat {} contains a duplicate or invalid attachment reference.",
                    thread.id
                )));
            }
            if !artifact_objects.contains(&(thread.id.as_str(), reference.logical_path.as_str())) {
                return Err(SpiceError::CorruptSnapshot(format!(
                    "Attachment reference {} is missing its content object.",
                    reference.logical_path
                )));
            }
        }
    }
    Ok(manifest.summary(
        manifest
            .objects
            .iter()
            .map(|object| object.stored_size)
            .sum(),
        verify_content,
    ))
}

fn validate_object_layout(manifest: &SnapshotManifest, object: &ObjectEntry) -> Result<()> {
    let segments: Vec<_> = object.logical_path.split('/').collect();
    if segments.iter().any(|segment| segment.is_empty()) {
        return Err(SpiceError::CorruptSnapshot(format!(
            "Object path has an empty component: {}",
            object.logical_path
        )));
    }
    let valid = match object.kind {
        ObjectKind::Rollout => {
            segments.len() == 3
                && segments[0] == "codex"
                && segments[1] == "rollouts"
                && segments[2] == format!("{}.jsonl", object.owner_id)
                && manifest
                    .threads
                    .iter()
                    .any(|thread| thread.id == object.owner_id)
        }
        ObjectKind::Artifact => {
            segments.len() >= 4
                && segments[0] == "codex"
                && segments[1] == "attachments"
                && segments[2] == object.owner_id
                && manifest
                    .threads
                    .iter()
                    .any(|thread| thread.id == object.owner_id)
        }
        ObjectKind::ProjectFile => {
            segments.len() >= 5
                && segments[0] == "projects"
                && segments[1] == object.owner_id
                && segments[3] == "files"
                && segments[2].parse::<usize>().ok().is_some_and(|index| {
                    manifest
                        .projects
                        .iter()
                        .find(|project| project.id == object.owner_id)
                        .map(|project| index < project.source_roots.len())
                        .unwrap_or(false)
                })
        }
        ObjectKind::ProjectlessFile => {
            segments.len() >= 4
                && segments[0] == "projectless"
                && segments[1] == object.owner_id
                && segments[2] == "files"
                && manifest
                    .threads
                    .iter()
                    .any(|thread| thread.id == object.owner_id && thread.projectless)
        }
        ObjectKind::GitBundle | ObjectKind::GitIndex | ObjectKind::GitObjectPack => {
            segments.len() == 5
                && segments[0] == "projects"
                && segments[1] == object.owner_id
                && segments[3] == "git"
                && segments[2].parse::<usize>().ok().is_some_and(|index| {
                    manifest
                        .projects
                        .iter()
                        .find(|project| project.id == object.owner_id)
                        .map(|project| index < project.source_roots.len())
                        .unwrap_or(false)
                })
        }
    };
    if valid {
        Ok(())
    } else {
        Err(SpiceError::CorruptSnapshot(format!(
            "Object metadata does not match its portable path: {}",
            object.logical_path
        )))
    }
}

pub fn latest_manifest(config: &AppConfig) -> Result<Option<SnapshotManifest>> {
    let mut heads = head_manifests(config)?;
    if heads.is_empty() {
        return Ok(None);
    }
    if heads.len() != 1 {
        return Err(SpiceError::CorruptSnapshot(format!("Expected one snapshot history head, but found {}. Keep both cloud copies for recovery and choose which history to continue.", heads.len())));
    }
    Ok(heads.pop())
}

pub fn head_manifests(config: &AppConfig) -> Result<Vec<SnapshotManifest>> {
    let manifests = list_manifests(config)?;
    let referenced: HashSet<&str> = manifests.iter().flat_map(manifest_parent_ids).collect();
    let by_id: HashMap<&str, &SnapshotManifest> = manifests
        .iter()
        .map(|manifest| (manifest.id.as_str(), manifest))
        .collect();
    let mut heads: Vec<_> = manifests
        .iter()
        .filter(|manifest| !referenced.contains(manifest.id.as_str()))
        .map(|manifest| (*manifest).clone())
        .collect();
    let mut visited = HashSet::new();
    for head in &heads {
        validate_ancestry_from(&head.id, &by_id, &mut HashSet::new(), &mut visited)?;
    }
    if !manifests.is_empty() && heads.is_empty() {
        return Err(SpiceError::CorruptSnapshot(
            "Snapshot ancestry has no head. It may contain a cycle.".to_string(),
        ));
    }
    if visited.len() != manifests.len() {
        return Err(SpiceError::CorruptSnapshot(
            "Snapshot ancestry contains a disconnected cycle or unreachable history.".to_string(),
        ));
    }
    heads.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(a.id.cmp(&b.id)));
    Ok(heads)
}

pub fn validate_ancestry(config: &AppConfig, head: &SnapshotManifest) -> Result<()> {
    let manifests = list_manifests(config)?;
    let by_id: HashMap<&str, &SnapshotManifest> = manifests
        .iter()
        .map(|manifest| (manifest.id.as_str(), manifest))
        .collect();
    validate_ancestry_from(&head.id, &by_id, &mut HashSet::new(), &mut HashSet::new())
}

pub fn common_ancestor(
    config: &AppConfig,
    local_id: Option<&str>,
    incoming_id: &str,
) -> Result<Option<SnapshotManifest>> {
    let Some(local_id) = local_id else {
        return Ok(None);
    };
    let manifests = list_manifests(config)?;
    let by_id: HashMap<&str, &SnapshotManifest> = manifests
        .iter()
        .map(|manifest| (manifest.id.as_str(), manifest))
        .collect();
    if !by_id.contains_key(local_id) {
        return Err(SpiceError::CorruptSnapshot(format!(
            "The local baseline snapshot {local_id} is no longer present in the cloud folder."
        )));
    }
    let local = ancestor_distances(local_id, &by_id)?;
    let incoming = ancestor_distances(incoming_id, &by_id)?;
    let best = local
        .iter()
        .filter_map(|(id, local_distance)| {
            incoming.get(id).map(|incoming_distance| {
                (id, local_distance + incoming_distance, incoming_distance)
            })
        })
        .min_by_key(|(id, total, incoming_distance)| (*total, **incoming_distance, id.as_str()))
        .map(|(id, _, _)| id);
    Ok(best.and_then(|id| by_id.get(id.as_str()).copied()).cloned())
}

fn manifest_parent_ids(manifest: &SnapshotManifest) -> impl Iterator<Item = &str> {
    manifest
        .parent_id
        .iter()
        .map(String::as_str)
        .chain(manifest.additional_parent_ids.iter().map(String::as_str))
}

fn validate_ancestry_from<'a>(
    id: &'a str,
    by_id: &HashMap<&'a str, &'a SnapshotManifest>,
    visiting: &mut HashSet<&'a str>,
    visited: &mut HashSet<&'a str>,
) -> Result<()> {
    if visited.contains(id) {
        return Ok(());
    }
    if !visiting.insert(id) {
        return Err(SpiceError::CorruptSnapshot(format!(
            "Snapshot ancestry contains a cycle at {id}."
        )));
    }
    let manifest = by_id.get(id).copied().ok_or_else(|| {
        SpiceError::CorruptSnapshot(format!("Snapshot ancestry is missing {id}."))
    })?;
    for parent in manifest_parent_ids(manifest) {
        validate_ancestry_from(parent, by_id, visiting, visited)?;
    }
    visiting.remove(id);
    visited.insert(id);
    Ok(())
}

fn ancestor_distances<'a>(
    start: &'a str,
    by_id: &HashMap<&'a str, &'a SnapshotManifest>,
) -> Result<HashMap<String, usize>> {
    let mut result = HashMap::new();
    let mut queue = VecDeque::from([(start, 0_usize)]);
    while let Some((id, distance)) = queue.pop_front() {
        if result
            .get(id)
            .map(|known| *known <= distance)
            .unwrap_or(false)
        {
            continue;
        }
        result.insert(id.to_string(), distance);
        let manifest = by_id.get(id).copied().ok_or_else(|| {
            SpiceError::CorruptSnapshot(format!("Snapshot ancestry is missing {id}."))
        })?;
        for parent in manifest_parent_ids(manifest) {
            queue.push_back((parent, distance + 1));
        }
    }
    Ok(result)
}

fn is_executable(metadata: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        false
    }
}

fn set_executable(path: &Path, executable: bool) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if executable {
            let mut permissions = fs::metadata(path)?.permissions();
            permissions.set_mode(permissions.mode() | 0o755);
            fs::set_permissions(path, permissions)?;
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (path, executable);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use tempfile::tempdir;

    fn manifest(
        id: &str,
        parent: Option<&str>,
        additional: &[&str],
        config: &AppConfig,
    ) -> SnapshotManifest {
        SnapshotManifest {
            schema_version: SNAPSHOT_SCHEMA,
            id: id.to_string(),
            created_at: format!("2026-01-01T00:00:0{}Z", id.len()),
            device_id: "device".to_string(),
            device_name: "Device".to_string(),
            parent_id: parent.map(str::to_string),
            additional_parent_ids: additional
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
            selection_revision: config.selection.revision.clone(),
            selection: config.selection.clone(),
            compatibility: crate::models::CompatibilityInfo {
                supported: true,
                adapter: "test".to_string(),
                state_migration: Some(1),
                history_migration: Some(1),
                schema_fingerprint: "test".to_string(),
                explanation: "test".to_string(),
            },
            codex_version: Some("test".to_string()),
            threads: Vec::new(),
            projects: Vec::new(),
            objects: Vec::new(),
            ui_state: serde_json::json!({}),
            session_index_lines: Vec::new(),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn workspace_estimates_follow_capture_exclusions_and_overrides() {
        let root = tempdir().unwrap();
        for (relative, size) in [
            ("src/main.rs", 3),
            ("node_modules/dependency.js", 9),
            ("target/app.exe", 7),
            (".env", 5),
            ("notes.tmp", 2),
            (".git/objects/data", 11),
        ] {
            let path = root.path().join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, vec![b'x'; size]).unwrap();
        }
        let mut config = settings::default_config();
        assert_eq!(
            estimate_workspace_bytes(root.path(), &config.selection).unwrap(),
            5
        );
        config
            .selection
            .extra_exclude_patterns
            .push("**/*.tmp".into());
        assert_eq!(
            estimate_workspace_bytes(root.path(), &config.selection).unwrap(),
            3
        );
        config.selection.include_sensitive_files = true;
        assert_eq!(
            estimate_workspace_bytes(root.path(), &config.selection).unwrap(),
            8
        );
        config.selection.include_build_outputs = true;
        let estimate = estimate_workspace_bytes(root.path(), &config.selection).unwrap();
        assert_eq!(estimate, 24);
        let store = ObjectStore::for_preview(root.path().join("not-created"));
        let mut objects = vec![];
        capture_tree(
            root.path(),
            "projects/project/0/files",
            "project",
            ObjectKind::ProjectFile,
            &config,
            &store,
            &mut objects,
            &mut vec![],
            None,
        )
        .unwrap();
        assert_eq!(
            objects.iter().map(|object| object.raw_size).sum::<u64>(),
            estimate
        );
        assert_eq!(
            estimate_workspace_bytes(&root.path().join("missing"), &config.selection).unwrap(),
            0
        );
    }

    #[test]
    fn hash_only_preview_matches_capture_without_writing_objects() {
        let root = tempdir().unwrap();
        let source = root.path().join("workspace.bin");
        fs::write(&source, vec![42_u8; 2 * 1024 * 1024]).unwrap();
        let preview_path = root.path().join("preview-objects");
        let preview = ObjectStore::for_preview(&preview_path);
        let actual = ObjectStore::new(root.path().join("captured-objects")).unwrap();
        let fingerprint = preview
            .put_file(
                &source,
                "project/file".into(),
                ObjectKind::ProjectFile,
                "project".into(),
            )
            .unwrap();
        let captured = actual
            .put_file(
                &source,
                "project/file".into(),
                ObjectKind::ProjectFile,
                "project".into(),
            )
            .unwrap();
        assert_eq!(fingerprint.hash, captured.hash);
        assert_eq!(fingerprint.raw_size, captured.raw_size);
        assert_eq!(fingerprint.stored_size, 0);
        assert!(captured.stored_size > 0);
        assert!(!preview_path.exists());

        let config = settings::default_config();
        let mut draft = manifest("preview", None, &[], &config);
        draft.objects.push(fingerprint);
        let mut complete = draft.clone();
        complete.objects[0] = captured;
        assert_eq!(
            manifest_fingerprint(&draft).unwrap(),
            manifest_fingerprint(&complete).unwrap()
        );
        complete.objects[0].hash = sha256_bytes(b"changed");
        assert_ne!(
            manifest_fingerprint(&draft).unwrap(),
            manifest_fingerprint(&complete).unwrap()
        );
        assert!(publish(&config, &preview, &draft)
            .unwrap_err()
            .to_string()
            .contains("preview"));
        let cancelled = AtomicBool::new(true);
        assert!(matches!(
            preview.put_file_cancellable(
                &source,
                "file".into(),
                ObjectKind::ProjectFile,
                "project".into(),
                Some(&cancelled)
            ),
            Err(SpiceError::Cancelled)
        ));
        assert!(!preview_path.exists());
    }

    #[test]
    fn manifest_inspection_checks_layout_without_hydrating_content() {
        let root = tempdir().unwrap();
        let mut config = settings::default_config();
        config.cloud_root = root.path().join("cloud").to_string_lossy().into_owned();
        let mut value = manifest("visible", None, &[], &config);
        value.projects.push(ProjectExport {
            id: "project".into(),
            legacy_id: None,
            name: "Project".into(),
            mode: ProjectMode::Full,
            source_roots: vec!["source".into()],
            rows: Default::default(),
            git: vec![],
        });
        value.objects.push(ObjectEntry {
            hash: sha256_bytes(b"content not delivered yet"),
            logical_path: "projects/project/0/files/readme.txt".into(),
            kind: ObjectKind::ProjectFile,
            owner_id: "project".into(),
            raw_size: 25,
            stored_size: 30,
            executable: false,
        });
        assert!(!inspect_manifest(&config, &value).unwrap().verified);
        assert!(!Path::new(&config.cloud_root).exists());
        assert!(verify_manifest(&config, &value)
            .unwrap_err()
            .to_string()
            .contains("Missing object"));
        assert!(!Path::new(&config.cloud_root).exists());
        let mut invalid = value.clone();
        invalid.objects[0].logical_path = "projects/project/0/files/../../escape".into();
        assert!(inspect_manifest(&config, &invalid).is_err());
        invalid = value.clone();
        invalid.objects[0].hash = "invalid hash".into();
        assert!(inspect_manifest(&config, &invalid).is_err());
        invalid = value.clone();
        invalid.objects.push(value.objects[0].clone());
        assert!(inspect_manifest(&config, &invalid).is_err());
        invalid.objects[1].logical_path = "projects/project/0/files/other.txt".into();
        invalid.objects[1].raw_size += 1;
        assert!(inspect_manifest(&config, &invalid).is_err());
    }

    #[test]
    fn corrupt_materialization_leaves_existing_destination_untouched() {
        let root = tempdir().unwrap();
        let store = ObjectStore::new(root.path().join("objects")).unwrap();
        let object = store
            .put_bytes(
                b"expected",
                "file".into(),
                ObjectKind::Artifact,
                "thread".into(),
            )
            .unwrap();
        let corrupted = zstd::stream::encode_all(&b"replaced"[..], 6).unwrap();
        fs::write(store.object_path(&object.hash).unwrap(), corrupted).unwrap();
        let folder = root.path().join("destination");
        fs::create_dir(&folder).unwrap();
        let target = folder.join("file.txt");
        fs::write(&target, b"keep my file").unwrap();
        assert!(matches!(
            store.materialize(&object, &target),
            Err(SpiceError::CorruptSnapshot(_))
        ));
        assert_eq!(fs::read(&target).unwrap(), b"keep my file");
        assert_eq!(fs::read_dir(folder).unwrap().count(), 1);
    }

    #[test]
    fn missing_full_source_blocks_push_but_allows_pull_staging() {
        let root = tempdir().unwrap();
        let store = ObjectStore::new(root.path().join("objects")).unwrap();
        let mut config = settings::default_config();
        let missing = root.path().join("unavailable-project");
        let reference = manifest("source", None, &[], &config);
        let mut export = CodexExport {
            compatibility: reference.compatibility,
            threads: vec![],
            projects: vec![ProjectExport {
                id: "project-a".into(),
                legacy_id: None,
                name: "Project A".into(),
                mode: ProjectMode::Full,
                source_roots: vec![missing.to_string_lossy().into_owned()],
                rows: Default::default(),
                git: vec![],
            }],
            pending_files: vec![],
            ui_state: serde_json::json!({}),
            session_index_lines: vec![],
            warnings: vec![],
        };
        let error = build_manifest_cancellable(&config, export.clone(), &store, None, None, true)
            .unwrap_err();
        assert!(error.to_string().contains("Cannot Push"));
        let pull = build_manifest(&config, export.clone(), &store, None).unwrap();
        assert!(pull.objects.is_empty());
        assert!(pull
            .warnings
            .iter()
            .any(|warning| warning.contains("root is missing")));

        let corrected = root.path().join("corrected-project");
        fs::create_dir_all(&corrected).unwrap();
        config.source_roots.insert(
            "project-a:0".into(),
            corrected.to_string_lossy().into_owned(),
        );
        build_manifest_cancellable(&config, export.clone(), &store, None, None, true).unwrap();
        config.source_roots.insert(
            "project-a:0".into(),
            root.path().join("typo").to_string_lossy().into_owned(),
        );
        assert!(
            build_manifest_cancellable(&config, export.clone(), &store, None, None, true).is_err()
        );
        export.projects[0].mode = ProjectMode::HistoryOnly;
        build_manifest_cancellable(&config, export, &store, None, None, true).unwrap();
    }

    #[test]
    fn reads_existing_format_one_and_new_format_two_without_republishing_ancestors() {
        let root = tempdir().unwrap();
        let mut config = settings::default_config();
        config.cloud_root = root.path().to_string_lossy().into_owned();
        let directory = cloud_store_root(&config).join("snapshots");
        fs::create_dir_all(&directory).unwrap();
        let mut old = manifest("old", None, &[], &config);
        old.schema_version = 1;
        let new = manifest("new", Some("old"), &[], &config);
        crate::util::write_json(&directory.join("old.json"), &old).unwrap();
        crate::util::write_json(&directory.join("new.json"), &new).unwrap();
        let old_bytes = fs::read(directory.join("old.json")).unwrap();
        assert_eq!(load_manifest(&config, "old").unwrap().schema_version, 1);
        assert_eq!(list_manifests(&config).unwrap().len(), 2);
        assert_eq!(head_manifests(&config).unwrap()[0].id, "new");
        verify_manifest(&config, &old).unwrap();
        verify_manifest(&config, &new).unwrap();
        assert_eq!(fs::read(directory.join("old.json")).unwrap(), old_bytes);
        let mut unknown = new;
        unknown.schema_version = 99;
        assert!(verify_manifest(&config, &unknown)
            .unwrap_err()
            .to_string()
            .contains("newer Spice Route"));
    }

    #[test]
    fn source_override_captures_chosen_folder_without_changing_project_identity() {
        let original = tempdir().unwrap();
        let chosen = tempdir().unwrap();
        let storage = tempdir().unwrap();
        fs::write(original.path().join("code.txt"), b"old location").unwrap();
        fs::write(chosen.path().join("code.txt"), b"chosen location").unwrap();
        fs::write(chosen.path().join("image.png"), b"moved image").unwrap();
        let mut config = settings::default_config();
        config.source_roots.insert(
            "project-a:0".into(),
            chosen.path().to_string_lossy().into_owned(),
        );
        let mut source_manifest = manifest("source", None, &[], &config);
        source_manifest.projects.push(ProjectExport {
            id: "project-a".into(),
            legacy_id: None,
            name: "Project A".into(),
            mode: ProjectMode::Full,
            source_roots: vec![original.path().to_string_lossy().into_owned()],
            rows: Default::default(),
            git: vec![],
        });
        let export = CodexExport {
            compatibility: source_manifest.compatibility,
            threads: vec![crate::models::ThreadExport {
                id: "chat-a".into(),
                title: "Chat A".into(),
                project_id: Some("project-a".into()),
                projectless: false,
                archived: false,
                source_cwd: original.path().to_string_lossy().into_owned(),
                rollout_relative_path: None,
                projectless_relative_root: None,
                attachments: vec![crate::models::AttachmentReference {
                    source_path: original
                        .path()
                        .join("image.png")
                        .to_string_lossy()
                        .into_owned(),
                    logical_path: "codex/attachments/chat-a/image.png".into(),
                }],
                state_rows: Default::default(),
                history_rows: Default::default(),
                fingerprint: "history".into(),
            }],
            projects: source_manifest.projects.clone(),
            pending_files: vec![crate::codex::PendingFile {
                logical_path: "codex/attachments/chat-a/image.png".into(),
                owner_id: "chat-a".into(),
                source: original.path().join("image.png"),
                kind: PendingFileKind::Attachment,
            }],
            ui_state: serde_json::json!({}),
            session_index_lines: vec![],
            warnings: vec![],
        };
        let store = ObjectStore::new(storage.path()).unwrap();
        let captured = build_manifest(&config, export, &store, None).unwrap();
        assert_eq!(
            captured.projects[0].source_roots,
            source_manifest.projects[0].source_roots
        );
        assert_eq!(captured.projects[0].id, "project-a");
        assert_eq!(captured.objects.len(), 3);
        assert!(captured
            .objects
            .iter()
            .any(|object| object.kind == ObjectKind::ProjectFile
                && object.hash == sha256_bytes(b"chosen location")));
        assert!(captured
            .objects
            .iter()
            .any(|object| object.kind == ObjectKind::Artifact
                && object.hash == sha256_bytes(b"moved image")));
        assert_eq!(
            captured.threads[0].source_cwd,
            original.path().to_string_lossy()
        );
        assert_eq!(
            captured.threads[0].attachments[0].source_path,
            original.path().join("image.png").to_string_lossy()
        );
        assert_eq!(
            fs::read(original.path().join("code.txt")).unwrap(),
            b"old location"
        );

        let mut shared = captured.projects.clone();
        let mut second = shared[0].clone();
        second.id = "project-b".into();
        shared.push(second);
        assert!(validate_source_folders(&config, &shared)
            .unwrap_err()
            .to_string()
            .contains("share one workspace"));
        config.source_roots.insert(
            "project-b:0".into(),
            chosen.path().to_string_lossy().into_owned(),
        );
        validate_source_folders(&config, &shared).unwrap();
    }

    #[test]
    fn object_round_trip_verifies_content() {
        let dir = tempdir().unwrap();
        let output = tempdir().unwrap();
        let store = ObjectStore::new(dir.path()).unwrap();
        let object = store
            .put_bytes(
                b"portable session",
                "test/value".to_string(),
                ObjectKind::Artifact,
                "test".to_string(),
            )
            .unwrap();
        store.verify(&object).unwrap();
        let target = output.path().join("value.bin");
        store.materialize(&object, &target).unwrap();
        assert_eq!(fs::read(target).unwrap(), b"portable session");
    }

    #[test]
    fn publishing_repairs_a_corrupt_existing_cloud_object() {
        let source_root = tempdir().unwrap();
        let cloud_root = tempdir().unwrap();
        let source = ObjectStore::new(source_root.path()).unwrap();
        let cloud = ObjectStore::new(cloud_root.path()).unwrap();
        let object = source
            .put_bytes(
                b"verified content",
                "test/value".to_string(),
                ObjectKind::Artifact,
                "test".to_string(),
            )
            .unwrap();
        let cloud_path = cloud.object_path(&object.hash).unwrap();
        fs::create_dir_all(cloud_path.parent().unwrap()).unwrap();
        fs::write(&cloud_path, b"corrupt").unwrap();

        cloud.import_from(&source, &object).unwrap();

        cloud.verify(&object).unwrap();
        let before = fs::read(&cloud_path).unwrap();
        let cancelled = AtomicBool::new(true);
        assert!(matches!(
            cloud.import_from_cancellable(&source, &object, Some(&cancelled)),
            Err(SpiceError::Cancelled)
        ));
        assert_eq!(fs::read(&cloud_path).unwrap(), before);
    }

    #[test]
    fn default_sensitive_patterns_are_excluded() {
        let mut config = crate::settings::default_config();
        let patterns = build_exclusions(&[]).unwrap();
        assert!(excluded_relative("app/.env", &config, &patterns));
        assert!(excluded_relative("keys/id_ed25519", &config, &patterns));
        assert!(excluded_relative("app/.npmrc", &config, &patterns));
        assert!(excluded_relative(
            "infra/prod.auto.tfvars",
            &config,
            &patterns
        ));
        assert!(excluded_relative(
            "home/.docker/config.json",
            &config,
            &patterns
        ));
        assert!(excluded_relative("nested/.git", &config, &patterns));
        assert!(!excluded_relative("app/.env.example", &config, &patterns));
        assert!(!excluded_relative("src/credentials.ts", &config, &patterns));
        config.selection.include_sensitive_files = true;
        assert!(!excluded_relative("app/.env", &config, &patterns));
    }

    #[test]
    fn cancelled_capture_removes_partial_object() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("large.bin");
        fs::write(&source, vec![42_u8; 2 * 1024 * 1024]).unwrap();
        let store = ObjectStore::new(dir.path().join("objects")).unwrap();
        let cancelled = AtomicBool::new(true);
        let result = store.put_file_cancellable(
            &source,
            "large.bin".to_string(),
            ObjectKind::Artifact,
            "test".to_string(),
            Some(&cancelled),
        );
        assert!(matches!(result, Err(SpiceError::Cancelled)));
        assert_eq!(fs::read_dir(dir.path().join("objects")).unwrap().count(), 0);
    }

    #[test]
    fn concurrent_heads_become_one_after_merge_snapshot() {
        let root = tempdir().unwrap();
        let data = root.path().join("data");
        let codex = root.path().join("codex");
        let projectless = root.path().join("projectless");
        let cloud = root.path().join("cloud");
        fs::create_dir_all(&codex).unwrap();
        fs::create_dir_all(&projectless).unwrap();
        fs::create_dir_all(&cloud).unwrap();
        let mut config = crate::settings::default_config();
        config.onboarding_complete = true;
        config.codex_home = codex.to_string_lossy().into_owned();
        config.projectless_root = projectless.to_string_lossy().into_owned();
        config.cloud_root = cloud.to_string_lossy().into_owned();
        crate::settings::save_config(&data, &config).unwrap();
        let snapshots = cloud_store_root(&config).join("snapshots");
        for value in [
            manifest("root", None, &[], &config),
            manifest("branch-a", Some("root"), &[], &config),
            manifest("branch-b", Some("root"), &[], &config),
        ] {
            crate::util::write_json(&snapshots.join(format!("{}.json", value.id)), &value).unwrap();
        }

        let heads = head_manifests(&config).unwrap();
        assert_eq!(sorted_manifest_ids(&heads), vec!["branch-a", "branch-b"]);
        assert_eq!(
            common_ancestor(&config, Some("branch-a"), "branch-b")
                .unwrap()
                .unwrap()
                .id,
            "root"
        );

        let merged = manifest("merged", Some("branch-a"), &["branch-b"], &config);
        crate::util::write_json(&snapshots.join("merged.json"), &merged).unwrap();
        assert_eq!(
            head_manifests(&config)
                .unwrap()
                .into_iter()
                .map(|item| item.id)
                .collect::<Vec<_>>(),
            vec!["merged"]
        );
        assert_eq!(
            common_ancestor(&config, Some("branch-b"), "merged")
                .unwrap()
                .unwrap()
                .id,
            "branch-b"
        );
    }

    fn sorted_manifest_ids(values: &[SnapshotManifest]) -> Vec<&str> {
        let mut ids: Vec<_> = values.iter().map(|value| value.id.as_str()).collect();
        ids.sort();
        ids
    }
}
