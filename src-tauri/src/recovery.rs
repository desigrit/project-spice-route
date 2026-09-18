use crate::error::{Result, SpiceError};
use crate::models::{RecoveryStatus, RecoverySummary};
use crate::platform;
use crate::util::{directory_size, hash_json, read_json, write_json};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use uuid::Uuid;
use walkdir::WalkDir;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecoveryManifest {
    id: String,
    created_at: String,
    reason: String,
    source_snapshot_id: Option<String>,
    status: RecoveryStatus,
    entries: Vec<RecoveryEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecoveryEntry {
    original_path: String,
    backup_name: String,
    existed: bool,
    directory: bool,
    #[serde(default)]
    backup_fingerprint: Option<String>,
}

pub fn has_pending(data_dir: &Path) -> bool {
    // Gates only need journal status. Counting all rollback bytes here made
    // every startup and transfer gate walk the same saved project trees again.
    // An unreadable journal is unresolved, not permission to write live data.
    pending_manifest(data_dir).unwrap_or(true)
}

fn pending_manifest(data_dir: &Path) -> Result<bool> {
    let Some(entries) = recovery_directories(data_dir)? else {
        return Ok(false);
    };
    for entry in entries {
        let entry = entry?;
        if !is_recovery_directory(&entry)? {
            continue;
        }
        let manifest = read_recovery_manifest(&entry.path())?;
        if matches!(manifest.status, RecoveryStatus::Pending) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn recovery_directories(data_dir: &Path) -> Result<Option<fs::ReadDir>> {
    match fs::read_dir(recovery_root(data_dir)) {
        Ok(entries) => Ok(Some(entries)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn is_recovery_directory(entry: &fs::DirEntry) -> Result<bool> {
    let kind = entry.file_type()?;
    if kind.is_symlink() {
        return Err(SpiceError::User(format!(
            "Recovery data contains an unsupported link: {}",
            entry.path().display()
        )));
    }
    Ok(kind.is_dir())
}

fn read_recovery_manifest(directory: &Path) -> Result<RecoveryManifest> {
    let path = directory.join("manifest.json");
    read_json(&path).map_err(|error| SpiceError::User(format!(
        "Recovery journal could not be read at {}: {error}. Handoffs are paused to protect local data. Keep this recovery folder for diagnosis.",
        path.display()
    )))
}

pub fn create(
    data_dir: &Path,
    reason: &str,
    source_snapshot_id: Option<String>,
    targets: &[PathBuf],
) -> Result<String> {
    create_cancellable(data_dir, reason, source_snapshot_id, targets, None)
}

pub fn create_cancellable(
    data_dir: &Path,
    reason: &str,
    source_snapshot_id: Option<String>,
    targets: &[PathBuf],
    cancel: Option<&AtomicBool>,
) -> Result<String> {
    let id = format!(
        "{}-{}",
        Utc::now().format("%Y%m%dT%H%M%SZ"),
        Uuid::new_v4().simple()
    );
    let directory = recovery_root(data_dir).join(&id);
    let files = directory.join("files");
    fs::create_dir_all(&files)?;
    let result = (|| -> Result<String> {
        let mut entries = Vec::new();
        let mut seen = HashSet::new();
        for path in targets {
            check_cancel(cancel)?;
            let absolute = if path.is_absolute() {
                path.clone()
            } else {
                std::env::current_dir()?.join(path)
            };
            let key = absolute.to_string_lossy().to_lowercase();
            if !seen.insert(key) {
                continue;
            }
            let existed = absolute.exists();
            let is_directory = absolute.is_dir();
            let backup_name = format!("{:06}", entries.len());
            if existed {
                let destination = files.join(&backup_name);
                if is_directory {
                    copy_directory_cancellable(&absolute, &destination, cancel)?;
                } else {
                    copy_file_cancellable(&absolute, &destination, cancel)?;
                }
            }
            let backup_fingerprint = existed
                .then(|| fingerprint_path_cancellable(&files.join(&backup_name), cancel))
                .transpose()?;
            if let Some(backup) = &backup_fingerprint {
                if fingerprint_path_cancellable(&absolute, cancel)? != *backup {
                    return Err(SpiceError::User(format!("{} changed while its rollback copy was being created. No live files were changed; review a fresh Pull.", absolute.display())));
                }
            }
            entries.push(RecoveryEntry {
                original_path: absolute.to_string_lossy().into_owned(),
                backup_name,
                existed,
                directory: is_directory,
                backup_fingerprint,
            });
        }
        let manifest = RecoveryManifest {
            id: id.clone(),
            created_at: Utc::now().to_rfc3339(),
            reason: reason.to_string(),
            source_snapshot_id,
            status: RecoveryStatus::Pending,
            entries,
        };
        write_json(&directory.join("manifest.json"), &manifest)?;
        Ok(id.clone())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&directory);
    }
    result
}

pub fn complete(data_dir: &Path, id: &str) -> Result<()> {
    let path = manifest_path(data_dir, id)?;
    let mut manifest: RecoveryManifest = read_json(&path)?;
    manifest.status = RecoveryStatus::Available;
    write_json(&path, &manifest)?;
    prune(data_dir, 10)
}

pub fn list(data_dir: &Path) -> Result<Vec<RecoverySummary>> {
    let Some(entries) = recovery_directories(data_dir)? else {
        return Ok(Vec::new());
    };
    let mut result = Vec::new();
    for entry in entries {
        let entry = entry?;
        if !is_recovery_directory(&entry)? {
            continue;
        }
        let manifest = read_recovery_manifest(&entry.path())?;
        result.push(RecoverySummary {
            id: manifest.id,
            created_at: manifest.created_at,
            reason: manifest.reason,
            source_snapshot_id: manifest.source_snapshot_id,
            status: manifest.status,
            size_bytes: directory_size(&entry.path()),
        });
    }
    result.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(result)
}

pub fn restore(data_dir: &Path, id: &str) -> Result<()> {
    restore_cancellable(data_dir, id, Arc::new(AtomicBool::new(false)))
}

pub fn restore_cancellable(data_dir: &Path, id: &str, cancel: Arc<AtomicBool>) -> Result<()> {
    platform::assert_codex_closed()?;
    let monitor = platform::WriterMonitor::start(cancel.clone())?;
    let result = restore_files_cancellable(
        data_dir,
        id,
        Some(cancel.as_ref()),
        &mut |target| {
            monitor.check()?;
            if is_codex_database_target(target) {
                platform::assert_codex_closed()?;
            }
            Ok(())
        },
        &mut || {
            platform::assert_codex_closed()?;
            monitor.check()
        },
    );
    result.map_err(|error| monitor.explain_error(error))
}

#[cfg(test)]
fn restore_files(data_dir: &Path, id: &str) -> Result<()> {
    restore_files_cancellable(data_dir, id, None, &mut |_| Ok(()), &mut || Ok(()))
}

fn is_codex_database_target(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some(
            "state_5.sqlite"
                | "thread_history_1.sqlite"
                | ".codex-global-state.json"
                | "session_index.jsonl"
                | "state_5.sqlite-wal"
                | "state_5.sqlite-shm"
                | "thread_history_1.sqlite-wal"
                | "thread_history_1.sqlite-shm"
        )
    )
}

fn restore_files_cancellable(
    data_dir: &Path,
    id: &str,
    cancel: Option<&AtomicBool>,
    before_mutation: &mut dyn FnMut(&Path) -> Result<()>,
    before_completion: &mut dyn FnMut() -> Result<()>,
) -> Result<()> {
    check_cancel(cancel)?;
    let path = manifest_path(data_dir, id)?;
    let mut manifest: RecoveryManifest = read_json(&path)?;
    if manifest.id != id {
        return Err(SpiceError::User(
            "The recovery journal identity does not match its folder.".into(),
        ));
    }
    let directory = path.parent().expect("manifest has parent");
    // Validate every target before touching any target. A malformed later entry
    // must never be discovered only after earlier paths have already changed.
    for entry in &manifest.entries {
        check_cancel(cancel)?;
        let original = Path::new(&entry.original_path);
        if !original.is_absolute()
            || original.parent().is_none()
            || original
                .components()
                .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(SpiceError::User(
                "Recovery contains an invalid absolute target.".into(),
            ));
        }
        let backup_name = Path::new(&entry.backup_name);
        if backup_name.components().count() != 1
            || !matches!(backup_name.components().next(), Some(Component::Normal(_)))
        {
            return Err(SpiceError::User(
                "Recovery contains an invalid backup name.".into(),
            ));
        }
        if fs::symlink_metadata(original).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            return Err(SpiceError::User(format!(
                "A recovery target is a symbolic link: {}",
                original.display()
            )));
        }
    }
    let mut verified_fingerprints = Vec::with_capacity(manifest.entries.len());
    for entry in &manifest.entries {
        check_cancel(cancel)?;
        if !entry.existed {
            verified_fingerprints.push(None);
            continue;
        }
        let backup = directory.join("files").join(&entry.backup_name);
        if !backup.exists() || backup.is_dir() != entry.directory {
            return Err(SpiceError::User(format!(
                "Recovery data is missing or has the wrong type for {}",
                entry.original_path
            )));
        }
        let actual = fingerprint_path_cancellable(&backup, cancel)?;
        if entry
            .backup_fingerprint
            .as_ref()
            .is_some_and(|expected| *expected != actual)
        {
            return Err(SpiceError::User(format!(
                "Recovery data failed its integrity check for {}",
                entry.original_path
            )));
        }
        verified_fingerprints.push(Some(actual));
    }
    check_cancel(cancel)?;
    // Available points become unresolved before live writes. A crash, cancelled
    // copy, or writer reopening leaves a journal that blocks subsequent handoffs.
    manifest.status = RecoveryStatus::Pending;
    write_json(&path, &manifest)?;
    for entry in manifest.entries.iter().rev() {
        check_cancel(cancel)?;
        let original = PathBuf::from(&entry.original_path);
        before_mutation(&original)?;
        check_cancel(cancel)?;
        if entry.existed {
            let backup = directory.join("files").join(&entry.backup_name);
            if original.exists() {
                remove_exact(&original, original.is_dir())?;
            }
            check_cancel(cancel)?;
            if entry.directory {
                copy_directory_guarded(&backup, &original, cancel, before_mutation)?;
            } else {
                before_mutation(&original)?;
                copy_file_cancellable(&backup, &original, cancel)?;
            }
        } else if original.exists() {
            remove_exact(&original, original.is_dir())?;
        }
    }
    for (entry, expected) in manifest.entries.iter().zip(verified_fingerprints) {
        check_cancel(cancel)?;
        let original = Path::new(&entry.original_path);
        if let Some(expected) = expected {
            if fingerprint_path_cancellable(original, cancel)? != expected {
                return Err(SpiceError::User(format!("Restored content failed verification for {}. The recovery point remains pending.", original.display())));
            }
        } else if original.exists() {
            return Err(SpiceError::User(format!(
                "A newly created target remains after recovery: {}",
                original.display()
            )));
        }
    }
    before_completion()?;
    check_cancel(cancel)?;
    manifest.status = RecoveryStatus::Restored;
    write_json(&path, &manifest)
}

fn manifest_path(data_dir: &Path, id: &str) -> Result<PathBuf> {
    if id.contains(['/', '\\']) || id.contains("..") {
        return Err(SpiceError::User("Invalid recovery id.".to_string()));
    }
    let path = recovery_root(data_dir).join(id).join("manifest.json");
    if !path.is_file() {
        return Err(SpiceError::User(format!(
            "Recovery point {id} was not found."
        )));
    }
    Ok(path)
}

fn recovery_root(data_dir: &Path) -> PathBuf {
    data_dir.join("recoveries")
}

fn copy_directory_cancellable(
    source: &Path,
    destination: &Path,
    cancel: Option<&AtomicBool>,
) -> Result<()> {
    copy_directory_guarded(source, destination, cancel, &mut |_| Ok(()))
}

fn copy_directory_guarded(
    source: &Path,
    destination: &Path,
    cancel: Option<&AtomicBool>,
    before_mutation: &mut dyn FnMut(&Path) -> Result<()>,
) -> Result<()> {
    before_mutation(destination)?;
    check_cancel(cancel)?;
    fs::create_dir_all(destination)?;
    for entry in WalkDir::new(source).follow_links(false) {
        check_cancel(cancel)?;
        let entry = entry.map_err(|error| {
            SpiceError::User(format!("Could not back up {}: {error}", source.display()))
        })?;
        let relative = entry
            .path()
            .strip_prefix(source)
            .map_err(|_| SpiceError::User("Recovery path escaped its root.".to_string()))?;
        let target = destination.join(relative);
        if entry.file_type().is_symlink() {
            return Err(SpiceError::User(format!(
                "A recovery target contains a symbolic link that cannot be backed up safely: {}",
                entry.path().display()
            )));
        } else if entry.file_type().is_dir() {
            before_mutation(&target)?;
            check_cancel(cancel)?;
            fs::create_dir_all(target)?;
        } else if entry.file_type().is_file() {
            before_mutation(&target)?;
            check_cancel(cancel)?;
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            copy_file_cancellable(entry.path(), &target, cancel)?;
        }
    }
    Ok(())
}

fn fingerprint_path_cancellable(path: &Path, cancel: Option<&AtomicBool>) -> Result<String> {
    check_cancel(cancel)?;
    if fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(SpiceError::User(format!(
            "Recovery data contains an unsupported symbolic link: {}",
            path.display()
        )));
    }
    if path.is_file() {
        let mut reader = BufReader::new(File::open(path)?);
        let mut hasher = sha2::Sha256::new();
        let mut buffer = vec![0_u8; 1024 * 1024];
        loop {
            check_cancel(cancel)?;
            let count = reader.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
        }
        return Ok(format!("{:x}", hasher.finalize()));
    }
    if !path.is_dir() {
        return Err(SpiceError::User(format!(
            "Recovery path is not a regular file or directory: {}",
            path.display()
        )));
    }
    let mut entries = Vec::new();
    for entry in WalkDir::new(path).follow_links(false) {
        check_cancel(cancel)?;
        let entry = entry.map_err(|error| {
            SpiceError::User(format!("Could not verify recovery data: {error}"))
        })?;
        if entry.file_type().is_symlink() {
            return Err(SpiceError::User(format!(
                "Recovery data contains an unsupported symbolic link: {}",
                entry.path().display()
            )));
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(path)
            .map_err(|_| SpiceError::User("Recovery data escaped its root.".to_string()))?
            .to_string_lossy()
            .replace('\\', "/");
        let hash = fingerprint_path_cancellable(entry.path(), cancel)?;
        let size = fs::metadata(entry.path())?.len();
        entries.push((relative, hash, size));
    }
    entries.sort();
    hash_json(&entries)
}

fn copy_file_cancellable(
    source: &Path,
    destination: &Path,
    cancel: Option<&AtomicBool>,
) -> Result<()> {
    check_cancel(cancel)?;
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut reader = BufReader::new(File::open(source)?);
    let mut writer = BufWriter::new(File::create(destination)?);
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        check_cancel(cancel)?;
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        writer.write_all(&buffer[..count])?;
    }
    writer.flush()?;
    writer.get_ref().sync_all()?;
    check_cancel(cancel)?;
    fs::set_permissions(destination, fs::metadata(source)?.permissions())?;
    Ok(())
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

fn remove_exact(path: &Path, directory: bool) -> Result<()> {
    if path.parent().is_none() || path == Path::new("/") {
        return Err(SpiceError::User(format!(
            "Refusing to remove broad recovery target {}",
            path.display()
        )));
    }
    if directory {
        fs::remove_dir_all(path)?;
    } else {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn prune(data_dir: &Path, keep: usize) -> Result<()> {
    let Some(entries) = recovery_directories(data_dir)? else {
        return Ok(());
    };
    let mut completed = Vec::new();
    for entry in entries {
        let entry = entry?;
        if !is_recovery_directory(&entry)? {
            continue;
        }
        let manifest = read_recovery_manifest(&entry.path())?;
        if !matches!(manifest.status, RecoveryStatus::Pending) {
            completed.push((entry.path(), manifest.created_at));
        }
    }
    // Read every journal before pruning any directory. Unknown recovery data
    // stops pruning, and retention never needs to walk the backup file trees.
    completed.sort_by(|a, b| b.1.cmp(&a.1).then(b.0.cmp(&a.0)));
    for (target, _) in completed.into_iter().skip(keep) {
        if target.parent() == Some(recovery_root(data_dir).as_path()) && target.is_dir() {
            fs::remove_dir_all(target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_status_fixture(data_dir: &Path, status: RecoveryStatus) -> PathBuf {
        let directory = recovery_root(data_dir).join("fixture");
        fs::create_dir_all(directory.join("files")).unwrap();
        let path = directory.join("manifest.json");
        write_json(
            &path,
            &RecoveryManifest {
                id: "fixture".into(),
                created_at: "2026-09-18T12:00:00Z".into(),
                reason: "Disposable recovery fixture".into(),
                source_snapshot_id: None,
                status,
                entries: Vec::new(),
            },
        )
        .unwrap();
        path
    }

    fn retention_fixture(data_dir: &Path) {
        for index in 0..14 {
            let id = format!("point-{index:02}");
            let directory = recovery_root(data_dir).join(&id);
            fs::create_dir_all(directory.join("files")).unwrap();
            write_json(
                &directory.join("manifest.json"),
                &RecoveryManifest {
                    id,
                    created_at: format!("2026-09-18T12:00:{index:02}Z"),
                    reason: "Disposable retention fixture".into(),
                    source_snapshot_id: None,
                    status: match index {
                        0 | 1 => RecoveryStatus::Pending,
                        2 => RecoveryStatus::Restored,
                        _ => RecoveryStatus::Available,
                    },
                    entries: Vec::new(),
                },
            )
            .unwrap();
        }
    }

    #[test]
    fn retention_keeps_ten_completed_points_and_all_unresolved_points() {
        let app = tempdir().unwrap();
        retention_fixture(app.path());
        prune(app.path(), 10).unwrap();
        let root = recovery_root(app.path());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 12);
        for index in 0..14 {
            assert_eq!(
                root.join(format!("point-{index:02}")).exists(),
                index != 2 && index != 3
            );
        }
        assert!(has_pending(app.path()));
    }

    #[test]
    fn retention_leaves_every_point_intact_if_any_journal_is_malformed() {
        let app = tempdir().unwrap();
        retention_fixture(app.path());
        let root = recovery_root(app.path());
        fs::write(root.join("point-13/manifest.json"), b"corrupt journal").unwrap();
        assert!(prune(app.path(), 10).is_err());
        assert_eq!(fs::read_dir(root).unwrap().count(), 14);
    }

    #[test]
    fn pending_gate_reads_journal_status_without_inspecting_backup_contents() {
        let app = tempdir().unwrap();
        assert!(!has_pending(app.path()));
        let path = write_status_fixture(app.path(), RecoveryStatus::Pending);
        assert!(has_pending(app.path()));
        let backup = path.parent().unwrap().join("files").join("project");
        fs::create_dir_all(&backup).unwrap();
        // Backed-up user files are opaque to the status check, even when named
        // like a journal or containing an apparent pending status.
        fs::write(backup.join("manifest.json"), b"not a recovery journal").unwrap();
        fs::write(backup.join("nested.json"), br#"{"status":"pending"}"#).unwrap();
        for status in [RecoveryStatus::Available, RecoveryStatus::Restored] {
            write_status_fixture(app.path(), status);
            assert!(!has_pending(app.path()));
        }
    }

    #[test]
    fn malformed_missing_or_unrecognized_recovery_journals_block_handoffs() {
        let app = tempdir().unwrap();
        let path = write_status_fixture(app.path(), RecoveryStatus::Available);
        for bytes in [
            b"{incomplete".as_slice(),
            br#"{"status":"available"}"#.as_slice(),
        ] {
            fs::write(&path, bytes).unwrap();
            assert!(has_pending(app.path()));
            assert!(list(app.path())
                .unwrap_err()
                .to_string()
                .contains("Recovery journal could not be read"));
        }
        write_status_fixture(app.path(), RecoveryStatus::Available);
        let mut json: serde_json::Value = read_json(&path).unwrap();
        json["status"] = serde_json::json!("unrecognized");
        write_json(&path, &json).unwrap();
        assert!(has_pending(app.path()));
        fs::remove_file(&path).unwrap();
        assert!(has_pending(app.path()));
        assert!(list(app.path()).is_err());
    }

    #[test]
    fn invalid_recovery_root_fails_closed_instead_of_looking_empty() {
        let app = tempdir().unwrap();
        fs::write(recovery_root(app.path()), b"unexpected file").unwrap();
        assert!(has_pending(app.path()));
        assert!(list(app.path()).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn unreadable_recovery_manifest_blocks_until_it_can_be_read() {
        use std::os::windows::fs::OpenOptionsExt;
        let app = tempdir().unwrap();
        let path = write_status_fixture(app.path(), RecoveryStatus::Available);
        let lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        assert!(has_pending(app.path()));
        assert!(list(app.path()).is_err());
        drop(lock);
        assert!(!has_pending(app.path()));
    }

    #[test]
    fn rollback_restores_real_schema_54_and_new_fields_after_interrupted_changes() {
        let app = tempdir().unwrap();
        let home = tempdir().unwrap();
        let fixture: serde_json::Value =
            serde_json::from_str(crate::compatibility::PROFILES[1].schema).unwrap();
        let mut targets = Vec::new();
        for database in ["state_5.sqlite", "thread_history_1.sqlite"] {
            let path = home.path().join(database);
            let db = rusqlite::Connection::open(&path).unwrap();
            let objects = fixture[database]["objects"].as_array().unwrap();
            for kind in ["table", "index", "trigger"] {
                for object in objects.iter().filter(|obj| obj["type"] == kind) {
                    db.execute_batch(object["sql"].as_str().unwrap()).unwrap();
                }
            }
            db.execute("INSERT INTO _sqlx_migrations(version,description,success,checksum,execution_time) VALUES(?1,'fixture',1,X'00',0)", [fixture[database]["migration"].as_i64().unwrap()]).unwrap();
            targets.push(path);
        }
        let db = rusqlite::Connection::open(&targets[0]).unwrap();
        db.execute_batch("INSERT INTO threads(id,rollout_path,created_at,updated_at,source,model_provider,cwd,title,sandbox_policy,approval_mode,originator,daybreak_enabled) VALUES('existing','rollout',1,1,'cli','openai','workspace','Existing','local','local','desktop',1)").unwrap();
        drop(db);
        let before: Vec<_> = targets.iter().map(|path| fs::read(path).unwrap()).collect();
        let id = create(app.path(), "Before cross-version import", None, &targets).unwrap();
        let db = rusqlite::Connection::open(&targets[0]).unwrap();
        db.execute_batch(
            "UPDATE threads SET title='partial replacement',originator=NULL,daybreak_enabled=NULL",
        )
        .unwrap();
        drop(db);
        assert!(has_pending(app.path()));
        restore_files(app.path(), &id).unwrap();
        for (path, bytes) in targets.iter().zip(before) {
            assert_eq!(fs::read(path).unwrap(), bytes);
        }
        crate::codex::verify_databases(home.path()).unwrap();
        assert_eq!(
            crate::codex::inspect(home.path()).unwrap().state_migration,
            Some(54)
        );
    }

    #[test]
    fn interrupted_manual_restore_marks_available_point_pending_and_can_resume() {
        let data = tempdir().unwrap();
        let workspace = tempdir().unwrap();
        let first = workspace.path().join("first.txt");
        let second = workspace.path().join("second.txt");
        fs::write(&first, "first before").unwrap();
        fs::write(&second, "second before").unwrap();
        let id = create(
            data.path(),
            "manual rollback",
            None,
            &[first.clone(), second.clone()],
        )
        .unwrap();
        complete(data.path(), &id).unwrap();
        assert!(!has_pending(data.path()));
        fs::write(&first, "first after").unwrap();
        fs::write(&second, "second after").unwrap();
        let cancel = AtomicBool::new(false);
        let result = restore_files_cancellable(
            data.path(),
            &id,
            Some(&cancel),
            &mut |target| {
                if target == first {
                    cancel.store(true, Ordering::SeqCst);
                }
                Ok(())
            },
            &mut || Ok(()),
        );
        assert!(matches!(result, Err(SpiceError::Cancelled)));
        assert!(has_pending(data.path()));
        assert_eq!(fs::read_to_string(&second).unwrap(), "second before");
        assert_eq!(fs::read_to_string(&first).unwrap(), "first after");
        restore_files(data.path(), &id).unwrap();
        assert!(!has_pending(data.path()));
        assert_eq!(fs::read_to_string(&first).unwrap(), "first before");
        let journal: RecoveryManifest =
            read_json(&manifest_path(data.path(), &id).unwrap()).unwrap();
        assert!(matches!(journal.status, RecoveryStatus::Restored));
    }

    #[test]
    fn invalid_later_target_is_rejected_before_any_restore_mutation() {
        let data = tempdir().unwrap();
        let workspace = tempdir().unwrap();
        let existing = workspace.path().join("existing.txt");
        fs::write(&existing, "before").unwrap();
        let id = create(data.path(), "test", None, std::slice::from_ref(&existing)).unwrap();
        complete(data.path(), &id).unwrap();
        fs::write(&existing, "current").unwrap();
        let path = manifest_path(data.path(), &id).unwrap();
        let mut journal: RecoveryManifest = read_json(&path).unwrap();
        journal.entries.insert(
            0,
            RecoveryEntry {
                original_path: "relative-target.txt".into(),
                backup_name: "000001".into(),
                existed: false,
                directory: false,
                backup_fingerprint: None,
            },
        );
        write_json(&path, &journal).unwrap();
        assert!(restore_files(data.path(), &id).is_err());
        assert_eq!(fs::read_to_string(&existing).unwrap(), "current");
        assert!(!has_pending(data.path()));
        let unchanged: RecoveryManifest = read_json(&path).unwrap();
        assert!(matches!(unchanged.status, RecoveryStatus::Available));
    }

    #[test]
    fn failed_backup_verification_does_not_begin_a_manual_restore() {
        let data = tempdir().unwrap();
        let workspace = tempdir().unwrap();
        let first = workspace.path().join("first.txt");
        let second = workspace.path().join("second.txt");
        fs::write(&first, "first before").unwrap();
        fs::write(&second, "second before").unwrap();
        let id = create(data.path(), "test", None, &[first.clone(), second.clone()]).unwrap();
        complete(data.path(), &id).unwrap();
        fs::write(&first, "first current").unwrap();
        fs::write(&second, "second current").unwrap();
        fs::write(
            recovery_root(data.path()).join(&id).join("files/000000"),
            "corrupt",
        )
        .unwrap();
        assert!(restore_files(data.path(), &id)
            .unwrap_err()
            .to_string()
            .contains("integrity check"));
        assert_eq!(fs::read_to_string(&first).unwrap(), "first current");
        assert_eq!(fs::read_to_string(&second).unwrap(), "second current");
        assert!(!has_pending(data.path()));
    }

    #[test]
    fn cancellation_before_completion_keeps_recovery_pending_and_guards_nested_databases() {
        let data = tempdir().unwrap();
        let workspace = tempdir().unwrap();
        let folder = workspace.path().join("profile");
        fs::create_dir_all(&folder).unwrap();
        let database = folder.join("state_5.sqlite");
        fs::write(&database, "original fixture database").unwrap();
        let id = create(data.path(), "test", None, std::slice::from_ref(&folder)).unwrap();
        complete(data.path(), &id).unwrap();
        fs::write(&database, "changed fixture database").unwrap();
        let cancel = AtomicBool::new(false);
        let mut guarded_database = false;
        let result = restore_files_cancellable(
            data.path(),
            &id,
            Some(&cancel),
            &mut |target| {
                if is_codex_database_target(target) {
                    guarded_database = true;
                }
                Ok(())
            },
            &mut || {
                cancel.store(true, Ordering::SeqCst);
                Ok(())
            },
        );
        assert!(matches!(result, Err(SpiceError::Cancelled)));
        assert!(guarded_database);
        assert!(has_pending(data.path()));
        assert_eq!(
            fs::read_to_string(database).unwrap(),
            "original fixture database"
        );
        restore_files(data.path(), &id).unwrap();
        assert!(!has_pending(data.path()));
    }

    #[test]
    fn restore_returns_files_and_removes_new_targets() {
        let data = tempdir().unwrap();
        let workspace = tempdir().unwrap();
        let existing = workspace.path().join("existing.txt");
        let created = workspace.path().join("created.txt");
        fs::write(&existing, "before").unwrap();
        let id = create(
            data.path(),
            "test",
            None,
            &[existing.clone(), created.clone()],
        )
        .unwrap();
        fs::write(&existing, "after").unwrap();
        fs::write(&created, "new").unwrap();
        restore_files(data.path(), &id).unwrap();
        assert_eq!(fs::read_to_string(existing).unwrap(), "before");
        assert!(!created.exists());
    }

    #[test]
    fn corrupt_recovery_is_rejected_before_live_files_change() {
        let data = tempdir().unwrap();
        let workspace = tempdir().unwrap();
        let existing = workspace.path().join("existing.txt");
        fs::write(&existing, "before").unwrap();
        let id = create(data.path(), "test", None, std::slice::from_ref(&existing)).unwrap();
        fs::write(&existing, "current").unwrap();
        fs::write(
            recovery_root(data.path())
                .join(&id)
                .join("files")
                .join("000000"),
            "corrupt",
        )
        .unwrap();

        assert!(restore_files(data.path(), &id).is_err());
        assert_eq!(fs::read_to_string(existing).unwrap(), "current");
    }

    #[test]
    fn cancelled_backup_leaves_no_recovery_point() {
        let data = tempdir().unwrap();
        let workspace = tempdir().unwrap();
        let existing = workspace.path().join("existing.txt");
        fs::write(&existing, "before").unwrap();
        let cancel = AtomicBool::new(true);

        let error =
            create_cancellable(data.path(), "test", None, &[existing], Some(&cancel)).unwrap_err();

        assert!(matches!(error, SpiceError::Cancelled));
        assert!(list(data.path()).unwrap().is_empty());
    }
}
