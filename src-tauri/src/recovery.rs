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
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
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
    list(data_dir)
        .map(|items| {
            items
                .iter()
                .any(|item| matches!(item.status, RecoveryStatus::Pending))
        })
        .unwrap_or(true)
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
    let root = recovery_root(data_dir);
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path().join("manifest.json");
        if !path.is_file() {
            continue;
        }
        if let Ok(manifest) = read_json::<RecoveryManifest>(&path) {
            result.push(RecoverySummary {
                id: manifest.id,
                created_at: manifest.created_at,
                reason: manifest.reason,
                source_snapshot_id: manifest.source_snapshot_id,
                status: manifest.status,
                size_bytes: directory_size(&entry.path()),
            });
        }
    }
    result.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(result)
}

pub fn restore(data_dir: &Path, id: &str) -> Result<()> {
    platform::assert_codex_closed()?;
    restore_files(data_dir, id)
}

fn restore_files(data_dir: &Path, id: &str) -> Result<()> {
    let path = manifest_path(data_dir, id)?;
    let mut manifest: RecoveryManifest = read_json(&path)?;
    let directory = path.parent().expect("manifest has parent");
    for entry in &manifest.entries {
        if !entry.existed {
            continue;
        }
        let backup = directory.join("files").join(&entry.backup_name);
        if !backup.exists() {
            return Err(SpiceError::User(format!(
                "Recovery data is missing for {}",
                entry.original_path
            )));
        }
        if let Some(expected) = &entry.backup_fingerprint {
            if fingerprint_path(&backup)? != *expected {
                return Err(SpiceError::User(format!(
                    "Recovery data failed its integrity check for {}",
                    entry.original_path
                )));
            }
        }
    }
    for entry in manifest.entries.iter().rev() {
        let original = PathBuf::from(&entry.original_path);
        if !original.is_absolute() {
            return Err(SpiceError::User(
                "Recovery contains a non-absolute target.".to_string(),
            ));
        }
        if entry.existed {
            let backup = directory.join("files").join(&entry.backup_name);
            if original.exists() {
                remove_exact(&original, entry.directory)?;
            }
            if entry.directory {
                copy_directory(&backup, &original)?;
            } else {
                if let Some(parent) = original.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(backup, &original)?;
            }
        } else if original.exists() {
            remove_exact(&original, original.is_dir())?;
        }
    }
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

fn copy_directory(source: &Path, destination: &Path) -> Result<()> {
    copy_directory_cancellable(source, destination, None)
}

fn copy_directory_cancellable(
    source: &Path,
    destination: &Path,
    cancel: Option<&AtomicBool>,
) -> Result<()> {
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
            fs::create_dir_all(target)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            copy_file_cancellable(entry.path(), &target, cancel)?;
        }
    }
    Ok(())
}

fn fingerprint_path(path: &Path) -> Result<String> {
    fingerprint_path_cancellable(path, None)
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
    let items = list(data_dir)?;
    for item in items
        .into_iter()
        .filter(|item| !matches!(item.status, RecoveryStatus::Pending))
        .skip(keep)
    {
        let target = recovery_root(data_dir).join(&item.id);
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
