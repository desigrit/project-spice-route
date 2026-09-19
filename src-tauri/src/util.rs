use crate::error::{Result, SpiceError};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

pub fn sha256_reader(mut reader: impl Read) -> Result<(String, u64)> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    let mut size = 0_u64;
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
        size += count as u64;
    }
    Ok((format!("{:x}", hasher.finalize()), size))
}

pub fn sha256_file(path: &Path) -> Result<(String, u64)> {
    sha256_reader(File::open(path)?)
}

pub fn hash_json(value: &impl Serialize) -> Result<String> {
    Ok(sha256_bytes(&serde_json::to_vec(value)?))
}

pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let file = File::open(path)?;
    // serde_json reads individual bytes from its Read implementation. Buffering
    // is essential for multi-megabyte manifests stored in a cloud-client folder.
    Ok(serde_json::from_reader(BufReader::with_capacity(
        256 * 1024,
        file,
    ))?)
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| SpiceError::User(format!("Path has no parent: {}", path.display())))?;
    fs::create_dir_all(parent)?;
    let partial = parent.join(format!(
        ".{}.{}.partial",
        path.file_name().and_then(|v| v.to_str()).unwrap_or("write"),
        Uuid::new_v4()
    ));
    {
        let mut file = File::create(&partial)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    if path.exists() {
        let backup = parent.join(format!(
            ".{}.previous",
            path.file_name().and_then(|v| v.to_str()).unwrap_or("write")
        ));
        let _ = fs::remove_file(&backup);
        fs::rename(path, &backup)?;
        if let Err(error) = fs::rename(&partial, path) {
            let _ = fs::rename(&backup, path);
            return Err(error.into());
        }
        let _ = fs::remove_file(backup);
    } else {
        fs::rename(&partial, path)?;
    }
    Ok(())
}

pub fn replace_file(source: &Path, destination: &Path) -> Result<()> {
    let parent = destination.parent().ok_or_else(|| {
        SpiceError::User(format!("Path has no parent: {}", destination.display()))
    })?;
    fs::create_dir_all(parent)?;
    let partial = parent.join(format!(
        ".{}.{}.partial",
        destination
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("copy"),
        Uuid::new_v4()
    ));
    {
        let mut input = File::open(source)?;
        let mut output = File::create(&partial)?;
        std::io::copy(&mut input, &mut output)?;
        output.sync_all()?;
    }
    if destination.exists() {
        fs::remove_file(destination)?;
    }
    fs::rename(partial, destination)?;
    Ok(())
}

pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    atomic_write(path, &serde_json::to_vec_pretty(value)?)
}

pub fn safe_relative(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        return Err(SpiceError::User(format!(
            "Absolute path is not allowed in a snapshot: {}",
            path.display()
        )));
    }
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => result.push(value),
            Component::CurDir => {}
            _ => {
                return Err(SpiceError::User(format!(
                    "Unsafe relative path in snapshot: {}",
                    path.display()
                )))
            }
        }
    }
    Ok(result)
}

pub fn directory_size(path: &Path) -> u64 {
    walkdir::WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.metadata().ok())
        .filter(|meta| meta.is_file())
        .map(|meta| meta.len())
        .sum()
}

pub fn paths_overlap(a: &Path, b: &Path) -> bool {
    let a = resolve_for_overlap(a);
    let b = resolve_for_overlap(b);
    path_starts_with(&a, &b) || path_starts_with(&b, &a)
}

/// Resolve links in the nearest existing ancestor while retaining any missing
/// suffix. This keeps comparisons consistent when a platform aliases a common
/// root, such as `/var` pointing at `/private/var` on macOS, and the project
/// folder being validated has not been created yet.
fn resolve_for_overlap(path: &Path) -> PathBuf {
    for ancestor in path.ancestors() {
        if let Ok(mut resolved) = dunce::canonicalize(ancestor) {
            if let Ok(suffix) = path.strip_prefix(ancestor) {
                for component in suffix.components() {
                    match component {
                        Component::Normal(_) => resolved.push(component.as_os_str()),
                        Component::CurDir => {}
                        Component::ParentDir => {
                            resolved.pop();
                        }
                        Component::Prefix(_) | Component::RootDir => {}
                    }
                }
                return resolved;
            }
        }
    }
    lexically_normalized(path)
}

fn lexically_normalized(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                result.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => match result.components().next_back() {
                Some(Component::Normal(_)) => {
                    result.pop();
                }
                Some(Component::ParentDir) | None if !result.has_root() => result.push(".."),
                _ => {}
            },
        }
    }
    result
}

fn path_starts_with(path: &Path, base: &Path) -> bool {
    #[cfg(windows)]
    {
        let path: Vec<_> = path.components().collect();
        let base: Vec<_> = base.components().collect();
        base.len() <= path.len()
            && base.iter().zip(path.iter()).all(|(left, right)| {
                left.as_os_str()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&right.as_os_str().to_string_lossy())
            })
    }
    #[cfg(not(windows))]
    {
        path.starts_with(base)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_parent_components() {
        assert!(safe_relative(Path::new("ok/nested.txt")).is_ok());
        assert!(safe_relative(Path::new("../escape.txt")).is_err());
    }

    #[test]
    fn hashes_are_stable() {
        assert_eq!(
            sha256_bytes(b"spice"),
            "58e989955f4b358feb6e4580eff3c7a04f5d9fa2d8381a6479cdfffa4cbe1211"
        );
    }

    #[cfg(windows)]
    #[test]
    fn overlap_checks_are_case_insensitive_on_windows() {
        assert!(paths_overlap(
            Path::new(r"C:\Users\Example\Cloud"),
            Path::new(r"c:\users\example\cloud\project")
        ));
        assert!(!paths_overlap(
            Path::new(r"C:\Users\Example\Cloud"),
            Path::new(r"C:\Users\Example\Cloudy")
        ));
    }

    #[cfg(unix)]
    #[test]
    fn overlap_resolves_links_before_appending_missing_folders() {
        use std::os::unix::fs::symlink;

        let fixture = tempfile::tempdir().unwrap();
        let real = fixture.path().join("real");
        let nested = real.join("nested");
        let alias = fixture.path().join("alias");
        fs::create_dir_all(&nested).unwrap();
        symlink(&nested, &alias).unwrap();

        assert!(paths_overlap(&real, &alias.join("missing/project")));
        assert!(!paths_overlap(&real.join("first"), &alias.join("second")));
        assert!(paths_overlap(
            &real.join("sibling"),
            &alias.join("../sibling")
        ));
    }
}
