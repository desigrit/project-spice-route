use crate::codex;
use crate::error::{Result, SpiceError};
use crate::models::*;
use crate::platform;
use crate::recovery;
use crate::settings;
use crate::snapshot::{self, ObjectStore};
use crate::util::{
    directory_size, hash_json, paths_overlap, read_json, replace_file, safe_relative, sha256_file,
    write_json,
};
use fs2::FileExt;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use uuid::Uuid;
use walkdir::WalkDir;

const MIN_OPERATION_HEADROOM: u64 = 64 * 1024 * 1024;

#[derive(Clone)]
struct PreparedOperation {
    preview: OperationPreview,
    config_fingerprint: String,
    local_fingerprint: String,
    transfer_contract: Option<(String, String)>,
    expected_latest: Option<String>,
    expected_head_ids: Vec<String>,
    additional_parent_ids: Vec<String>,
    stage_dir: PathBuf,
    cancel: Arc<AtomicBool>,
    progress: Arc<Mutex<OperationProgress>>,
}

#[derive(Clone)]
struct PreparedCloudCleanup {
    preview: CloudCleanupPreview,
    config_fingerprint: String,
    store_fingerprint: String,
}

pub struct Engine {
    pub data_dir: PathBuf,
    operations: Mutex<HashMap<String, PreparedOperation>>,
    cleanups: Mutex<HashMap<String, PreparedCloudCleanup>>,
    _instance_lock: File,
}

impl Engine {
    pub fn new(data_dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&data_dir)?;
        let lock = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(data_dir.join("instance.lock"))?;
        FileExt::try_lock_exclusive(&lock).map_err(|_| SpiceError::User(
            "Spice Route is already running. Use the existing window before starting another copy.".to_string(),
        ))?;
        let previews = data_dir.join("previews");
        if previews.is_dir() {
            fs::remove_dir_all(&previews)?;
        }
        fs::create_dir_all(&previews)?;
        Ok(Self {
            data_dir,
            operations: Mutex::new(HashMap::new()),
            cleanups: Mutex::new(HashMap::new()),
            _instance_lock: lock,
        })
    }

    pub fn load_config(&self) -> Result<AppConfig> {
        settings::load_config(&self.data_dir)
    }

    pub fn save_config(&self, config: AppConfig) -> Result<AppConfig> {
        settings::save_config(&self.data_dir, &config)?;
        Ok(config)
    }

    pub fn discover_environment(&self) -> Result<EnvironmentDiscovery> {
        let config = self
            .load_config()
            .unwrap_or_else(|_| settings::default_config());
        let configured = PathBuf::from(&config.codex_home);
        let home = if configured.exists() {
            Some(configured)
        } else {
            settings::discover_codex_home()
        };
        let resolved = home
            .as_ref()
            .and_then(|path| dunce::canonicalize(path).ok());
        let executable = platform::find_codex_executable();
        let codex_version = platform::codex_version(executable.as_deref());
        let compatibility = home.as_ref().and_then(|path| {
            codex::inspect(path)
                .ok()
                .map(|info| codex::with_build_gate(info, codex_version.as_deref()))
        });
        let mut warnings = Vec::new();
        if executable.is_none() {
            warnings.push("Codex CLI was not found. Session discovery works, but rollout migrations cannot be diagnosed.".to_string());
        }
        if let Some(info) = &compatibility {
            if !info.supported {
                warnings.push(info.explanation.clone());
            }
        }
        Ok(EnvironmentDiscovery {
            codex_home: home.as_ref().map(|path| path_string(path)),
            codex_home_resolved: resolved.as_ref().map(|path| path_string(path)),
            codex_version,
            codex_executable: executable.as_ref().map(|path| path_string(path)),
            codex_running: platform::codex_running(),
            cloud_candidates: platform::cloud_candidates(),
            compatibility,
            warnings,
        })
    }

    pub fn list_content(&self, config: &AppConfig) -> Result<ContentCatalog> {
        self.list_content_with_sizes(config, true)
    }

    /// Counts and associations for startup, without walking every working tree.
    /// The selection page can request detailed workspace estimates separately.
    pub fn list_content_quick(&self, config: &AppConfig) -> Result<ContentCatalog> {
        self.list_content_with_sizes(config, false)
    }

    fn list_content_with_sizes(
        &self,
        config: &AppConfig,
        include_workspace_sizes: bool,
    ) -> Result<ContentCatalog> {
        settings::validate_config(config)?;
        let mut catalog = codex::list_content(Path::new(&config.codex_home))?;
        for project in &mut catalog.projects {
            project.local_roots = project
                .roots
                .iter()
                .enumerate()
                .map(|(index, root)| {
                    path_string(&settings::source_project_folder(
                        config,
                        &project.id,
                        index,
                        root,
                    ))
                })
                .collect();
            project.estimated_bytes = 0;
            if include_workspace_sizes {
                for root in &project.local_roots {
                    match snapshot::estimate_workspace_bytes(Path::new(root), &config.selection) {
                        Ok(bytes) => {
                            project.estimated_bytes = project.estimated_bytes.saturating_add(bytes);
                        }
                        Err(error) => catalog.warnings.push(format!(
                            "{} has an incomplete workspace size estimate: {error}",
                            project.name
                        )),
                    }
                }
            }
            project.git_repository = project
                .local_roots
                .iter()
                .any(|root| Path::new(root).join(".git").exists());
            project.linked_worktree = project
                .local_roots
                .iter()
                .any(|root| Path::new(root).join(".git").is_file());
        }
        catalog.total_estimated_bytes = catalog
            .threads
            .iter()
            .map(|thread| thread.estimated_bytes)
            .sum::<u64>()
            + catalog
                .projects
                .iter()
                .map(|project| project.estimated_bytes)
                .sum::<u64>();
        Ok(catalog)
    }

    pub fn sync_status(&self, config: &AppConfig) -> Result<SyncStatus> {
        if !config.onboarding_complete || config.cloud_root.trim().is_empty() {
            return Ok(SyncStatus {
                latest_snapshot: None,
                visible_heads: Vec::new(),
                last_applied_snapshot_id: None,
                last_pushed_snapshot_id: None,
                cloud_bytes: 0,
                incoming_available: false,
                merge_ready: false,
                pending_recovery: recovery::has_pending(&self.data_dir),
                state: SyncState::NeedsSetup,
                message: "Finish setup to create the first handoff.".to_string(),
            });
        }
        let state = settings::load_local_state(&self.data_dir)?;
        let heads = snapshot::head_manifests(config)?;
        let mut visible_heads = Vec::with_capacity(heads.len());
        for head in &heads {
            visible_heads.push(snapshot::inspect_manifest(config, head)?);
        }
        let latest_snapshot = (visible_heads.len() == 1).then(|| visible_heads[0].clone());
        let incoming_available = heads
            .iter()
            .any(|manifest| Some(&manifest.id) != state.last_applied_snapshot_id.as_ref());
        let head_ids = sorted_ids(heads.iter().map(|manifest| manifest.id.clone()));
        let merge_ready = heads.len() > 1
            && sorted_ids(state.pending_merge_parent_ids.clone()) == head_ids
            && state
                .last_applied_snapshot_id
                .as_ref()
                .map(|id| head_ids.contains(id))
                .unwrap_or(false);
        let pending = recovery::has_pending(&self.data_dir);
        let cleanup_pending = self.data_dir.join("cloud-cleanup.json").is_file();
        let compatibility = codex::with_build_gate(
            codex::inspect(Path::new(&config.codex_home))?,
            platform::codex_version(platform::find_codex_executable().as_deref()).as_deref(),
        );
        let (sync_state, message) = if cleanup_pending {
            (SyncState::Blocked, "A cloud-history cleanup was interrupted. Open Settings and run Reset cloud history again.".to_string())
        } else if pending {
            (
                SyncState::Blocked,
                "An interrupted pull needs attention in Recovery.".to_string(),
            )
        } else if !compatibility.supported {
            (
                SyncState::Blocked,
                "This Codex version is available for diagnostics only.".to_string(),
            )
        } else if heads.len() > 1 && merge_ready {
            (
                SyncState::Ready,
                "The selected branch is merged locally. Push to publish one combined history."
                    .to_string(),
            )
        } else if heads.len() > 1 {
            (
                SyncState::NeedsPull,
                format!(
                    "{} cloud branches are visible. Review one to combine the histories.",
                    heads.len()
                ),
            )
        } else if incoming_available {
            (
                SyncState::NeedsPull,
                if state.last_applied_snapshot_id.is_none() {
                    "This device has no saved sync baseline. Review the visible snapshot with Pull; choose which versions to keep before pushing.".to_string()
                } else {
                    "An incoming snapshot is ready to review.".to_string()
                },
            )
        } else {
            (
                SyncState::Ready,
                "This device is aligned with the latest visible snapshot.".to_string(),
            )
        };
        Ok(SyncStatus {
            latest_snapshot,
            visible_heads,
            last_applied_snapshot_id: state.last_applied_snapshot_id,
            last_pushed_snapshot_id: state.last_pushed_snapshot_id,
            cloud_bytes: directory_size(&settings::cloud_store_root(config)),
            incoming_available,
            merge_ready,
            pending_recovery: pending,
            state: sync_state,
            message,
        })
    }

    pub fn preview_push(&self, config: &AppConfig) -> Result<OperationPreview> {
        self.validate_operation(config)?;
        ensure_disk_space(
            &self.data_dir,
            MIN_OPERATION_HEADROOM,
            "prepare a Push preview",
        )?;
        let operation_id = Uuid::new_v4().to_string();
        let heads = snapshot::head_manifests(config)?;
        let expected_head_ids = sorted_ids(heads.iter().map(|item| item.id.clone()));
        let state = settings::load_local_state(&self.data_dir)?;
        let mut blocked_reasons = Vec::new();
        let merge_ready = heads.len() > 1
            && sorted_ids(state.pending_merge_parent_ids.clone()) == expected_head_ids
            && state
                .last_applied_snapshot_id
                .as_ref()
                .map(|id| expected_head_ids.contains(id))
                .unwrap_or(false);
        if heads.len() > 1 && !merge_ready {
            blocked_reasons.push("Several cloud branches are visible. Review and Pull one branch before publishing a merge snapshot.".to_string());
        }
        let primary = if heads.len() == 1 {
            Some(&heads[0])
        } else {
            state
                .last_applied_snapshot_id
                .as_deref()
                .and_then(|id| heads.iter().find(|item| item.id == id))
                .or_else(|| heads.first())
        };
        if heads.len() == 1
            && state.last_applied_snapshot_id.as_deref() != primary.map(|item| item.id.as_str())
        {
            blocked_reasons.push(if state.last_applied_snapshot_id.is_none() {
                format!("This device has no saved sync baseline. Review snapshot {} with Pull and choose which versions to keep. Completing that review enables Push.", short_id(&heads[0].id))
            } else {
                format!("Review and Pull snapshot {} before pushing. Resolve any conflicts to combine this device's work with the incoming history.", short_id(&heads[0].id))
            });
        }
        // A blocked Push cannot use a file preview. Return the actionable reason
        // before reading workspaces or making Git bundles.
        if !blocked_reasons.is_empty() {
            return Ok(OperationPreview {
                operation_id,
                direction: Direction::Push,
                snapshot_id: primary.map(|item| item.id.clone()),
                estimated_bytes: 0,
                changes: Vec::new(),
                warnings: Vec::new(),
                blocked_reasons,
                requires_codex_close: false,
                required_mappings: Vec::new(),
            });
        }
        let stage = self.preview_directory(&operation_id)?;
        let additional_parent_ids = if merge_ready {
            expected_head_ids
                .iter()
                .filter(|id| Some(id.as_str()) != primary.map(|item| item.id.as_str()))
                .cloned()
                .collect()
        } else {
            Vec::new()
        };
        let mut manifest = self.capture_local_cancellable(
            config,
            &stage,
            primary.map(|item| item.id.clone()),
            None,
            true,
            false,
        )?;
        manifest.additional_parent_ids = additional_parent_ids.clone();
        let local_fingerprint = snapshot::manifest_fingerprint(&manifest)?;
        let mut changes = diff_for_push(primary, &manifest);
        changes.sort_by(change_order);
        let preview = OperationPreview {
            operation_id: operation_id.clone(),
            direction: Direction::Push,
            snapshot_id: Some(manifest.id.clone()),
            estimated_bytes: changes
                .iter()
                .filter(|change| {
                    !matches!(
                        change.action,
                        ChangeAction::Unchanged | ChangeAction::Delete
                    )
                })
                .map(|change| change.bytes)
                .sum(),
            changes,
            warnings: manifest.warnings.clone(),
            blocked_reasons,
            requires_codex_close: platform::codex_running(),
            required_mappings: Vec::new(),
        };
        self.store_prepared(PreparedOperation {
            preview: preview.clone(),
            config_fingerprint: config_fingerprint(config)?,
            local_fingerprint,
            transfer_contract: None,
            expected_latest: primary.map(|item| item.id.clone()),
            expected_head_ids,
            additional_parent_ids,
            stage_dir: stage,
            cancel: Arc::new(AtomicBool::new(false)),
            progress: new_progress(&operation_id, 5),
        })?;
        Ok(preview)
    }

    pub fn preview_pull(
        &self,
        config: &AppConfig,
        snapshot_id: Option<&str>,
    ) -> Result<OperationPreview> {
        self.validate_operation(config)?;
        ensure_disk_space(
            &self.data_dir,
            MIN_OPERATION_HEADROOM,
            "prepare a Pull preview",
        )?;
        let heads = snapshot::head_manifests(config)?;
        let expected_head_ids = sorted_ids(heads.iter().map(|item| item.id.clone()));
        let incoming = match snapshot_id {
            Some(id) => heads.iter().find(|item| item.id == id).cloned().ok_or_else(|| SpiceError::User("The selected cloud branch is no longer a visible history head. Refresh and choose again.".to_string()))?,
            None if heads.len() == 1 => heads[0].clone(),
            None if heads.len() > 1 => return Err(SpiceError::User("Several cloud branches are visible. Choose one branch to review.".to_string())),
            None => return Err(SpiceError::User("No complete snapshot is visible. Push from the source computer first.".to_string())),
        };
        snapshot::inspect_manifest(config, &incoming)?;
        codex::validate_snapshot_source(&incoming)?;
        let operation_id = Uuid::new_v4().to_string();
        let stage = self.preview_directory(&operation_id)?;
        let current = self.capture_local(config, &stage, None)?;
        let state = settings::load_local_state(&self.data_dir)?;
        let baseline = snapshot::common_ancestor(
            config,
            state.last_applied_snapshot_id.as_deref(),
            &incoming.id,
        )?;
        let required_mappings = required_mappings(config, &incoming);
        let mut blocked_reasons = codex::transfer_issues(&incoming, &current.compatibility);
        if required_mappings.is_empty() {
            blocked_reasons.extend(destination_layout_issues(config, &incoming));
        }
        if incoming.device_id == config.device_id
            && state.last_applied_snapshot_id.as_deref() == Some(&incoming.id)
        {
            blocked_reasons.push("This snapshot was already applied on this device.".to_string());
        }
        let changes = diff_for_pull(config, &incoming, &current, baseline.as_ref())?;
        let local_fingerprint = pull_local_fingerprint(config, &current, &incoming)?;
        let mut warnings = incoming.warnings.clone();
        if let Some(base) = &baseline {
            for project in base.projects.iter().filter(|project| {
                project.mode == ProjectMode::Full
                    && selection_includes_project(&incoming.selection, &project.id)
                    && !incoming
                        .projects
                        .iter()
                        .any(|incoming| incoming.id == project.id)
            }) {
                if project.source_roots.iter().enumerate().any(|(index, _)| {
                    codex::destination_project_root(project, index, config).is_none()
                }) {
                    warnings.push(format!("{} was deleted on the source device. Its unmapped local files will be preserved. Set its local folder in What to sync and preview again to compare file deletions.", project.name));
                }
            }
        }
        let preview = OperationPreview {
            operation_id: operation_id.clone(),
            direction: Direction::Pull,
            snapshot_id: Some(incoming.id.clone()),
            estimated_bytes: changes
                .iter()
                .filter(|change| {
                    !matches!(
                        change.action,
                        ChangeAction::Unchanged | ChangeAction::Delete
                    )
                })
                .map(|change| change.bytes)
                .sum(),
            changes,
            warnings,
            blocked_reasons,
            requires_codex_close: platform::codex_running(),
            required_mappings,
        };
        self.store_prepared(PreparedOperation {
            preview: preview.clone(),
            config_fingerprint: config_fingerprint(config)?,
            local_fingerprint,
            transfer_contract: Some(transfer_contract(&incoming, &current)?),
            expected_latest: Some(incoming.id),
            expected_head_ids,
            additional_parent_ids: Vec::new(),
            stage_dir: stage,
            cancel: Arc::new(AtomicBool::new(false)),
            progress: new_progress(&operation_id, 7),
        })?;
        Ok(preview)
    }

    pub fn execute_push(&self, config: &AppConfig, operation_id: &str) -> Result<OperationResult> {
        self.validate_operation(config)?;
        let prepared = self.prepared(operation_id, Direction::Push)?;
        validate_prepared(config, &prepared)?;
        check_cancel(&prepared.cancel)?;
        set_progress(
            &prepared,
            OperationPhase::Rechecking,
            "Rechecking Codex and visible cloud history…",
            1,
        )?;
        platform::assert_codex_closed()?;
        let writer_monitor = platform::WriterMonitor::start(prepared.cancel.clone())?;
        let result = (|| -> Result<OperationResult> {
            self.ensure_heads(config, &prepared.expected_head_ids)?;
            let final_stage = self.preview_directory(&format!("{}-final", operation_id))?;
            let store = ObjectStore::with_reuse(
                final_stage.join("objects"),
                settings::cloud_store_root(config).join("objects"),
            )?;
            set_progress(
                &prepared,
                OperationPhase::Capturing,
                "Capturing a consistent Codex and workspace snapshot…",
                2,
            )?;
            let mut manifest = self.capture_local_using_store(
                config,
                &final_stage,
                prepared.expected_latest.clone(),
                Some(&prepared.cancel),
                &store,
                true,
            )?;
            manifest.additional_parent_ids = prepared.additional_parent_ids.clone();
            if snapshot::manifest_fingerprint(&manifest)? != prepared.local_fingerprint {
                return Err(SpiceError::User("Codex or a selected project changed after the preview. Review a fresh Push preview before publishing.".to_string()));
            }
            check_cancel(&prepared.cancel)?;
            set_progress(
                &prepared,
                OperationPhase::Verifying,
                "Checking hashes and available cloud-folder space…",
                3,
            )?;
            let cloud_store = ObjectStore::new(settings::cloud_store_root(config).join("objects"))?;
            let additional_cloud_bytes = manifest
                .objects
                .iter()
                .filter(|object| {
                    cloud_store
                        .object_path(&object.hash)
                        .map(|path| !path.is_file())
                        .unwrap_or(true)
                })
                .map(|object| object.stored_size)
                .sum::<u64>();
            ensure_disk_space(
                &settings::cloud_store_root(config),
                additional_cloud_bytes.saturating_add(MIN_OPERATION_HEADROOM),
                "publish this snapshot",
            )?;
            snapshot::validate_push_source_roots(config, &manifest.projects)?;
            set_progress(
                &prepared,
                OperationPhase::Publishing,
                "Saving content objects, then publishing the completion manifest…",
                4,
            )?;
            writer_monitor.check()?;
            platform::assert_codex_closed()?;
            let summary = snapshot::publish_with_progress(
                config,
                &store,
                &manifest,
                Some(&prepared.cancel),
                &mut |progress| {
                    set_progress(
                        &prepared,
                        OperationPhase::Publishing,
                        &content_progress_message(progress, "Checking and saving content"),
                        4,
                    )
                },
            )?;
            let mut state = settings::load_local_state(&self.data_dir)?;
            state.last_applied_snapshot_id = Some(manifest.id.clone());
            state.last_pushed_snapshot_id = Some(manifest.id.clone());
            state.last_selection_revision = Some(config.selection.revision.clone());
            state.pending_merge_parent_ids.clear();
            settings::save_local_state(&self.data_dir, &state)?;
            set_progress(
                &prepared,
                OperationPhase::Complete,
                "Snapshot saved to the sync folder.",
                5,
            )?;
            self.finish_operation(operation_id, &[prepared.stage_dir, final_stage])?;
            Ok(OperationResult {
                snapshot: summary,
                warnings: manifest.warnings,
                recovery_id: None,
                status_message: "Snapshot saved to the sync folder.".to_string(),
            })
        })();
        result.map_err(|error| writer_monitor.explain_error(error))
    }

    pub fn execute_pull(
        &self,
        config: &AppConfig,
        operation_id: &str,
        resolutions: &[ConflictResolution],
    ) -> Result<OperationResult> {
        self.validate_operation(config)?;
        let prepared = self.prepared(operation_id, Direction::Pull)?;
        validate_prepared(config, &prepared)?;
        check_cancel(&prepared.cancel)?;
        set_progress(
            &prepared,
            OperationPhase::Rechecking,
            "Rechecking Codex and visible cloud history…",
            1,
        )?;
        platform::assert_codex_closed()?;
        let writer_monitor = platform::WriterMonitor::start(prepared.cancel.clone())?;
        let result = (|| -> Result<OperationResult> {
            self.ensure_heads(config, &prepared.expected_head_ids)?;
            let incoming_id = prepared
                .expected_latest
                .as_deref()
                .ok_or_else(|| SpiceError::User("Pull preview has no snapshot.".to_string()))?;
            let incoming = snapshot::load_manifest(config, incoming_id)?;
            codex::validate_snapshot_source(&incoming)?;
            set_progress(
                &prepared,
                OperationPhase::Verifying,
                "Verifying every required cloud object…",
                2,
            )?;
            let summary = snapshot::verify_manifest_with_progress(
                config,
                &incoming,
                Some(&prepared.cancel),
                &mut |progress| {
                    set_progress(
                        &prepared,
                        OperationPhase::Verifying,
                        &content_progress_message(progress, "Checking received content"),
                        2,
                    )
                },
            )?;
            let recovery_estimate = incoming
                .objects
                .iter()
                .map(|object| object.raw_size)
                .sum::<u64>();
            ensure_disk_space(
                &self.data_dir,
                recovery_estimate.saturating_add(MIN_OPERATION_HEADROOM),
                "stage and protect this Pull",
            )?;
            let resolution_map: HashMap<_, _> = resolutions
                .iter()
                .map(|resolution| (resolution.key.as_str(), &resolution.choice))
                .collect();
            for conflict in prepared
                .preview
                .changes
                .iter()
                .filter(|change| change.action == ChangeAction::Conflict)
            {
                if !resolution_map.contains_key(conflict.key.as_str()) {
                    return Err(SpiceError::User(format!(
                        "Choose which version to keep for {}.",
                        conflict.label
                    )));
                }
            }
            if !required_mappings(config, &incoming).is_empty() {
                return Err(SpiceError::User("Choose a destination for every incoming project root, save it, and preview Pull again.".to_string()));
            }
            let recheck_stage = self.preview_directory(&format!("{}-recheck", operation_id))?;
            set_progress(
                &prepared,
                OperationPhase::Capturing,
                "Rechecking local sessions and destination files…",
                3,
            )?;
            let current = self.capture_local_cancellable(
                config,
                &recheck_stage,
                None,
                Some(&prepared.cancel),
                false,
                false,
            )?;
            if pull_local_fingerprint(config, &current, &incoming)? != prepared.local_fingerprint {
                return Err(SpiceError::User("Codex or a destination project changed after the preview. Review a fresh Pull preview before applying it.".to_string()));
            }
            if prepared.transfer_contract.as_ref() != Some(&transfer_contract(&incoming, &current)?)
            {
                return Err(SpiceError::User("The source snapshot or destination Codex version changed after preview. Refresh and review Pull again before restoring.".into()));
            }
            check_cancel(&prepared.cancel)?;
            let decisions = decisions(&prepared.preview, &resolution_map, &incoming);
            // Keeping local versions is a completed reconciliation too. Record the
            // reviewed ancestor without rewriting unchanged Codex databases or files.
            if decisions
                .values()
                .all(|decision| *decision == ApplyDecision::Local)
            {
                platform::assert_codex_closed()?;
                self.ensure_heads(config, &prepared.expected_head_ids)?;
                writer_monitor.check()?;
                record_pull_baseline(&self.data_dir, &incoming, &prepared.expected_head_ids)?;
                set_progress(
                    &prepared,
                    OperationPhase::Complete,
                    "Snapshot verified and acknowledged. Local versions kept; Push is available.",
                    7,
                )?;
                self.finish_operation(operation_id, &[prepared.stage_dir, recheck_stage])?;
                return Ok(OperationResult {
                snapshot: summary,
                warnings: incoming.warnings,
                recovery_id: None,
                status_message:
                    "Snapshot verified and acknowledged. Local versions kept; Push is available."
                        .to_string(),
            });
            }
            let apply_stage = self.preview_directory(&format!("{}-apply", operation_id))?;
            let staged_home = apply_stage.join("codex");
            let live_home = Path::new(&config.codex_home);
            codex::snapshot_databases(live_home, &staged_home)?;
            for name in [".codex-global-state.json", "session_index.jsonl"] {
                let source = live_home.join(name);
                if source.is_file() {
                    fs::copy(source, staged_home.join(name))?;
                }
            }
            let cloud_store = ObjectStore::new(settings::cloud_store_root(config).join("objects"))?;
            let incoming_threads: HashSet<String> = incoming
                .threads
                .iter()
                .filter(|thread| decision_incoming(&decisions, &format!("thread:{}", thread.id)))
                .map(|thread| thread.id.clone())
                .collect();
            let incoming_projects: HashSet<String> = incoming
                .projects
                .iter()
                .filter(|project| decision_incoming(&decisions, &format!("project:{}", project.id)))
                .map(|project| project.id.clone())
                .collect();
            let deleted_threads: HashSet<String> = decisions
                .iter()
                .filter(|(key, action)| {
                    key.starts_with("thread:") && **action == ApplyDecision::Delete
                })
                .map(|(key, _)| key.trim_start_matches("thread:").to_string())
                .collect();
            let deleted_projects: HashSet<String> = decisions
                .iter()
                .filter(|(key, action)| {
                    key.starts_with("project:") && **action == ApplyDecision::Delete
                })
                .map(|(key, _)| key.trim_start_matches("project:").to_string())
                .collect();
            let mut rollout_paths = HashMap::new();
            let mut rollout_fingerprints = HashMap::new();
            let mut staged_rollouts = Vec::new();
            for object in incoming.objects.iter().filter(|object| {
                matches!(object.kind, ObjectKind::Rollout)
                    && incoming_threads.contains(&object.owner_id)
            }) {
                let thread = incoming
                    .threads
                    .iter()
                    .find(|thread| thread.id == object.owner_id)
                    .expect("rollout owner in manifest");
                let relative = thread
                    .rollout_relative_path
                    .as_deref()
                    .map(PathBuf::from)
                    .and_then(|path| safe_relative(&path).ok())
                    .unwrap_or_else(|| {
                        PathBuf::from("sessions")
                            .join("spice-route")
                            .join(format!("{}.jsonl", thread.id))
                    });
                let staged = staged_home.join(&relative);
                cloud_store.materialize_cancellable(object, &staged, Some(&prepared.cancel))?;
                rollout_fingerprints.insert(
                    thread.id.clone(),
                    codex::portable_rollout_fingerprint(&staged)?,
                );
                let live = live_home.join(&relative);
                rollout_paths.insert(thread.id.clone(), path_string(&live));
                staged_rollouts.push((thread.id.clone(), staged, live));
            }
            let applied_id = settings::load_local_state(&self.data_dir)?.last_applied_snapshot_id;
            let baseline = snapshot::common_ancestor(config, applied_id.as_deref(), &incoming.id)?;
            let attachment_operations =
                plan_attachment_operations(config, &incoming, baseline.as_ref(), &decisions)?;
            let mut attachment_paths: HashMap<String, HashMap<String, String>> = HashMap::new();
            for object in incoming.objects.iter().filter(|object| {
                matches!(object.kind, ObjectKind::Artifact)
                    && incoming_threads.contains(&object.owner_id)
            }) {
                let thread = incoming
                    .threads
                    .iter()
                    .find(|thread| thread.id == object.owner_id)
                    .ok_or_else(|| {
                        SpiceError::CorruptSnapshot(format!(
                            "Attachment owner {} is missing.",
                            object.owner_id
                        ))
                    })?;
                let reference = thread
                    .attachments
                    .iter()
                    .find(|reference| reference.logical_path == object.logical_path)
                    .ok_or_else(|| {
                        SpiceError::CorruptSnapshot(format!(
                            "Attachment reference is missing for {}.",
                            object.logical_path
                        ))
                    })?;
                let destination =
                    attachment_destination(config, &incoming, object).ok_or_else(|| {
                        SpiceError::CorruptSnapshot(format!(
                            "Attachment path is invalid: {}.",
                            object.logical_path
                        ))
                    })?;
                attachment_paths
                    .entry(thread.id.clone())
                    .or_default()
                    .insert(reference.source_path.clone(), path_string(&destination));
            }
            for (thread_id, staged, _) in &staged_rollouts {
                if let Some(replacements) = attachment_paths.get(thread_id) {
                    codex::rewrite_rollout_local_image_paths(staged, replacements)?;
                }
            }
            codex::apply_bundle(
                &staged_home,
                &incoming,
                &incoming_threads,
                &incoming_projects,
                &deleted_threads,
                &deleted_projects,
                config,
                &rollout_paths,
                &attachment_paths,
            )?;
            let file_operations =
                plan_file_operations(config, &incoming, baseline.as_ref(), &decisions)?;
            let git_operations = plan_git_operations(config, &incoming, &decisions)?;
            let history_roots: Vec<PathBuf> = incoming
                .projects
                .iter()
                .filter(|project| {
                    project.mode == ProjectMode::HistoryOnly
                        && incoming_projects.contains(&project.id)
                })
                .flat_map(|project| {
                    (0..project.source_roots.len().max(1))
                        .filter_map(|index| codex::destination_project_root(project, index, config))
                })
                .collect();
            let mut recovery_targets = vec![
                self.data_dir.join("state.json"),
                live_home.join("state_5.sqlite"),
                live_home.join("thread_history_1.sqlite"),
                live_home.join(".codex-global-state.json"),
                live_home.join("session_index.jsonl"),
            ];
            for suffix in [
                "state_5.sqlite-wal",
                "state_5.sqlite-shm",
                "thread_history_1.sqlite-wal",
                "thread_history_1.sqlite-shm",
            ] {
                recovery_targets.push(live_home.join(suffix));
            }
            recovery_targets.extend(staged_rollouts.iter().map(|(_, _, live)| live.clone()));
            if let Some(base) = &baseline {
                for thread in base
                    .threads
                    .iter()
                    .filter(|thread| deleted_threads.contains(&thread.id))
                {
                    if let Some(relative) = &thread.rollout_relative_path {
                        if let Ok(relative) = safe_relative(Path::new(relative)) {
                            recovery_targets.push(live_home.join(relative));
                        }
                    }
                }
            }
            for operation in &file_operations {
                recovery_targets.push(operation.destination.clone());
            }
            for operation in &attachment_operations {
                recovery_targets.push(operation.destination.clone());
            }
            for operation in &git_operations {
                recovery_targets.extend(git_recovery_targets(&operation.destination));
            }
            recovery_targets.extend(history_roots.iter().cloned());
            platform::assert_codex_closed()?;
            set_progress(
                &prepared,
                OperationPhase::BackingUp,
                "Creating a durable local rollback set…",
                4,
            )?;
            let recovery_id = recovery::create_cancellable(
                &self.data_dir,
                &format!("Before pulling {}", short_id(&incoming.id)),
                Some(incoming.id.clone()),
                &recovery_targets,
                Some(&prepared.cancel),
            )?;
            set_progress(
                &prepared,
                OperationPhase::Applying,
                "Applying selected sessions, projects, and files…",
                5,
            )?;
            let apply_result = (|| -> Result<()> {
                check_cancel(&prepared.cancel)?;
                platform::assert_codex_closed()?;
                let mut live_mutations = 0_usize;
                for name in [
                    "state_5.sqlite",
                    "thread_history_1.sqlite",
                    ".codex-global-state.json",
                    "session_index.jsonl",
                ] {
                    writer_monitor.check()?;
                    check_codex_writer_periodically(&mut live_mutations)?;
                    let source = staged_home.join(name);
                    if source.is_file() {
                        replace_file(&source, &live_home.join(name))?;
                    }
                }
                for suffix in [
                    "state_5.sqlite-wal",
                    "state_5.sqlite-shm",
                    "thread_history_1.sqlite-wal",
                    "thread_history_1.sqlite-shm",
                ] {
                    writer_monitor.check()?;
                    check_codex_writer_periodically(&mut live_mutations)?;
                    let path = live_home.join(suffix);
                    if path.is_file() {
                        fs::remove_file(path)?;
                    }
                }
                for (_, source, live) in &staged_rollouts {
                    writer_monitor.check()?;
                    check_codex_writer_periodically(&mut live_mutations)?;
                    replace_file(source, live)?;
                }
                for root in &history_roots {
                    writer_monitor.check()?;
                    check_codex_writer_periodically(&mut live_mutations)?;
                    fs::create_dir_all(root)?;
                }
                platform::assert_codex_closed()?;
                restore_git_groups(
                    &cloud_store,
                    &incoming,
                    &git_operations,
                    &apply_stage,
                    Some(&prepared.cancel),
                    true,
                )?;
                for operation in &file_operations {
                    writer_monitor.check()?;
                    match &operation.object {
                        Some(object) => cloud_store.materialize_cancellable(
                            object,
                            &operation.destination,
                            Some(&prepared.cancel),
                        )?,
                        None => {
                            if operation.destination.is_file() {
                                fs::remove_file(&operation.destination)?;
                            }
                        }
                    }
                }
                for operation in &attachment_operations {
                    writer_monitor.check()?;
                    match &operation.object {
                        Some(object) => cloud_store.materialize_cancellable(
                            object,
                            &operation.destination,
                            Some(&prepared.cancel),
                        )?,
                        None => {
                            if operation.destination.is_file() {
                                fs::remove_file(&operation.destination)?;
                            }
                        }
                    }
                }
                for id in &deleted_threads {
                    writer_monitor.check()?;
                    check_codex_writer_periodically(&mut live_mutations)?;
                    if let Some(base) = baseline.as_ref() {
                        if let Some(thread) = base.threads.iter().find(|thread| &thread.id == id) {
                            if let Some(relative) = &thread.rollout_relative_path {
                                if let Ok(relative) = safe_relative(Path::new(relative)) {
                                    let path = live_home.join(relative);
                                    if path.is_file() {
                                        fs::remove_file(path)?;
                                    }
                                }
                            }
                        }
                    }
                }
                set_progress(
                    &prepared,
                    OperationPhase::FinalVerification,
                    "Verifying restored databases, files, transcripts, and Git state…",
                    6,
                )?;
                verify_applied_state(
                    config,
                    &incoming_threads,
                    &incoming_projects,
                    &deleted_threads,
                    &deleted_projects,
                    &file_operations,
                    &attachment_operations,
                    &git_operations,
                    &rollout_paths,
                    &rollout_fingerprints,
                    &history_roots,
                )?;
                writer_monitor.check()?;
                record_pull_baseline(&self.data_dir, &incoming, &prepared.expected_head_ids)?;
                Ok(())
            })();
            if let Err(error) = apply_result {
                let error = writer_monitor.explain_error(error);
                return Err(SpiceError::User(format!(
                    "Pull stopped after creating recovery point {recovery_id}: {error}"
                )));
            }
            recovery::complete(&self.data_dir, &recovery_id)?;
            set_progress(
                &prepared,
                OperationPhase::Complete,
                "Snapshot received, verified, and applied.",
                7,
            )?;
            self.finish_operation(
                operation_id,
                &[prepared.stage_dir, recheck_stage, apply_stage],
            )?;
            Ok(OperationResult {
                snapshot: summary,
                warnings: incoming.warnings,
                recovery_id: Some(recovery_id),
                status_message: "Snapshot received, verified, and applied.".to_string(),
            })
        })();
        result.map_err(|error| writer_monitor.explain_error(error))
    }

    pub fn cancel(&self, operation_id: &str) -> Result<()> {
        if let Some(operation) = self
            .operations
            .lock()
            .map_err(|_| SpiceError::User("Operation state lock was poisoned.".to_string()))?
            .get(operation_id)
        {
            operation.cancel.store(true, Ordering::SeqCst);
            set_progress(
                operation,
                OperationPhase::Cancelling,
                "Stopping at the next safe checkpoint…",
                0,
            )?;
        }
        Ok(())
    }

    /// Used when a native frontend closes its command pipe. Workers still finish
    /// their recovery bookkeeping before the sidecar exits.
    pub fn cancel_all(&self) {
        if let Ok(operations) = self.operations.lock() {
            for operation in operations.values() {
                operation.cancel.store(true, Ordering::SeqCst);
            }
        }
    }

    pub fn operation_progress(&self, operation_id: &str) -> Result<Option<OperationProgress>> {
        let operation = self
            .operations
            .lock()
            .map_err(|_| SpiceError::User("Operation state lock was poisoned.".to_string()))?
            .get(operation_id)
            .cloned();
        operation
            .map(|operation| {
                operation
                    .progress
                    .lock()
                    .map(|value| value.clone())
                    .map_err(|_| SpiceError::User("Progress state lock was poisoned.".to_string()))
            })
            .transpose()
    }

    pub fn preview_cloud_cleanup(&self, config: &AppConfig) -> Result<CloudCleanupPreview> {
        settings::validate_config(config)?;
        let root = validate_cleanup_root(config)?;
        let inventory = cleanup_inventory(&root)?;
        let preview = CloudCleanupPreview {
            operation_id: Uuid::new_v4().to_string(),
            snapshot_count: inventory.snapshot_count,
            object_count: inventory.object_count,
            stored_bytes: inventory.stored_bytes,
            confirmation_phrase: "RESET SPICE ROUTE".to_string(),
        };
        self.cleanups
            .lock()
            .map_err(|_| SpiceError::User("Cleanup state lock was poisoned.".to_string()))?
            .insert(
                preview.operation_id.clone(),
                PreparedCloudCleanup {
                    preview: preview.clone(),
                    config_fingerprint: config_fingerprint(config)?,
                    store_fingerprint: inventory.fingerprint,
                },
            );
        Ok(preview)
    }

    pub fn execute_cloud_cleanup(
        &self,
        config: &AppConfig,
        operation_id: &str,
        confirmation: &str,
    ) -> Result<CloudCleanupResult> {
        let prepared = self
            .cleanups
            .lock()
            .map_err(|_| SpiceError::User("Cleanup state lock was poisoned.".to_string()))?
            .get(operation_id)
            .cloned()
            .ok_or_else(|| {
                SpiceError::User(
                    "This cleanup preview expired. Review the current cloud contents again."
                        .to_string(),
                )
            })?;
        if confirmation != prepared.preview.confirmation_phrase {
            return Err(SpiceError::User(format!(
                "Type {} exactly to continue.",
                prepared.preview.confirmation_phrase
            )));
        }
        if config_fingerprint(config)? != prepared.config_fingerprint {
            return Err(SpiceError::User(
                "Settings changed after the cleanup preview. Review it again.".to_string(),
            ));
        }
        let root = validate_cleanup_root(config)?;
        let current = cleanup_inventory(&root)?;
        if current.fingerprint != prepared.store_fingerprint {
            return Err(SpiceError::User("The cloud folder changed after the cleanup preview. Review the current contents again.".to_string()));
        }
        let journal = serde_json::json!({
            "operationId": operation_id,
            "cloudStore": root.to_string_lossy(),
            "snapshotCount": current.snapshot_count,
            "objectCount": current.object_count,
            "storedBytes": current.stored_bytes,
        });
        let journal_path = self.data_dir.join("cloud-cleanup.json");
        write_json(&journal_path, &journal)?;
        for name in ["snapshots", "objects"] {
            let directory = root.join(name);
            if directory.exists() {
                fs::remove_dir_all(&directory)?;
            }
            fs::create_dir_all(&directory)?;
        }
        settings::save_local_state(&self.data_dir, &LocalState::default())?;
        if journal_path.is_file() {
            fs::remove_file(journal_path)?;
        }
        self.cleanups
            .lock()
            .map_err(|_| SpiceError::User("Cleanup state lock was poisoned.".to_string()))?
            .remove(operation_id);
        self.operations
            .lock()
            .map_err(|_| SpiceError::User("Operation state lock was poisoned.".to_string()))?
            .clear();
        Ok(CloudCleanupResult {
            snapshots_removed: current.snapshot_count,
            objects_removed: current.object_count,
            bytes_removed: current.stored_bytes,
        })
    }

    fn validate_operation(&self, config: &AppConfig) -> Result<()> {
        settings::validate_config(config)?;
        if self.data_dir.join("cloud-cleanup.json").is_file() {
            return Err(SpiceError::User("A cloud-history cleanup was interrupted. Open Settings and run Reset cloud history again.".to_string()));
        }
        if recovery::has_pending(&self.data_dir) {
            return Err(SpiceError::PendingRecovery);
        }
        let compatibility = codex::with_build_gate(
            codex::inspect(Path::new(&config.codex_home))?,
            platform::codex_version(platform::find_codex_executable().as_deref()).as_deref(),
        );
        if !compatibility.supported {
            return Err(SpiceError::UnsupportedCodex(compatibility.explanation));
        }
        Ok(())
    }

    fn capture_local(
        &self,
        config: &AppConfig,
        stage: &Path,
        parent: Option<String>,
    ) -> Result<SnapshotManifest> {
        self.capture_local_cancellable(config, stage, parent, None, false, false)
    }

    fn capture_local_cancellable(
        &self,
        config: &AppConfig,
        stage: &Path,
        parent: Option<String>,
        cancel: Option<&AtomicBool>,
        require_project_sources: bool,
        capture_content: bool,
    ) -> Result<SnapshotManifest> {
        let object_store = if capture_content {
            ObjectStore::new(stage.join("objects"))?
        } else {
            ObjectStore::for_preview(stage.join("objects"))
        };
        self.capture_local_using_store(
            config,
            stage,
            parent,
            cancel,
            &object_store,
            require_project_sources,
        )
    }

    fn capture_local_using_store(
        &self,
        config: &AppConfig,
        stage: &Path,
        parent: Option<String>,
        cancel: Option<&AtomicBool>,
        object_store: &ObjectStore,
        require_project_sources: bool,
    ) -> Result<SnapshotManifest> {
        let db_dir = stage.join("db");
        codex::snapshot_databases(Path::new(&config.codex_home), &db_dir)?;
        if let Some(flag) = cancel {
            check_cancel(flag)?;
        }
        let export = codex::export_selected(
            Path::new(&config.codex_home),
            &db_dir,
            &config.selection,
            Path::new(&config.projectless_root),
        )?;
        let mut manifest = snapshot::build_manifest_cancellable(
            config,
            export,
            object_store,
            parent,
            cancel,
            require_project_sources,
        )?;
        let version = platform::codex_version(platform::find_codex_executable().as_deref());
        manifest.compatibility = codex::with_build_gate(manifest.compatibility, version.as_deref());
        manifest.codex_version = version;
        if !manifest.compatibility.supported {
            return Err(SpiceError::UnsupportedCodex(
                manifest.compatibility.explanation.clone(),
            ));
        }
        Ok(manifest)
    }

    fn preview_directory(&self, operation_id: &str) -> Result<PathBuf> {
        if operation_id.contains(['/', '\\']) || operation_id.contains("..") {
            return Err(SpiceError::User("Invalid operation id.".to_string()));
        }
        let path = self.data_dir.join("previews").join(operation_id);
        if path.exists() {
            fs::remove_dir_all(&path)?;
        }
        fs::create_dir_all(&path)?;
        Ok(path)
    }

    fn store_prepared(&self, operation: PreparedOperation) -> Result<()> {
        self.operations
            .lock()
            .map_err(|_| SpiceError::User("Operation state lock was poisoned.".to_string()))?
            .insert(operation.preview.operation_id.clone(), operation);
        Ok(())
    }

    fn prepared(&self, id: &str, direction: Direction) -> Result<PreparedOperation> {
        let operation = self
            .operations
            .lock()
            .map_err(|_| SpiceError::User("Operation state lock was poisoned.".to_string()))?
            .get(id)
            .cloned()
            .ok_or_else(|| {
                SpiceError::User("This preview expired. Create a fresh preview.".to_string())
            })?;
        if std::mem::discriminant(&operation.preview.direction)
            != std::mem::discriminant(&direction)
        {
            return Err(SpiceError::User(
                "Preview direction does not match the requested operation.".to_string(),
            ));
        }
        Ok(operation)
    }

    fn ensure_heads(&self, config: &AppConfig, expected: &[String]) -> Result<()> {
        let current = sorted_ids(
            snapshot::head_manifests(config)?
                .into_iter()
                .map(|manifest| manifest.id),
        );
        if current != expected {
            return Err(SpiceError::User(
                "The visible cloud history changed after the preview. Refresh and review again."
                    .to_string(),
            ));
        }
        Ok(())
    }

    fn finish_operation(&self, id: &str, paths: &[PathBuf]) -> Result<()> {
        self.operations
            .lock()
            .map_err(|_| SpiceError::User("Operation state lock was poisoned.".to_string()))?
            .remove(id);
        let root = self.data_dir.join("previews");
        for path in paths {
            if path.parent() == Some(root.as_path()) && path.is_dir() {
                let _ = fs::remove_dir_all(path);
            }
        }
        Ok(())
    }
}

fn config_fingerprint(config: &AppConfig) -> Result<String> {
    validation_fingerprint(config)
}

fn validation_fingerprint(value: &impl serde::Serialize) -> Result<String> {
    // Config maps get a fresh randomized iteration order on every IPC decode.
    // Compare their meaning, including nested objects, rather than that order.
    // Keep this separate from persisted fingerprints used by older snapshots.
    let mut canonical = serde_json::to_value(value)?;
    canonical.sort_all_objects();
    hash_json(&canonical)
}

fn transfer_contract(
    incoming: &SnapshotManifest,
    local: &SnapshotManifest,
) -> Result<(String, String)> {
    Ok((
        validation_fingerprint(incoming)?,
        validation_fingerprint(&(&local.compatibility, &local.codex_version))?,
    ))
}
fn new_progress(operation_id: &str, total_steps: u32) -> Arc<Mutex<OperationProgress>> {
    Arc::new(Mutex::new(OperationProgress {
        operation_id: operation_id.to_string(),
        phase: OperationPhase::Ready,
        message: "Preview approved; ready to begin.".to_string(),
        completed_steps: 0,
        total_steps,
        cancellation_requested: false,
    }))
}
fn set_progress(
    operation: &PreparedOperation,
    phase: OperationPhase,
    message: &str,
    completed_steps: u32,
) -> Result<()> {
    let mut progress = operation
        .progress
        .lock()
        .map_err(|_| SpiceError::User("Progress state lock was poisoned.".to_string()))?;
    let cancelling = matches!(phase, OperationPhase::Cancelling);
    progress.phase = phase;
    progress.message = message.to_string();
    if !cancelling {
        progress.completed_steps = completed_steps.min(progress.total_steps);
    }
    progress.cancellation_requested = operation.cancel.load(Ordering::SeqCst);
    Ok(())
}
fn content_progress_message(progress: &snapshot::ContentProgress, action: &str) -> String {
    if progress.writing_manifest {
        return format!(
            "{} content objects checked. Publishing the completion manifest…",
            progress.completed_objects
        );
    }
    let mut message = format!(
        "{action}: {} of {} objects; {} of {} processed.",
        progress.completed_objects,
        progress.total_objects,
        progress_bytes(progress.processed_raw_bytes),
        progress_bytes(progress.total_raw_bytes),
    );
    if let Some(filename) = &progress.filename {
        message.push(' ');
        message.push_str(filename);
    }
    message
}

fn progress_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let units = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut index = 0;
    while value >= 1024.0 && index < units.len() - 1 {
        value /= 1024.0;
        index += 1;
    }
    format!("{value:.1} {}", units[index])
}

fn validate_prepared(config: &AppConfig, prepared: &PreparedOperation) -> Result<()> {
    if config_fingerprint(config)? != prepared.config_fingerprint {
        return Err(SpiceError::User(
            "Settings changed after this preview. Create a fresh preview.".to_string(),
        ));
    }
    if !prepared.preview.blocked_reasons.is_empty() {
        return Err(SpiceError::User(prepared.preview.blocked_reasons.join(" ")));
    }
    Ok(())
}
fn check_cancel(flag: &AtomicBool) -> Result<()> {
    if flag.load(Ordering::SeqCst) {
        Err(SpiceError::Cancelled)
    } else {
        Ok(())
    }
}
fn check_codex_writer_periodically(mutations: &mut usize) -> Result<()> {
    *mutations = mutations.saturating_add(1);
    // Even a short Pull replaces several live databases. Recheck before each
    // mutation so reopening Codex does not leave a batch of writes unguarded.
    platform::assert_codex_closed()
}
fn ensure_disk_space(path: &Path, required: u64, action: &str) -> Result<()> {
    let mut existing = path;
    while !existing.exists() {
        existing = existing.parent().ok_or_else(|| {
            SpiceError::User(format!(
                "Cannot determine free space for {}.",
                path.display()
            ))
        })?;
    }
    let available = fs2::available_space(existing)?;
    if available < required {
        return Err(SpiceError::User(format!("Not enough free space to {action}. Need about {} MB, but only {} MB is available on {}.", required / 1024 / 1024, available / 1024 / 1024, existing.display())));
    }
    Ok(())
}

struct CleanupInventory {
    snapshot_count: usize,
    object_count: usize,
    stored_bytes: u64,
    fingerprint: String,
}

fn validate_cleanup_root(config: &AppConfig) -> Result<PathBuf> {
    if !config.onboarding_complete {
        return Err(SpiceError::User(
            "Finish setup before resetting cloud history.".to_string(),
        ));
    }
    let cloud = Path::new(&config.cloud_root);
    let root = settings::cloud_store_root(config);
    if root.file_name().and_then(|value| value.to_str()) != Some(".spice-route")
        || root.parent() != Some(cloud)
    {
        return Err(SpiceError::User(
            "The cloud snapshot folder is not a safe Spice Route path.".to_string(),
        ));
    }
    if !root.is_dir() {
        return Err(SpiceError::User(
            "There is no Spice Route cloud history to reset.".to_string(),
        ));
    }
    if fs::symlink_metadata(&root)?.file_type().is_symlink() {
        return Err(SpiceError::User("The Spice Route data folder is a link. Choose the real cloud folder before cleaning it.".to_string()));
    }
    let canonical_cloud = dunce::canonicalize(cloud)?;
    let canonical_root = dunce::canonicalize(&root)?;
    if canonical_root.parent() != Some(canonical_cloud.as_path()) {
        return Err(SpiceError::User("The Spice Route data folder does not resolve directly inside the selected cloud folder.".to_string()));
    }
    let marker: serde_json::Value = read_json(&root.join("format.json"))?;
    if marker.get("format").and_then(serde_json::Value::as_str) != Some("spice-route")
        || marker
            .get("schemaVersion")
            .and_then(serde_json::Value::as_u64)
            != Some(1)
    {
        return Err(SpiceError::User(
            "The selected folder does not contain a recognized Spice Route marker.".to_string(),
        ));
    }
    Ok(root)
}

fn cleanup_inventory(root: &Path) -> Result<CleanupInventory> {
    let mut snapshot_count = 0;
    let mut object_count = 0;
    let mut stored_bytes = 0_u64;
    let mut entries = Vec::new();
    for name in ["snapshots", "objects"] {
        let directory = root.join(name);
        if !directory.exists() {
            continue;
        }
        if fs::symlink_metadata(&directory)?.file_type().is_symlink() {
            return Err(SpiceError::User(format!(
                "{} is a link and cannot be cleaned safely.",
                directory.display()
            )));
        }
        for entry in WalkDir::new(&directory).follow_links(false) {
            let entry = entry.map_err(|error| {
                SpiceError::User(format!("Could not inspect cloud history: {error}"))
            })?;
            if entry.file_type().is_symlink() {
                return Err(SpiceError::User(format!(
                    "Cloud history contains a link and cannot be cleaned safely: {}",
                    entry.path().display()
                )));
            }
            if !entry.file_type().is_file() {
                continue;
            }
            let size = entry
                .metadata()
                .map_err(|error| {
                    SpiceError::User(format!(
                        "Could not inspect {}: {error}",
                        entry.path().display()
                    ))
                })?
                .len();
            stored_bytes = stored_bytes.saturating_add(size);
            let relative = entry
                .path()
                .strip_prefix(root)
                .map_err(|_| {
                    SpiceError::User(
                        "A cloud-history path escaped the selected folder.".to_string(),
                    )
                })?
                .to_string_lossy()
                .replace('\\', "/");
            if name == "snapshots"
                && entry.path().extension().and_then(|value| value.to_str()) == Some("json")
            {
                snapshot_count += 1;
            }
            if name == "objects"
                && entry.path().extension().and_then(|value| value.to_str()) == Some("zst")
            {
                object_count += 1;
            }
            entries.push((relative, size));
        }
    }
    entries.sort();
    Ok(CleanupInventory {
        snapshot_count,
        object_count,
        stored_bytes,
        fingerprint: hash_json(&entries)?,
    })
}

fn short_id(value: &str) -> String {
    crate::models::handoff_label(value)
}

fn record_pull_baseline(
    data_dir: &Path,
    incoming: &SnapshotManifest,
    heads: &[String],
) -> Result<()> {
    let mut state = settings::load_local_state(data_dir)?;
    state.last_applied_snapshot_id = Some(incoming.id.clone());
    state.last_selection_revision = Some(incoming.selection_revision.clone());
    state.pending_merge_parent_ids = if heads.len() > 1 {
        heads.to_vec()
    } else {
        Vec::new()
    };
    settings::save_local_state(data_dir, &state)
}
fn sorted_ids(values: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut values: Vec<_> = values.into_iter().collect();
    values.sort();
    values.dedup();
    values
}
fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn change_order(a: &ChangePreview, b: &ChangePreview) -> std::cmp::Ordering {
    action_rank(&a.action)
        .cmp(&action_rank(&b.action))
        .then(a.label.cmp(&b.label))
}
fn action_rank(action: &ChangeAction) -> u8 {
    match action {
        ChangeAction::Conflict => 0,
        ChangeAction::Add => 1,
        ChangeAction::Update => 2,
        ChangeAction::Delete => 3,
        ChangeAction::Unchanged => 4,
    }
}

fn thread_map(manifest: &SnapshotManifest) -> HashMap<&str, &ThreadExport> {
    manifest
        .threads
        .iter()
        .map(|thread| (thread.id.as_str(), thread))
        .collect()
}
fn object_map(manifest: &SnapshotManifest) -> HashMap<&str, &ObjectEntry> {
    manifest
        .objects
        .iter()
        .map(|object| (object.logical_path.as_str(), object))
        .collect()
}
fn project_map(manifest: &SnapshotManifest) -> HashMap<&str, &ProjectExport> {
    manifest
        .projects
        .iter()
        .map(|project| (project.id.as_str(), project))
        .collect()
}

fn diff_for_push(base: Option<&SnapshotManifest>, local: &SnapshotManifest) -> Vec<ChangePreview> {
    let base_threads = base.map(thread_map).unwrap_or_default();
    let base_objects = base.map(object_map).unwrap_or_default();
    let base_projects = base.map(project_map).unwrap_or_default();
    let mut changes = Vec::new();
    for project in &local.projects {
        let current = project_fingerprint(project).unwrap_or_default();
        let action = match base_projects.get(project.id.as_str()) {
            None => ChangeAction::Add,
            Some(old) if project_fingerprint(old).unwrap_or_default() != current => {
                ChangeAction::Update
            }
            _ => ChangeAction::Unchanged,
        };
        changes.push(change(
            format!("project:{}", project.id),
            ChangeKind::Project,
            action,
            &project.name,
            match project.mode {
                ProjectMode::Full => "Project listing, chats, Git state, and portable files",
                ProjectMode::HistoryOnly => "Project listing and chat history",
                ProjectMode::Excluded => "Excluded",
            },
            0,
        ));
    }
    for thread in &local.threads {
        let action = match base_threads.get(thread.id.as_str()) {
            None => ChangeAction::Add,
            Some(old) if old.fingerprint != thread.fingerprint => ChangeAction::Update,
            _ => ChangeAction::Unchanged,
        };
        let bytes = local
            .objects
            .iter()
            .filter(|object| object.owner_id == thread.id)
            .map(|object| object.raw_size)
            .sum();
        changes.push(change(
            format!("thread:{}", thread.id),
            ChangeKind::Thread,
            action,
            &codex::display_thread_title(&thread.title),
            if thread.projectless {
                "Projectless chat"
            } else {
                "Project chat"
            },
            bytes,
        ));
    }
    for object in local.objects.iter().filter(|object| {
        matches!(
            object.kind,
            ObjectKind::ProjectFile | ObjectKind::ProjectlessFile
        )
    }) {
        let action = match base_objects.get(object.logical_path.as_str()) {
            None => ChangeAction::Add,
            Some(old) if old.hash != object.hash => ChangeAction::Update,
            _ => ChangeAction::Unchanged,
        };
        changes.push(change(
            format!("file:{}", object.logical_path),
            ChangeKind::ProjectFile,
            action,
            file_label(&object.logical_path),
            &object.logical_path,
            object.raw_size,
        ));
    }
    if let Some(base) = base {
        let excluded = codex::selection_excluded_thread_ids(&local.selection, &base.threads);
        for old in &base.threads {
            if !local.threads.iter().any(|thread| thread.id == old.id)
                && !excluded.contains(&old.id)
            {
                changes.push(change(
                    format!("thread:{}", old.id),
                    ChangeKind::Thread,
                    ChangeAction::Delete,
                    &codex::display_thread_title(&old.title),
                    "Deleted on this device",
                    0,
                ));
            }
        }
        for old in &base.projects {
            if !local.projects.iter().any(|project| project.id == old.id)
                && selection_includes_project(&local.selection, &old.id)
            {
                changes.push(change(
                    format!("project:{}", old.id),
                    ChangeKind::Project,
                    ChangeAction::Delete,
                    &old.name,
                    "Deleted on this device",
                    0,
                ));
            }
        }
        for old in base.objects.iter().filter(|object| {
            matches!(
                object.kind,
                ObjectKind::ProjectFile | ObjectKind::ProjectlessFile
            )
        }) {
            if !local
                .objects
                .iter()
                .any(|object| object.logical_path == old.logical_path)
                && selection_includes_object(local, old)
            {
                changes.push(change(
                    format!("file:{}", old.logical_path),
                    ChangeKind::ProjectFile,
                    ChangeAction::Delete,
                    file_label(&old.logical_path),
                    "Deleted on this device",
                    0,
                ));
            }
        }
    }
    changes
}

fn diff_for_pull(
    config: &AppConfig,
    incoming: &SnapshotManifest,
    local: &SnapshotManifest,
    baseline: Option<&SnapshotManifest>,
) -> Result<Vec<ChangePreview>> {
    let local_threads = thread_map(local);
    let incoming_threads = thread_map(incoming);
    let base_threads = baseline.map(thread_map).unwrap_or_default();
    let local_projects = project_map(local);
    let incoming_projects = project_map(incoming);
    let base_projects = baseline.map(project_map).unwrap_or_default();
    let base_objects = baseline.map(object_map).unwrap_or_default();
    // Older snapshots can contain internal approval tasks. Keep their historical
    // records in the immutable snapshot, but never offer or import them as chats.
    let excluded_threads =
        codex::selection_excluded_thread_ids(&incoming.selection, &incoming.threads);
    let mut changes = Vec::new();
    for project in &incoming.projects {
        let local_value = local_projects
            .get(project.id.as_str())
            .map(|value| project_fingerprint(value).unwrap_or_default());
        let incoming_value = project_fingerprint(project)?;
        let base_value = base_projects
            .get(project.id.as_str())
            .map(|value| project_fingerprint(value).unwrap_or_default());
        let action = three_way(
            local_value.as_deref(),
            Some(&incoming_value),
            base_value.as_deref(),
        );
        changes.push(with_conflict(change(
            format!("project:{}", project.id),
            ChangeKind::Project,
            action,
            &project.name,
            match project.mode {
                ProjectMode::Full => "Incoming full project",
                ProjectMode::HistoryOnly => "Incoming history-only project",
                ProjectMode::Excluded => "Excluded",
            },
            0,
        )));
    }
    for thread in &incoming.threads {
        if excluded_threads.contains(&thread.id) {
            continue;
        }
        let action = three_way(
            local_threads
                .get(thread.id.as_str())
                .map(|item| item.fingerprint.as_str()),
            Some(&thread.fingerprint),
            base_threads
                .get(thread.id.as_str())
                .map(|item| item.fingerprint.as_str()),
        );
        let bytes = incoming
            .objects
            .iter()
            .filter(|object| object.owner_id == thread.id)
            .map(|object| object.raw_size)
            .sum();
        changes.push(with_conflict(change(
            format!("thread:{}", thread.id),
            ChangeKind::Thread,
            action,
            &codex::display_thread_title(&thread.title),
            if thread.projectless {
                "Incoming projectless chat"
            } else {
                "Incoming project chat"
            },
            bytes,
        )));
    }
    for object in incoming.objects.iter().filter(|object| {
        matches!(
            object.kind,
            ObjectKind::ProjectFile | ObjectKind::ProjectlessFile
        ) && !(object.kind == ObjectKind::ProjectlessFile
            && excluded_threads.contains(&object.owner_id))
    }) {
        let Some(destination) = object_destination(config, incoming, object) else {
            continue;
        };
        let local_hash = if destination.is_file() {
            Some(sha256_file(&destination)?.0)
        } else {
            None
        };
        let base_hash = base_objects
            .get(object.logical_path.as_str())
            .map(|item| item.hash.as_str());
        let action = three_way(local_hash.as_deref(), Some(&object.hash), base_hash);
        changes.push(with_conflict(change(
            format!("file:{}", object.logical_path),
            ChangeKind::ProjectFile,
            action,
            file_label(&object.logical_path),
            &path_string(&destination),
            object.raw_size,
        )));
    }
    if let Some(base) = baseline {
        let excluded = codex::selection_excluded_thread_ids(&incoming.selection, &base.threads);
        for old in &base.threads {
            if !incoming_threads.contains_key(old.id.as_str()) && !excluded.contains(&old.id) {
                let local_value = local_threads
                    .get(old.id.as_str())
                    .map(|item| item.fingerprint.as_str());
                changes.push(with_conflict(change(
                    format!("thread:{}", old.id),
                    ChangeKind::Thread,
                    three_way(local_value, None, Some(&old.fingerprint)),
                    &codex::display_thread_title(&old.title),
                    "Deleted on the source device",
                    0,
                )));
            }
        }
        for old in &base.projects {
            if !incoming_projects.contains_key(old.id.as_str())
                && selection_includes_project(&incoming.selection, &old.id)
            {
                let base_value = project_fingerprint(old).unwrap_or_default();
                let local_value = local_projects
                    .get(old.id.as_str())
                    .map(|item| project_fingerprint(item).unwrap_or_default());
                changes.push(with_conflict(change(
                    format!("project:{}", old.id),
                    ChangeKind::Project,
                    three_way(local_value.as_deref(), None, Some(&base_value)),
                    &old.name,
                    "Deleted on the source device",
                    0,
                )));
            }
        }
        let incoming_objects = object_map(incoming);
        for old in base.objects.iter().filter(|object| {
            matches!(
                object.kind,
                ObjectKind::ProjectFile | ObjectKind::ProjectlessFile
            )
        }) {
            if !incoming_objects.contains_key(old.logical_path.as_str())
                && selection_includes_object(incoming, old)
                && !(old.kind == ObjectKind::ProjectlessFile && excluded.contains(&old.owner_id))
            {
                if let Some(destination) = object_destination(config, base, old) {
                    let local_hash = if destination.is_file() {
                        Some(sha256_file(&destination)?.0)
                    } else {
                        None
                    };
                    changes.push(with_conflict(change(
                        format!("file:{}", old.logical_path),
                        ChangeKind::ProjectFile,
                        three_way(local_hash.as_deref(), None, Some(&old.hash)),
                        file_label(&old.logical_path),
                        &path_string(&destination),
                        0,
                    )));
                }
            }
        }
    }
    for descriptor in incoming.projects.iter().flat_map(|project| &project.git) {
        let key = format!("git:{}:{}", descriptor.project_id, descriptor.root_index);
        let incoming_hash = git_descriptor_fingerprint(descriptor)?;
        let base_hash = baseline
            .and_then(|base| find_git(base, &descriptor.project_id, descriptor.root_index))
            .map(git_descriptor_fingerprint)
            .transpose()?
            .unwrap_or_default();
        let local_hash = project_root(
            config,
            incoming,
            &descriptor.project_id,
            descriptor.root_index,
        )
        .and_then(|root| git_fingerprint(&root));
        let action = three_way(
            local_hash.as_deref(),
            Some(&incoming_hash),
            (!base_hash.is_empty()).then_some(base_hash.as_str()),
        );
        changes.push(with_conflict(change(
            key,
            ChangeKind::Project,
            action,
            &format!(
                "{} Git state",
                project_name(incoming, &descriptor.project_id)
            ),
            descriptor
                .branch
                .as_deref()
                .or(descriptor.head.as_deref())
                .unwrap_or("Git repository"),
            0,
        )));
    }
    changes.sort_by(change_order);
    Ok(changes)
}

fn three_way(local: Option<&str>, incoming: Option<&str>, baseline: Option<&str>) -> ChangeAction {
    if local == incoming {
        return ChangeAction::Unchanged;
    }
    match (local, incoming, baseline) {
        (None, Some(_), _) => ChangeAction::Add,
        (Some(_), None, Some(base)) if local == Some(base) => ChangeAction::Delete,
        (Some(_), Some(_), Some(base)) if local == Some(base) => ChangeAction::Update,
        (Some(_), _, Some(base)) if incoming == Some(base) => ChangeAction::Unchanged,
        (None, None, _) => ChangeAction::Unchanged,
        _ => ChangeAction::Conflict,
    }
}

fn change(
    key: String,
    kind: ChangeKind,
    action: ChangeAction,
    label: &str,
    detail: &str,
    bytes: u64,
) -> ChangePreview {
    ChangePreview {
        key,
        kind,
        action,
        label: label.to_string(),
        detail: detail.to_string(),
        bytes,
        conflict: None,
    }
}
fn with_conflict(mut value: ChangePreview) -> ChangePreview {
    if value.action == ChangeAction::Conflict {
        value.conflict = Some(ConflictDetail {
            local_description: "Changed on this device".to_string(),
            incoming_description: "Changed on the source device".to_string(),
        });
    }
    value
}
fn file_label(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn project_fingerprint(project: &ProjectExport) -> Result<String> {
    let mut rows = project.rows.clone();
    if let Some(roots) = rows.get_mut("project_roots") {
        for row in roots {
            row.values.remove("path");
        }
    }
    let git = project
        .git
        .iter()
        .map(|descriptor| {
            (
                descriptor.root_index,
                descriptor.linked_worktree,
                descriptor.head.as_deref(),
                descriptor.branch.as_deref(),
                descriptor.index_object.as_deref(),
            )
        })
        .collect::<Vec<_>>();
    hash_json(&(
        project.id.as_str(),
        project.legacy_id.as_deref(),
        project.name.as_str(),
        project.mode,
        rows,
        git,
    ))
}

fn selection_includes_project(selection: &SelectionRules, project_id: &str) -> bool {
    selection
        .project_modes
        .get(project_id)
        .copied()
        .unwrap_or(selection.default_project_mode)
        != ProjectMode::Excluded
}
fn selection_includes_thread(selection: &SelectionRules, thread: &ThreadExport) -> bool {
    !codex::is_internal_thread(thread)
        && !selection.excluded_thread_ids.contains(&thread.id)
        && (selection.include_archived || !thread.archived)
        && thread
            .project_id
            .as_deref()
            .map(|id| selection_includes_project(selection, id))
            .unwrap_or(true)
}
fn selection_includes_object(manifest: &SnapshotManifest, object: &ObjectEntry) -> bool {
    match object.kind {
        ObjectKind::ProjectFile
        | ObjectKind::GitBundle
        | ObjectKind::GitIndex
        | ObjectKind::GitObjectPack => {
            selection_includes_project(&manifest.selection, &object.owner_id)
                && snapshot::selection_allows_object_path(object, &manifest.selection)
                && manifest
                    .projects
                    .iter()
                    .find(|project| project.id == object.owner_id)
                    .map(|project| project.mode == ProjectMode::Full)
                    .unwrap_or(false)
        }
        ObjectKind::ProjectlessFile | ObjectKind::Rollout | ObjectKind::Artifact => {
            manifest
                .threads
                .iter()
                .find(|thread| thread.id == object.owner_id)
                .map(|thread| selection_includes_thread(&manifest.selection, thread))
                .unwrap_or(false)
                && snapshot::selection_allows_object_path(object, &manifest.selection)
        }
    }
}

fn required_mappings(config: &AppConfig, incoming: &SnapshotManifest) -> Vec<RequiredMapping> {
    let base = settings::default_restore_location(config);
    let mut result = Vec::new();
    for project in incoming
        .projects
        .iter()
        .filter(|project| project.mode == ProjectMode::Full)
    {
        for (index, source) in project.source_roots.iter().enumerate() {
            let key = format!("{}:{}", project.id, index);
            if config.destination_roots.contains_key(&key)
                || (index == 0 && config.destination_roots.contains_key(&project.id))
            {
                continue;
            }
            let suffix = if project.source_roots.len() > 1 {
                format!("-{}", index + 1)
            } else {
                String::new()
            };
            result.push(RequiredMapping {
                project_id: project.id.clone(),
                root_index: index,
                project_name: project.name.clone(),
                source_path: source.clone(),
                suggested_path: path_string(&base.join(format!(
                    "{}{}",
                    safe_component(&project.name),
                    suffix
                ))),
            });
        }
    }
    result
}

fn safe_component(value: &str) -> String {
    let result: String = value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let result = result.trim_matches([' ', '.']);
    if result.is_empty() {
        "Project".to_string()
    } else {
        result.to_string()
    }
}
fn project_name<'a>(manifest: &'a SnapshotManifest, id: &str) -> &'a str {
    manifest
        .projects
        .iter()
        .find(|project| project.id == id)
        .map(|project| project.name.as_str())
        .unwrap_or("Project")
}
fn project_root(
    config: &AppConfig,
    manifest: &SnapshotManifest,
    project_id: &str,
    index: usize,
) -> Option<PathBuf> {
    manifest
        .projects
        .iter()
        .find(|project| project.id == project_id)
        .and_then(|project| codex::destination_project_root(project, index, config))
}

fn destination_layout_issues(config: &AppConfig, incoming: &SnapshotManifest) -> Vec<String> {
    let mut issues = Vec::new();
    let excluded = codex::selection_excluded_thread_ids(&incoming.selection, &incoming.threads);
    let mut roots: Vec<(String, PathBuf, String)> = Vec::new();
    for project in incoming
        .projects
        .iter()
        .filter(|project| project.mode == ProjectMode::Full)
    {
        for (index, source) in project.source_roots.iter().enumerate() {
            let Some(destination) = codex::destination_project_root(project, index, config) else {
                continue;
            };
            if let Err(error) = settings::validate_project_folder(&destination, config) {
                issues.push(format!("{}: {error}", project.name));
                continue;
            }
            if let Some(component) = unsupported_windows_component(&destination) {
                issues.push(format!(
                    "{} has a destination name Windows cannot restore: {component}.",
                    project.name
                ));
            }
            roots.push((
                source.to_lowercase(),
                settings::resolved_project_folder(&destination),
                project.name.clone(),
            ));
        }
    }
    for left in 0..roots.len() {
        for right in (left + 1)..roots.len() {
            let (left_source, left_destination, left_name) = &roots[left];
            let (right_source, right_destination, right_name) = &roots[right];
            if left_source == right_source
                && !settings::same_project_folder(left_destination, right_destination)
            {
                issues.push(format!(
                    "{} and {} share one source workspace and must use the same destination.",
                    left_name, right_name
                ));
            } else if left_source != right_source
                && paths_overlap(left_destination, right_destination)
            {
                issues.push(format!(
                    "The destinations for {} and {} overlap. Choose separate folders.",
                    left_name, right_name
                ));
            }
        }
    }
    let mut destinations: HashMap<String, (&str, &str)> = HashMap::new();
    for object in incoming.objects.iter().filter(|object| {
        matches!(
            object.kind,
            ObjectKind::ProjectFile | ObjectKind::ProjectlessFile
        ) && !(object.kind == ObjectKind::ProjectlessFile && excluded.contains(&object.owner_id))
    }) {
        if let Some(destination) = object_destination(config, incoming, object) {
            if let Some(component) = unsupported_windows_component(&destination) {
                issues.push(format!(
                    "{} contains a filename Windows cannot restore: {}.",
                    object.logical_path, component
                ));
            }
            let folded = destination.to_string_lossy().to_lowercase();
            if let Some((existing_path, existing_hash)) =
                destinations.insert(folded, (object.logical_path.as_str(), object.hash.as_str()))
            {
                if existing_path != object.logical_path && existing_hash != object.hash {
                    issues.push(format!(
                        "{} and {} collide on this filesystem.",
                        existing_path, object.logical_path
                    ));
                }
            }
        }
    }
    issues.sort();
    issues.dedup();
    issues
}

#[cfg(windows)]
fn unsupported_windows_component(path: &Path) -> Option<String> {
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .find_map(|value| {
            let stem = value
                .split('.')
                .next()
                .unwrap_or(value)
                .to_ascii_uppercase();
            let invalid = value.ends_with([' ', '.'])
                || value.chars().any(|character| {
                    character < ' ' || matches!(character, '<' | '>' | ':' | '"' | '|' | '?' | '*')
                })
                || RESERVED.contains(&stem.as_str());
            invalid.then(|| value.to_string())
        })
}

#[cfg(not(windows))]
fn unsupported_windows_component(_path: &Path) -> Option<String> {
    None
}

fn object_destination(
    config: &AppConfig,
    manifest: &SnapshotManifest,
    object: &ObjectEntry,
) -> Option<PathBuf> {
    let segments: Vec<_> = object.logical_path.split('/').collect();
    match object.kind {
        ObjectKind::ProjectFile => {
            if segments.len() < 5 {
                return None;
            }
            let project_id = segments[1];
            let index = segments[2].parse().ok()?;
            let relative = safe_relative(&segments[4..].iter().collect::<PathBuf>()).ok()?;
            project_root(config, manifest, project_id, index).map(|root| root.join(relative))
        }
        ObjectKind::ProjectlessFile => {
            if segments.len() < 4 {
                return None;
            }
            let thread = manifest
                .threads
                .iter()
                .find(|thread| thread.id == object.owner_id)?;
            let base = thread
                .projectless_relative_root
                .as_deref()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(safe_component(&thread.title)));
            let relative = safe_relative(&segments[3..].iter().collect::<PathBuf>()).ok()?;
            Some(
                Path::new(&config.projectless_root)
                    .join(base)
                    .join(relative),
            )
        }
        _ => None,
    }
}

fn attachment_destination(
    config: &AppConfig,
    manifest: &SnapshotManifest,
    object: &ObjectEntry,
) -> Option<PathBuf> {
    if !matches!(object.kind, ObjectKind::Artifact) {
        return None;
    }
    let segments: Vec<_> = object.logical_path.split('/').collect();
    if segments.len() < 4
        || segments[0] != "codex"
        || segments[1] != "attachments"
        || segments[2] != object.owner_id
    {
        return None;
    }
    let thread = manifest
        .threads
        .iter()
        .find(|thread| thread.id == object.owner_id)?;
    if !thread
        .attachments
        .iter()
        .any(|reference| reference.logical_path == object.logical_path)
    {
        return None;
    }
    let relative = safe_relative(&segments[2..].iter().collect::<PathBuf>()).ok()?;
    Some(
        Path::new(&config.codex_home)
            .join("imported-attachments")
            .join(relative),
    )
}

fn pull_local_fingerprint(
    config: &AppConfig,
    local: &SnapshotManifest,
    incoming: &SnapshotManifest,
) -> Result<String> {
    let mut files = Vec::new();
    let excluded = codex::selection_excluded_thread_ids(&incoming.selection, &incoming.threads);
    for object in incoming.objects.iter().filter(|object| {
        matches!(
            object.kind,
            ObjectKind::ProjectFile | ObjectKind::ProjectlessFile
        ) && !(object.kind == ObjectKind::ProjectlessFile && excluded.contains(&object.owner_id))
    }) {
        if let Some(destination) = object_destination(config, incoming, object) {
            files.push((
                object.logical_path.clone(),
                if destination.is_file() {
                    Some(sha256_file(&destination)?.0)
                } else {
                    None
                },
            ));
        }
    }
    files.sort();
    hash_json(&(snapshot::manifest_fingerprint(local)?, files))
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ApplyDecision {
    Incoming,
    Local,
    Delete,
}
fn decisions<'a>(
    preview: &OperationPreview,
    resolutions: &HashMap<&'a str, &'a ConflictChoice>,
    incoming: &SnapshotManifest,
) -> HashMap<String, ApplyDecision> {
    preview
        .changes
        .iter()
        .map(|change| {
            let incoming_has_key = if let Some(id) = change.key.strip_prefix("thread:") {
                incoming.threads.iter().any(|item| item.id == id)
            } else if let Some(id) = change.key.strip_prefix("project:") {
                incoming.projects.iter().any(|item| item.id == id)
            } else if let Some(path) = change.key.strip_prefix("file:") {
                incoming
                    .objects
                    .iter()
                    .any(|item| item.logical_path == path)
            } else {
                true
            };
            let decision = match change.action {
                ChangeAction::Add | ChangeAction::Update => ApplyDecision::Incoming,
                ChangeAction::Delete => ApplyDecision::Delete,
                ChangeAction::Conflict => match resolutions.get(change.key.as_str()) {
                    Some(ConflictChoice::Incoming) => {
                        if incoming_has_key {
                            ApplyDecision::Incoming
                        } else {
                            ApplyDecision::Delete
                        }
                    }
                    _ => ApplyDecision::Local,
                },
                ChangeAction::Unchanged => ApplyDecision::Local,
            };
            (change.key.clone(), decision)
        })
        .collect()
}
fn decision_incoming(decisions: &HashMap<String, ApplyDecision>, key: &str) -> bool {
    matches!(decisions.get(key), Some(ApplyDecision::Incoming))
}

struct FileOperation<'a> {
    destination: PathBuf,
    object: Option<&'a ObjectEntry>,
}
fn plan_file_operations<'a>(
    config: &AppConfig,
    incoming: &'a SnapshotManifest,
    baseline: Option<&'a SnapshotManifest>,
    decisions: &HashMap<String, ApplyDecision>,
) -> Result<Vec<FileOperation<'a>>> {
    let mut result = Vec::new();
    for object in incoming.objects.iter().filter(|object| {
        matches!(
            object.kind,
            ObjectKind::ProjectFile | ObjectKind::ProjectlessFile
        )
    }) {
        if decision_incoming(decisions, &format!("file:{}", object.logical_path)) {
            if let Some(destination) = object_destination(config, incoming, object) {
                result.push(FileOperation {
                    destination,
                    object: Some(object),
                });
            }
        }
    }
    if let Some(baseline) = baseline {
        for object in baseline.objects.iter().filter(|object| {
            matches!(
                object.kind,
                ObjectKind::ProjectFile | ObjectKind::ProjectlessFile
            )
        }) {
            if matches!(
                decisions.get(&format!("file:{}", object.logical_path)),
                Some(ApplyDecision::Delete)
            ) {
                if let Some(destination) = object_destination(config, baseline, object) {
                    result.push(FileOperation {
                        destination,
                        object: None,
                    });
                }
            }
        }
    }
    Ok(result)
}

fn plan_attachment_operations<'a>(
    config: &AppConfig,
    incoming: &'a SnapshotManifest,
    baseline: Option<&'a SnapshotManifest>,
    decisions: &HashMap<String, ApplyDecision>,
) -> Result<Vec<FileOperation<'a>>> {
    let mut result = Vec::new();
    for object in incoming
        .objects
        .iter()
        .filter(|object| matches!(object.kind, ObjectKind::Artifact))
    {
        if decision_incoming(decisions, &format!("thread:{}", object.owner_id)) {
            let destination =
                attachment_destination(config, incoming, object).ok_or_else(|| {
                    SpiceError::CorruptSnapshot(format!(
                        "Attachment path is invalid: {}",
                        object.logical_path
                    ))
                })?;
            result.push(FileOperation {
                destination,
                object: Some(object),
            });
        }
    }
    if let Some(baseline) = baseline {
        let incoming_objects = object_map(incoming);
        for object in baseline
            .objects
            .iter()
            .filter(|object| matches!(object.kind, ObjectKind::Artifact))
        {
            let replace_thread = matches!(
                decisions.get(&format!("thread:{}", object.owner_id)),
                Some(ApplyDecision::Incoming | ApplyDecision::Delete)
            );
            if replace_thread && !incoming_objects.contains_key(object.logical_path.as_str()) {
                let destination =
                    attachment_destination(config, baseline, object).ok_or_else(|| {
                        SpiceError::CorruptSnapshot(format!(
                            "Baseline attachment path is invalid: {}",
                            object.logical_path
                        ))
                    })?;
                result.push(FileOperation {
                    destination,
                    object: None,
                });
            }
        }
    }
    Ok(result)
}

struct GitOperation<'a> {
    destination: PathBuf,
    descriptor: &'a GitDescriptor,
}
fn plan_git_operations<'a>(
    config: &AppConfig,
    incoming: &'a SnapshotManifest,
    decisions: &HashMap<String, ApplyDecision>,
) -> Result<Vec<GitOperation<'a>>> {
    let mut result = Vec::new();
    for descriptor in incoming.projects.iter().flat_map(|project| &project.git) {
        if decision_incoming(
            decisions,
            &format!("git:{}:{}", descriptor.project_id, descriptor.root_index),
        ) {
            let destination = project_root(
                config,
                incoming,
                &descriptor.project_id,
                descriptor.root_index,
            )
            .ok_or_else(|| {
                SpiceError::User(format!(
                    "No destination is mapped for {}.",
                    project_name(incoming, &descriptor.project_id)
                ))
            })?;
            result.push(GitOperation {
                destination,
                descriptor,
            });
        }
    }
    Ok(result)
}

fn find_git<'a>(
    manifest: &'a SnapshotManifest,
    project_id: &str,
    root_index: usize,
) -> Option<&'a GitDescriptor> {
    manifest
        .projects
        .iter()
        .find(|project| project.id == project_id)
        .and_then(|project| project.git.iter().find(|git| git.root_index == root_index))
}
fn git_descriptor_fingerprint(descriptor: &GitDescriptor) -> Result<String> {
    hash_json(&(
        descriptor.head.as_deref(),
        descriptor.branch.as_deref(),
        descriptor.index_object.as_deref(),
    ))
}
fn git_fingerprint(root: &Path) -> Option<String> {
    if !root.join(".git").exists() {
        return None;
    }
    let head = git_output(root, &["rev-parse", "HEAD"]);
    let branch = git_output(root, &["symbolic-ref", "--quiet", "--short", "HEAD"]);
    let index = git_output(root, &["rev-parse", "--git-path", "index"]).and_then(|path| {
        sha256_file(&absolute_from(root, &path))
            .ok()
            .map(|item| item.0)
    });
    hash_json(&(head, branch, index)).ok()
}

#[allow(clippy::too_many_arguments)]
fn verify_applied_state(
    config: &AppConfig,
    incoming_threads: &HashSet<String>,
    incoming_projects: &HashSet<String>,
    deleted_threads: &HashSet<String>,
    deleted_projects: &HashSet<String>,
    file_operations: &[FileOperation<'_>],
    attachment_operations: &[FileOperation<'_>],
    git_operations: &[GitOperation<'_>],
    rollout_paths: &HashMap<String, String>,
    rollout_fingerprints: &HashMap<String, String>,
    history_roots: &[PathBuf],
) -> Result<()> {
    platform::assert_codex_closed()?;
    codex::verify_databases(Path::new(&config.codex_home))?;
    let catalog = codex::list_content(Path::new(&config.codex_home))?;
    let thread_ids: HashSet<_> = catalog
        .threads
        .iter()
        .map(|thread| thread.id.as_str())
        .collect();
    let project_ids: HashSet<_> = catalog
        .projects
        .iter()
        .map(|project| project.id.as_str())
        .collect();
    for id in incoming_threads {
        if !thread_ids.contains(id.as_str()) {
            return Err(SpiceError::User(format!(
                "Post-restore verification could not find chat {id}."
            )));
        }
    }
    for id in deleted_threads {
        if thread_ids.contains(id.as_str()) {
            return Err(SpiceError::User(format!(
                "Post-restore verification found chat {id}, which should have been deleted."
            )));
        }
    }
    for id in incoming_projects {
        if !project_ids.contains(id.as_str()) {
            return Err(SpiceError::User(format!(
                "Post-restore verification could not find project {id}."
            )));
        }
    }
    for id in deleted_projects {
        if project_ids.contains(id.as_str()) {
            return Err(SpiceError::User(format!(
                "Post-restore verification found project {id}, which should have been deleted."
            )));
        }
    }
    for operation in file_operations.iter().chain(attachment_operations) {
        match operation.object {
            Some(object) => {
                if !operation.destination.is_file()
                    || sha256_file(&operation.destination)?.0 != object.hash
                {
                    return Err(SpiceError::User(format!(
                        "Post-restore verification failed for {}.",
                        operation.destination.display()
                    )));
                }
            }
            None if operation.destination.exists() => {
                return Err(SpiceError::User(format!(
                    "Post-restore verification found a file that should have been deleted: {}",
                    operation.destination.display()
                )));
            }
            None => {}
        }
    }
    for operation in git_operations {
        let local = git_fingerprint(&operation.destination).ok_or_else(|| {
            SpiceError::User(format!(
                "Post-restore verification could not read Git state in {}.",
                operation.destination.display()
            ))
        })?;
        if local != git_descriptor_fingerprint(operation.descriptor)? {
            return Err(SpiceError::User(format!(
                "Post-restore Git state does not match for {}.",
                operation.destination.display()
            )));
        }
        command_status(
            {
                let mut command = platform::hidden_command("git");
                command.arg("-C").arg(&operation.destination).args([
                    "fsck",
                    "--connectivity-only",
                    "--no-reflogs",
                ]);
                command
            },
            "verify restored Git objects",
        )?;
    }
    for (thread_id, path) in rollout_paths {
        let expected = rollout_fingerprints.get(thread_id).ok_or_else(|| {
            SpiceError::User(format!(
                "Snapshot transcript fingerprint is missing for chat {thread_id}."
            ))
        })?;
        if !Path::new(path).is_file()
            || codex::portable_rollout_fingerprint(Path::new(path))? != *expected
        {
            return Err(SpiceError::User(format!(
                "Post-restore transcript verification failed for chat {thread_id}."
            )));
        }
    }
    for root in history_roots {
        if !root.is_dir() {
            return Err(SpiceError::User(format!(
                "History-only project mapping was not created: {}",
                root.display()
            )));
        }
    }
    platform::assert_codex_closed()
}

fn git_output(root: &Path, args: &[&str]) -> Option<String> {
    let output = platform::hidden_command("git")
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
fn absolute_from(root: &Path, value: &str) -> PathBuf {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

fn git_recovery_targets(root: &Path) -> Vec<PathBuf> {
    let dot_git = root.join(".git");
    if !dot_git.exists() {
        return vec![root.to_path_buf()];
    }
    let mut targets = vec![dot_git];
    for argument in ["--git-common-dir", "--git-dir"] {
        if let Some(value) = git_output(root, &["rev-parse", argument]) {
            let path = absolute_from(root, &value);
            if !targets.iter().any(|existing| {
                existing
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&path.to_string_lossy())
            }) {
                targets.push(path);
            }
        }
    }
    targets
}

fn restore_git_groups(
    store: &ObjectStore,
    manifest: &SnapshotManifest,
    operations: &[GitOperation<'_>],
    stage: &Path,
    cancel: Option<&AtomicBool>,
    require_codex_closed: bool,
) -> Result<()> {
    let mut primaries: HashMap<&str, PathBuf> = HashMap::new();
    let mut ordered: Vec<_> = operations.iter().collect();
    ordered.sort_by_key(|operation| {
        (
            operation.descriptor.common_id.as_str(),
            operation.descriptor.linked_worktree,
        )
    });
    for operation in ordered {
        if let Some(flag) = cancel {
            check_cancel(flag)?;
        }
        if require_codex_closed {
            platform::assert_codex_closed()?;
        }
        let descriptor = operation.descriptor;
        let bundle_entry = descriptor.bundle_object.as_ref().and_then(|hash| {
            manifest
                .objects
                .iter()
                .find(|object| object.hash == *hash && matches!(object.kind, ObjectKind::GitBundle))
        });
        let bundle_path = if let Some(object) = bundle_entry {
            let path = stage.join("git").join(format!("{}.bundle", object.hash));
            store.materialize_cancellable(object, &path, cancel)?;
            Some(path)
        } else {
            None
        };
        let primary = primaries.get(descriptor.common_id.as_str()).cloned();
        if operation.destination.join(".git").exists() {
            if let Some(bundle) = &bundle_path {
                fetch_bundle(&operation.destination, bundle)?;
            }
        } else if let Some(primary) = primary.filter(|_| descriptor.linked_worktree) {
            if let Some(parent) = operation.destination.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut command = platform::hidden_command("git");
            command
                .arg("-C")
                .arg(primary)
                .args(["worktree", "add", "--force"]);
            if descriptor.branch.is_none() {
                command.arg("--detach");
            }
            command.arg(&operation.destination).arg(
                descriptor
                    .branch
                    .as_deref()
                    .or(descriptor.head.as_deref())
                    .unwrap_or("HEAD"),
            );
            command_status(command, "reconstruct linked Git worktree")?;
        } else {
            fs::create_dir_all(&operation.destination)?;
            command_status(
                {
                    let mut c = platform::hidden_command("git");
                    c.arg("-C").arg(&operation.destination).arg("init");
                    c
                },
                "initialize Git repository",
            )?;
            if let Some(bundle) = &bundle_path {
                fetch_bundle(&operation.destination, bundle)?;
            }
            primaries.insert(descriptor.common_id.as_str(), operation.destination.clone());
        }
        set_git_head(&operation.destination, descriptor)?;
        if let Some(pack_hash) = &descriptor.index_pack_object {
            let object = manifest
                .objects
                .iter()
                .find(|object| {
                    object.hash == *pack_hash && matches!(object.kind, ObjectKind::GitObjectPack)
                })
                .ok_or_else(|| {
                    SpiceError::CorruptSnapshot(format!(
                        "Git index-object pack is missing for project {}.",
                        descriptor.project_id
                    ))
                })?;
            let pack_path = stage
                .join("git")
                .join(format!("{}.index-objects.pack", object.hash));
            store.materialize_cancellable(object, &pack_path, cancel)?;
            install_git_object_pack(&operation.destination, &pack_path)?;
        }
        if let Some(index_hash) = &descriptor.index_object {
            if let Some(object) = manifest.objects.iter().find(|object| {
                object.hash == *index_hash && matches!(object.kind, ObjectKind::GitIndex)
            }) {
                let index_path = git_output(
                    &operation.destination,
                    &["rev-parse", "--git-path", "index"],
                )
                .map(|path| absolute_from(&operation.destination, &path))
                .ok_or_else(|| {
                    SpiceError::User(format!(
                        "Could not locate Git index for {}",
                        operation.destination.display()
                    ))
                })?;
                store.materialize_cancellable(object, &index_path, cancel)?;
            }
        }
    }
    Ok(())
}

fn install_git_object_pack(root: &Path, pack: &Path) -> Result<()> {
    let input = File::open(pack)?;
    command_status(
        {
            let mut command = platform::hidden_command("git");
            command
                .arg("-C")
                .arg(root)
                .args(["index-pack", "--stdin"])
                .stdin(Stdio::from(input));
            command
        },
        "restore staged Git objects",
    )
}

fn fetch_bundle(root: &Path, bundle: &Path) -> Result<()> {
    command_status(
        {
            let mut c = platform::hidden_command("git");
            c.arg("-C")
                .arg(root)
                .args(["fetch", "--force"])
                .arg(bundle)
                .args(["+refs/heads/*:refs/heads/*", "+refs/tags/*:refs/tags/*"]);
            c
        },
        "restore Git history",
    )
}
fn set_git_head(root: &Path, descriptor: &GitDescriptor) -> Result<()> {
    if let (Some(branch), Some(head)) = (&descriptor.branch, &descriptor.head) {
        command_status(
            {
                let mut c = platform::hidden_command("git");
                c.arg("-C")
                    .arg(root)
                    .args(["update-ref", &format!("refs/heads/{branch}"), head]);
                c
            },
            "restore Git branch",
        )?;
        command_status(
            {
                let mut c = platform::hidden_command("git");
                c.arg("-C").arg(root).args([
                    "symbolic-ref",
                    "HEAD",
                    &format!("refs/heads/{branch}"),
                ]);
                c
            },
            "select Git branch",
        )?;
    } else if let Some(head) = &descriptor.head {
        command_status(
            {
                let mut c = platform::hidden_command("git");
                c.arg("-C")
                    .arg(root)
                    .args(["update-ref", "--no-deref", "HEAD", head]);
                c
            },
            "restore detached Git HEAD",
        )?;
    }
    Ok(())
}
fn command_status(mut command: Command, action: &str) -> Result<()> {
    let output = command.output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(SpiceError::User(format!(
            "Could not {action}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use tempfile::tempdir;

    fn review_config() -> AppConfig {
        let mut config = settings::default_config();
        for index in 0..16 {
            let id = format!("project-{index}");
            config
                .source_roots
                .insert(id.clone(), format!("source/{index}"));
            config
                .destination_roots
                .insert(id.clone(), format!("destination/{index}"));
            config
                .selection
                .project_modes
                .insert(id, ProjectMode::HistoryOnly);
        }
        config
    }

    fn prepared_for_config(config: &AppConfig) -> PreparedOperation {
        PreparedOperation {
            preview: OperationPreview {
                operation_id: "review".into(),
                direction: Direction::Pull,
                snapshot_id: Some("snapshot".into()),
                changes: Vec::new(),
                warnings: Vec::new(),
                blocked_reasons: Vec::new(),
                estimated_bytes: 0,
                requires_codex_close: false,
                required_mappings: Vec::new(),
            },
            config_fingerprint: config_fingerprint(config).unwrap(),
            local_fingerprint: String::new(),
            transfer_contract: None,
            expected_latest: None,
            expected_head_ids: Vec::new(),
            additional_parent_ids: Vec::new(),
            stage_dir: PathBuf::new(),
            cancel: Arc::new(AtomicBool::new(false)),
            progress: new_progress("review", 1),
        }
    }

    #[test]
    fn unchanged_settings_survive_ipc_round_trips_and_different_map_insertion_orders() {
        let config = review_config();
        let prepared = prepared_for_config(&config);
        let serialized = serde_json::to_string(&config).unwrap();
        for _ in 0..64 {
            let mut decoded: AppConfig = serde_json::from_str(&serialized).unwrap();
            // Rebuild each map in the opposite order, as another client can do.
            decoded.source_roots = config
                .source_roots
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            decoded.destination_roots = (0..16)
                .rev()
                .map(|index| (format!("project-{index}"), format!("destination/{index}")))
                .collect();
            decoded.selection.project_modes = (0..16)
                .rev()
                .map(|index| (format!("project-{index}"), ProjectMode::HistoryOnly))
                .collect();
            validate_prepared(&decoded, &prepared).unwrap();
        }
    }

    #[test]
    fn real_settings_changes_still_invalidate_the_preview() {
        let config = review_config();
        let prepared = prepared_for_config(&config);
        let mut variants = Vec::new();
        let mut changed = config.clone();
        changed
            .destination_roots
            .insert("project-0".into(), "different/destination".into());
        variants.push(changed);
        let mut changed = config.clone();
        changed
            .source_roots
            .insert("project-0".into(), "different/source".into());
        variants.push(changed);
        let mut changed = config.clone();
        changed
            .selection
            .project_modes
            .insert("project-0".into(), ProjectMode::Full);
        variants.push(changed);
        let mut changed = config.clone();
        changed.selection.excluded_thread_ids.push("chat".into());
        variants.push(changed);
        let mut changed = config.clone();
        changed.selection.include_sensitive_files = !changed.selection.include_sensitive_files;
        variants.push(changed);
        let mut changed = config.clone();
        changed.cloud_root.push_str("/different-cloud");
        variants.push(changed);
        for changed in variants {
            assert!(validate_prepared(&changed, &prepared)
                .unwrap_err()
                .to_string()
                .contains("Settings changed"));
        }
    }

    #[test]
    fn transfer_validation_survives_manifest_reload_but_detects_content_and_schema_changes() {
        let mut incoming = review_manifest();
        incoming.selection = review_config().selection;
        incoming.ui_state =
            serde_json::from_str(r#"{"projects":{"beta":2,"alpha":1},"views":[{"z":0,"a":1}]}"#)
                .unwrap();
        let local = review_manifest();
        let expected = transfer_contract(&incoming, &local).unwrap();
        let serialized = serde_json::to_string(&incoming).unwrap();
        for _ in 0..64 {
            let mut decoded: SnapshotManifest = serde_json::from_str(&serialized).unwrap();
            decoded.ui_state = serde_json::from_str(
                r#"{"views":[{"a":1,"z":0}],"projects":{"alpha":1,"beta":2}}"#,
            )
            .unwrap();
            assert_eq!(transfer_contract(&decoded, &local).unwrap(), expected);
        }
        incoming.ui_state["projects"]["alpha"] = serde_json::json!(3);
        assert_ne!(transfer_contract(&incoming, &local).unwrap().0, expected.0);
        let mut changed_local = local.clone();
        changed_local.compatibility.schema_fingerprint = "new-schema".into();
        assert_ne!(
            transfer_contract(&incoming, &changed_local).unwrap().1,
            expected.1
        );
    }

    fn review_manifest() -> SnapshotManifest {
        let config = settings::default_config();
        SnapshotManifest {
            schema_version: snapshot::SNAPSHOT_SCHEMA,
            id: "20260101T120000Z-11111111222233334444555566667777".into(),
            created_at: "2026-09-12T17:28:02Z".into(),
            device_id: "original-device".into(),
            device_name: "Same computer name".into(),
            parent_id: None,
            additional_parent_ids: Vec::new(),
            selection_revision: config.selection.revision.clone(),
            selection: config.selection,
            compatibility: CompatibilityInfo {
                supported: false,
                adapter: "test".into(),
                state_migration: None,
                history_migration: None,
                schema_fingerprint: "test".into(),
                explanation: "test".into(),
            },
            codex_version: None,
            threads: Vec::new(),
            projects: Vec::new(),
            objects: Vec::new(),
            ui_state: serde_json::json!({}),
            session_index_lines: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn review_thread(id: &str, fingerprint: &str, parent: Option<&str>) -> ThreadExport {
        let source = parent
            .map(|id| {
                serde_json::json!({"subagent":{"thread_spawn":{"parent_thread_id":id}}}).to_string()
            })
            .unwrap_or_else(|| "cli".into());
        ThreadExport {
            id: id.into(),
            title: id.into(),
            project_id: None,
            projectless: true,
            archived: false,
            source_cwd: String::new(),
            rollout_relative_path: None,
            projectless_relative_root: None,
            attachments: Vec::new(),
            state_rows: BTreeMap::from([(
                "threads".into(),
                vec![DatabaseRow {
                    values: BTreeMap::from([("source".into(), SqlValue::Text(source))]),
                }],
            )]),
            history_rows: BTreeMap::new(),
            fingerprint: fingerprint.into(),
        }
    }

    #[test]
    fn missing_baseline_conflicts_can_keep_local_then_record_the_reviewed_ancestor() {
        let data = tempdir().unwrap();
        let config = settings::default_config();
        let mut incoming = review_manifest();
        incoming
            .threads
            .push(review_thread("chat", "morning", None));
        let mut local = incoming.clone();
        local.device_id = "new-device-identity".into();
        local.threads[0].fingerprint = "afternoon".into();
        // Matching device names do not authorize adopting a missing baseline.
        let changes = diff_for_pull(&config, &incoming, &local, None).unwrap();
        assert_eq!(changes[0].action, ChangeAction::Conflict);
        assert!(settings::load_local_state(data.path())
            .unwrap()
            .last_applied_snapshot_id
            .is_none());
        let preview = OperationPreview {
            operation_id: "review".into(),
            direction: Direction::Pull,
            snapshot_id: Some(incoming.id.clone()),
            estimated_bytes: 0,
            changes,
            warnings: Vec::new(),
            blocked_reasons: Vec::new(),
            requires_codex_close: false,
            required_mappings: Vec::new(),
        };
        let choices = HashMap::from([("thread:chat", &ConflictChoice::Local)]);
        assert!(decisions(&preview, &choices, &incoming)
            .values()
            .all(|decision| *decision == ApplyDecision::Local));
        fs::write(data.path().join("untouched.sqlite"), b"local database").unwrap();
        record_pull_baseline(data.path(), &incoming, std::slice::from_ref(&incoming.id)).unwrap();
        let state = settings::load_local_state(data.path()).unwrap();
        assert_eq!(
            state.last_applied_snapshot_id.as_deref(),
            Some(incoming.id.as_str())
        );
        assert!(state.last_pushed_snapshot_id.is_none());
        assert_eq!(
            fs::read(data.path().join("untouched.sqlite")).unwrap(),
            b"local database"
        );
        assert_eq!(
            diff_for_pull(&config, &incoming, &local, Some(&incoming)).unwrap()[0].action,
            ChangeAction::Unchanged
        );
        assert_eq!(
            diff_for_push(Some(&incoming), &local)[0].action,
            ChangeAction::Update
        );
        let use_incoming = HashMap::from([("thread:chat", &ConflictChoice::Incoming)]);
        assert!(decision_incoming(
            &decisions(&preview, &use_incoming, &incoming),
            "thread:chat"
        ));
    }

    #[test]
    fn excluding_parent_preserves_previously_shared_agent_histories() {
        let config = settings::default_config();
        let mut base = review_manifest();
        base.threads = vec![
            review_thread("parent", "p", None),
            review_thread("child", "c", Some("parent")),
        ];
        let mut excluded = base.clone();
        excluded.threads.clear();
        excluded.selection.excluded_thread_ids.push("parent".into());
        assert!(diff_for_push(Some(&base), &excluded).is_empty());
        assert!(diff_for_pull(&config, &excluded, &base, Some(&base))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn old_internal_tasks_and_their_files_are_not_offered_or_restored() {
        let config = settings::default_config();
        let local = review_manifest();
        let mut incoming = local.clone();
        let mut guardian = review_thread("internal", "approval", None);
        guardian.state_rows.get_mut("threads").unwrap()[0]
            .values
            .insert(
                "source".into(),
                SqlValue::Text(serde_json::json!({"subagent":{"other":"guardian"}}).to_string()),
            );
        guardian.projectless_relative_root = Some("internal".into());
        incoming.threads = vec![
            guardian,
            review_thread("internal-child", "child", Some("internal")),
            review_thread("normal", "conversation", None),
        ];
        incoming.objects.push(ObjectEntry {
            hash: "a".repeat(64),
            logical_path: "projectless/internal/files/file.txt".into(),
            kind: ObjectKind::ProjectlessFile,
            owner_id: "internal".into(),
            raw_size: 1,
            stored_size: 1,
            executable: false,
        });
        let changes = diff_for_pull(&config, &incoming, &local, None).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].key, "thread:normal");
        let preview = OperationPreview {
            operation_id: "old-snapshot-review".into(),
            direction: Direction::Pull,
            snapshot_id: Some(incoming.id.clone()),
            changes,
            warnings: vec![],
            blocked_reasons: vec![],
            estimated_bytes: 0,
            requires_codex_close: false,
            required_mappings: vec![],
        };
        let chosen = decisions(&preview, &HashMap::new(), &incoming);
        assert!(!decision_incoming(&chosen, "thread:internal"));
        assert!(!decision_incoming(&chosen, "thread:internal-child"));
        assert!(plan_file_operations(&config, &incoming, None, &chosen)
            .unwrap()
            .is_empty());
        // Two old snapshots may both retain internal tasks while omitting a
        // child's formerly shared file. That is not an authorized deletion.
        let mut legacy_base = incoming.clone();
        let mut child_file = legacy_base.objects[0].clone();
        child_file.owner_id = "internal-child".into();
        child_file.logical_path = "projectless/internal-child/files/file.txt".into();
        legacy_base.objects.push(child_file);
        let legacy_changes =
            diff_for_pull(&config, &incoming, &legacy_base, Some(&legacy_base)).unwrap();
        assert!(legacy_changes
            .iter()
            .all(|change| change.key == "thread:normal"));
        let mut next = incoming.clone();
        next.threads.retain(|thread| thread.id == "normal");
        next.objects.clear();
        assert!(diff_for_pull(&config, &next, &incoming, Some(&incoming))
            .unwrap()
            .iter()
            .all(|change| change.action == ChangeAction::Unchanged));
    }

    #[test]
    fn review_titles_are_bounded_without_altering_incoming_history() {
        let config = settings::default_config();
        let local = review_manifest();
        let mut incoming = local.clone();
        let mut thread = review_thread("chat", "history", None);
        thread.title = format!("A normal conversation\n{}", "more text ".repeat(2000));
        incoming.threads.push(thread);
        let original = incoming.threads[0].title.clone();
        let changes = diff_for_pull(&config, &incoming, &local, None).unwrap();
        assert!(changes[0].label.chars().count() <= 160);
        assert!(!changes[0].label.contains('\n'));
        assert_eq!(incoming.threads[0].title, original);
    }

    #[test]
    fn execution_rejects_pending_recovery_before_consuming_a_preview() {
        let directory = tempdir().unwrap();
        let engine = Engine::new(directory.path().join("app")).unwrap();
        let mut config = settings::default_config();
        config.codex_home = path_string(&directory.path().join("codex"));
        config.projectless_root = path_string(&directory.path().join("workspaces"));
        recovery::create(&engine.data_dir, "Interrupted test Pull", None, &[]).unwrap();
        assert!(matches!(
            engine.execute_push(&config, "preview"),
            Err(SpiceError::PendingRecovery)
        ));
        assert!(matches!(
            engine.execute_pull(&config, "preview", &[]),
            Err(SpiceError::PendingRecovery)
        ));
    }

    fn project(root: &str, common_id: &str, bundle: &str) -> ProjectExport {
        ProjectExport {
            id: "project-1".to_string(),
            legacy_id: Some("legacy-1".to_string()),
            name: "Portable project".to_string(),
            mode: ProjectMode::Full,
            source_roots: vec![root.to_string()],
            rows: BTreeMap::from([
                (
                    "projects".to_string(),
                    vec![DatabaseRow {
                        values: BTreeMap::from([
                            ("id".to_string(), SqlValue::Text("project-1".to_string())),
                            (
                                "name".to_string(),
                                SqlValue::Text("Portable project".to_string()),
                            ),
                        ]),
                    }],
                ),
                (
                    "project_roots".to_string(),
                    vec![DatabaseRow {
                        values: BTreeMap::from([
                            (
                                "project_id".to_string(),
                                SqlValue::Text("project-1".to_string()),
                            ),
                            ("position".to_string(), SqlValue::Integer(0)),
                            ("path".to_string(), SqlValue::Text(root.to_string())),
                        ]),
                    }],
                ),
            ]),
            git: vec![GitDescriptor {
                project_id: "project-1".to_string(),
                root_index: 0,
                common_id: common_id.to_string(),
                linked_worktree: false,
                head: Some("abc123".to_string()),
                branch: Some("main".to_string()),
                bundle_object: Some(bundle.to_string()),
                index_object: Some("index-hash".to_string()),
                index_pack_object: Some("index-pack-hash".to_string()),
            }],
        }
    }

    #[test]
    fn incoming_projects_require_explicit_mapping_even_when_source_exists_locally() {
        let source = tempdir().unwrap();
        let objects = tempdir().unwrap();
        let destination = tempdir().unwrap();
        let mut config = settings::default_config();
        let mut full = project(&path_string(source.path()), "unused", "unused");
        full.git.clear();
        let mut history = full.clone();
        history.id = "history".into();
        history.mode = ProjectMode::HistoryOnly;
        let incoming = snapshot::build_manifest(
            &config,
            codex::CodexExport {
                compatibility: CompatibilityInfo {
                    supported: true,
                    adapter: "test".into(),
                    state_migration: None,
                    history_migration: None,
                    schema_fingerprint: "test".into(),
                    explanation: "test".into(),
                },
                threads: vec![],
                projects: vec![full, history],
                pending_files: vec![],
                ui_state: serde_json::json!({}),
                session_index_lines: vec![],
                warnings: vec![],
            },
            &ObjectStore::new(objects.path()).unwrap(),
            None,
        )
        .unwrap();
        let mappings = required_mappings(&config, &incoming);
        assert_eq!(mappings.len(), 1);
        assert_eq!(mappings[0].project_id, "project-1");
        assert!(Path::new(&mappings[0].suggested_path).is_absolute());
        assert!(codex::destination_project_root(&incoming.projects[0], 0, &config).is_none());
        assert!(codex::destination_project_root(&incoming.projects[1], 0, &config).is_some());
        config
            .destination_roots
            .insert("project-1:0".into(), path_string(destination.path()));
        assert!(required_mappings(&config, &incoming).is_empty());
        assert_eq!(
            codex::destination_project_root(&incoming.projects[0], 0, &config).unwrap(),
            destination.path()
        );

        config.projects_root = path_string(&destination.path().join("suggestions"));
        config.destination_roots.clear();
        assert!(
            Path::new(&required_mappings(&config, &incoming)[0].suggested_path)
                .starts_with(&config.projects_root)
        );
    }

    #[test]
    fn project_fingerprint_ignores_device_paths_and_capture_only_git_fields() {
        let windows_a = project(r"C:\Users\alice\source", "common-a", "bundle-a");
        let windows_b = project(r"D:\Work\source", "common-b", "bundle-b");
        assert_eq!(
            project_fingerprint(&windows_a).unwrap(),
            project_fingerprint(&windows_b).unwrap()
        );

        let mut changed = windows_b;
        changed.git[0].head = Some("different-head".to_string());
        assert_ne!(
            project_fingerprint(&windows_a).unwrap(),
            project_fingerprint(&changed).unwrap()
        );
    }

    #[test]
    fn git_restore_preserves_history_index_and_working_tree_state() {
        let source = tempdir().unwrap();
        let destination = tempdir().unwrap();
        let object_root = tempdir().unwrap();
        let stage = tempdir().unwrap();
        command_status(
            {
                let mut command = platform::hidden_command("git");
                command
                    .arg("-C")
                    .arg(source.path())
                    .args(["init", "-b", "main"]);
                command
            },
            "initialize test repository",
        )
        .unwrap();
        for (key, value) in [
            ("user.email", "test@example.com"),
            ("user.name", "Spice Route Test"),
        ] {
            command_status(
                {
                    let mut command = platform::hidden_command("git");
                    command
                        .arg("-C")
                        .arg(source.path())
                        .args(["config", key, value]);
                    command
                },
                "configure test repository",
            )
            .unwrap();
        }
        fs::write(source.path().join("tracked.txt"), b"tracked\n").unwrap();
        fs::write(source.path().join("deleted.txt"), b"delete me\n").unwrap();
        command_status(
            {
                let mut command = platform::hidden_command("git");
                command
                    .arg("-C")
                    .arg(source.path())
                    .args(["add", "tracked.txt", "deleted.txt"]);
                command
            },
            "stage baseline",
        )
        .unwrap();
        command_status(
            {
                let mut command = platform::hidden_command("git");
                command
                    .arg("-C")
                    .arg(source.path())
                    .args(["commit", "-m", "baseline"]);
                command
            },
            "commit baseline",
        )
        .unwrap();
        fs::write(source.path().join("staged.txt"), b"staged version\n").unwrap();
        command_status(
            {
                let mut command = platform::hidden_command("git");
                command
                    .arg("-C")
                    .arg(source.path())
                    .args(["add", "staged.txt"]);
                command
            },
            "stage new file",
        )
        .unwrap();
        command_status(
            {
                let mut command = platform::hidden_command("git");
                command
                    .arg("-C")
                    .arg(source.path())
                    .args(["update-index", "--split-index"]);
                command
            },
            "enable split index",
        )
        .unwrap();
        fs::write(source.path().join("staged.txt"), b"working version\n").unwrap();
        fs::write(source.path().join("untracked.txt"), b"untracked\n").unwrap();
        fs::remove_file(source.path().join("deleted.txt")).unwrap();

        let store = ObjectStore::new(object_root.path()).unwrap();
        let mut objects = Vec::new();
        let mut warnings = Vec::new();
        let descriptor = snapshot::capture_git(
            source.path(),
            "project-1",
            0,
            &store,
            &mut objects,
            &mut warnings,
            None,
        )
        .unwrap()
        .unwrap();
        assert!(descriptor.bundle_object.is_some());
        assert!(descriptor.index_object.is_some());
        assert!(descriptor.index_pack_object.is_some());

        let config = settings::default_config();
        let manifest = SnapshotManifest {
            schema_version: snapshot::SNAPSHOT_SCHEMA,
            id: "git-test".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            device_id: config.device_id.clone(),
            device_name: config.device_name.clone(),
            parent_id: None,
            additional_parent_ids: Vec::new(),
            selection_revision: config.selection.revision.clone(),
            selection: config.selection.clone(),
            compatibility: CompatibilityInfo {
                supported: true,
                adapter: "test".to_string(),
                state_migration: Some(1),
                history_migration: Some(1),
                schema_fingerprint: "test".to_string(),
                explanation: "test".to_string(),
            },
            codex_version: Some("test".to_string()),
            threads: Vec::new(),
            projects: vec![ProjectExport {
                id: "project-1".to_string(),
                legacy_id: None,
                name: "Git test".to_string(),
                mode: ProjectMode::Full,
                source_roots: vec![source.path().to_string_lossy().into_owned()],
                rows: std::collections::BTreeMap::new(),
                git: vec![descriptor],
            }],
            objects,
            ui_state: serde_json::json!({}),
            session_index_lines: Vec::new(),
            warnings,
        };
        let destination_root = destination.path().join("restored");
        let operations = [GitOperation {
            destination: destination_root.clone(),
            descriptor: &manifest.projects[0].git[0],
        }];
        restore_git_groups(&store, &manifest, &operations, stage.path(), None, false).unwrap();
        for name in ["tracked.txt", "staged.txt", "untracked.txt"] {
            fs::copy(source.path().join(name), destination_root.join(name)).unwrap();
        }

        let staged = git_output(&destination_root, &["show", ":staged.txt"]).unwrap();
        assert_eq!(staged, "staged version");
        assert_eq!(
            fs::read_to_string(destination_root.join("staged.txt")).unwrap(),
            "working version\n"
        );
        let status = git_output(&destination_root, &["status", "--porcelain"]).unwrap();
        assert!(status.lines().any(|line| line.ends_with("D deleted.txt")));
        assert!(status.lines().any(|line| line == "AM staged.txt"));
        assert!(status.lines().any(|line| line == "?? untracked.txt"));
        command_status(
            {
                let mut command = platform::hidden_command("git");
                command.arg("-C").arg(&destination_root).args([
                    "fsck",
                    "--connectivity-only",
                    "--no-reflogs",
                ]);
                command
            },
            "verify test repository",
        )
        .unwrap();
    }

    #[test]
    fn newly_excluded_file_is_not_treated_as_a_source_deletion() {
        let destination = tempdir().unwrap();
        let mut config = settings::default_config();
        config.destination_roots.insert(
            "project-1:0".to_string(),
            destination.path().to_string_lossy().into_owned(),
        );
        let project = ProjectExport {
            id: "project-1".to_string(),
            legacy_id: None,
            name: "Project".to_string(),
            mode: ProjectMode::Full,
            source_roots: vec![r"C:\source".to_string()],
            rows: BTreeMap::new(),
            git: Vec::new(),
        };
        let bytes = b"keep locally";
        fs::write(destination.path().join("secret.txt"), bytes).unwrap();
        let object = ObjectEntry {
            hash: crate::util::sha256_bytes(bytes),
            logical_path: "projects/project-1/0/files/secret.txt".to_string(),
            kind: ObjectKind::ProjectFile,
            owner_id: "project-1".to_string(),
            raw_size: bytes.len() as u64,
            stored_size: bytes.len() as u64,
            executable: false,
        };
        let make_manifest =
            |selection: SelectionRules, objects: Vec<ObjectEntry>| SnapshotManifest {
                schema_version: snapshot::SNAPSHOT_SCHEMA,
                id: Uuid::new_v4().to_string(),
                created_at: "2026-01-01T00:00:00Z".to_string(),
                device_id: "device".to_string(),
                device_name: "Device".to_string(),
                parent_id: None,
                additional_parent_ids: Vec::new(),
                selection_revision: selection.revision.clone(),
                selection,
                compatibility: CompatibilityInfo {
                    supported: true,
                    adapter: "test".to_string(),
                    state_migration: Some(1),
                    history_migration: Some(1),
                    schema_fingerprint: "test".to_string(),
                    explanation: "test".to_string(),
                },
                codex_version: Some("test".to_string()),
                threads: Vec::new(),
                projects: vec![project.clone()],
                objects,
                ui_state: serde_json::json!({}),
                session_index_lines: Vec::new(),
                warnings: Vec::new(),
            };
        let baseline = make_manifest(config.selection.clone(), vec![object.clone()]);
        let local = make_manifest(config.selection.clone(), vec![object]);
        let mut excluded_selection = config.selection.clone();
        excluded_selection.extra_exclude_patterns = vec!["secret.txt".to_string()];
        let excluded = make_manifest(excluded_selection, Vec::new());

        let excluded_changes = diff_for_pull(&config, &excluded, &local, Some(&baseline)).unwrap();
        assert!(!excluded_changes
            .iter()
            .any(|change| change.key.ends_with("secret.txt")));

        let deleted = make_manifest(config.selection.clone(), Vec::new());
        let deleted_changes = diff_for_pull(&config, &deleted, &local, Some(&baseline)).unwrap();
        assert!(deleted_changes.iter().any(|change| {
            change.key.ends_with("secret.txt") && change.action == ChangeAction::Delete
        }));
    }

    #[test]
    fn cloud_cleanup_requires_phrase_and_preserves_shared_policy() {
        let local = tempdir().unwrap();
        let cloud = tempdir().unwrap();
        let codex = tempdir().unwrap();
        let projectless = tempdir().unwrap();
        let engine = Engine::new(local.path().join("app-data")).unwrap();
        let mut config = settings::default_config();
        config.onboarding_complete = true;
        config.codex_home = codex.path().to_string_lossy().into_owned();
        config.projectless_root = projectless.path().to_string_lossy().into_owned();
        config.cloud_root = cloud.path().to_string_lossy().into_owned();
        settings::save_config(&engine.data_dir, &config).unwrap();
        let root = settings::cloud_store_root(&config);
        fs::write(root.join("snapshots").join("one.json"), b"manifest").unwrap();
        fs::create_dir_all(root.join("objects").join("ab")).unwrap();
        fs::write(
            root.join("objects")
                .join("ab")
                .join(format!("{}.zst", "a".repeat(64))),
            b"object",
        )
        .unwrap();

        let preview = engine.preview_cloud_cleanup(&config).unwrap();
        assert_eq!(preview.snapshot_count, 1);
        assert_eq!(preview.object_count, 1);
        assert!(engine
            .execute_cloud_cleanup(&config, &preview.operation_id, "wrong")
            .is_err());
        let result = engine
            .execute_cloud_cleanup(&config, &preview.operation_id, &preview.confirmation_phrase)
            .unwrap();

        assert_eq!(result.snapshots_removed, 1);
        assert_eq!(result.objects_removed, 1);
        assert!(root.join("format.json").is_file());
        assert!(root.join("selection.json").is_file());
        assert_eq!(fs::read_dir(root.join("snapshots")).unwrap().count(), 0);
        assert_eq!(fs::read_dir(root.join("objects")).unwrap().count(), 0);
    }
}
