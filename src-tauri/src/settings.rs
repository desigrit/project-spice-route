use crate::error::{Result, SpiceError};
use crate::models::{AppConfig, CloudProvider, LocalState, ProjectMode, SelectionRules, ThemeMode};
use crate::util::{paths_overlap, read_json, write_json};
use globset::Glob;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

pub const CONFIG_SCHEMA: u32 = 1;

pub fn default_config() -> AppConfig {
    let codex_home = discover_codex_home()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".codex"));
    let projectless_root = default_projectless_root();
    AppConfig {
        schema_version: CONFIG_SCHEMA,
        device_id: Uuid::new_v4().to_string(),
        device_name: hostname::get()
            .ok()
            .and_then(|name| name.into_string().ok())
            .unwrap_or_else(|| "This computer".to_string()),
        codex_home: codex_home.to_string_lossy().into_owned(),
        projectless_root: projectless_root.to_string_lossy().into_owned(),
        projects_root: String::new(),
        cloud_root: String::new(),
        cloud_provider: CloudProvider::Custom,
        theme: ThemeMode::System,
        onboarding_complete: false,
        source_roots: HashMap::new(),
        destination_roots: HashMap::new(),
        selection: SelectionRules {
            revision: Uuid::new_v4().to_string(),
            default_project_mode: ProjectMode::Full,
            project_modes: HashMap::new(),
            excluded_thread_ids: Vec::new(),
            include_archived: true,
            include_build_outputs: false,
            include_sensitive_files: true,
            extra_exclude_patterns: Vec::new(),
        },
    }
}

pub fn discover_codex_home() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(value) = env::var_os("CODEX_HOME") {
        candidates.push(PathBuf::from(value));
    }
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".codex"));
    }
    candidates.push(PathBuf::from(r"D:\Codex\.codex"));
    candidates
        .into_iter()
        .find(|path| path.join("state_5.sqlite").is_file() || path.join("sessions").is_dir())
}

fn default_projectless_root() -> PathBuf {
    let known = PathBuf::from(r"D:\Codex\sessions");
    if known.is_dir() {
        known
    } else {
        dirs::document_dir()
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_default())
            .join("Codex Sessions")
    }
}

pub fn default_restore_location(config: &AppConfig) -> PathBuf {
    if !config.projects_root.trim().is_empty() {
        return PathBuf::from(&config.projects_root);
    }
    dirs::document_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default())
        .join("Codex Projects")
}

pub fn source_project_folder(
    config: &AppConfig,
    project_id: &str,
    index: usize,
    discovered: &str,
) -> PathBuf {
    config
        .source_roots
        .get(&format!("{project_id}:{index}"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(discovered))
}

pub fn load_config(data_dir: &Path) -> Result<AppConfig> {
    let path = data_dir.join("config.json");
    if !path.is_file() {
        return Ok(default_config());
    }
    let mut config: AppConfig = read_json(&path)?;
    if config.schema_version != CONFIG_SCHEMA {
        return Err(SpiceError::User(format!(
            "Settings format {} is newer than this app supports.",
            config.schema_version
        )));
    }
    if !config.cloud_root.is_empty() {
        let shared = shared_selection_path(&config);
        if shared.is_file() {
            config.selection = read_json::<SelectionRules>(&shared).map_err(|error| {
                SpiceError::User(format!(
                    "The shared sync selection could not be read from {}: {error}",
                    shared.display()
                ))
            })?;
        }
    }
    Ok(config)
}

pub fn save_config(data_dir: &Path, config: &AppConfig) -> Result<()> {
    validate_config(config)?;
    fs::create_dir_all(data_dir)?;
    if config.onboarding_complete {
        let root = cloud_store_root(config);
        fs::create_dir_all(root.join("objects"))?;
        fs::create_dir_all(root.join("snapshots"))?;
        write_json(&shared_selection_path(config), &config.selection)?;
        let marker = serde_json::json!({ "format": "spice-route", "schemaVersion": 1 });
        write_json(&root.join("format.json"), &marker)?;
    }
    // Only commit the local setting after the shared folder has accepted its
    // marker and selection policy. A failed cloud write must not leave setup
    // looking complete on this device.
    write_json(&data_dir.join("config.json"), config)?;
    Ok(())
}

pub fn validate_config(config: &AppConfig) -> Result<()> {
    if config.schema_version != CONFIG_SCHEMA {
        return Err("Unsupported settings version.".into());
    }
    if config.device_name.trim().is_empty() {
        return Err("Give this device a name.".into());
    }
    if config.onboarding_complete && config.cloud_root.trim().is_empty() {
        return Err("Choose a cloud folder.".into());
    }
    let codex = Path::new(&config.codex_home);
    let cloud = Path::new(&config.cloud_root);
    let projectless = Path::new(&config.projectless_root);
    let projects = Path::new(&config.projects_root);
    if !codex.is_absolute() {
        return Err("Choose an absolute Codex data folder path.".into());
    }
    if !projectless.is_absolute() {
        return Err("Choose an absolute projectless chat workspaces folder path.".into());
    }
    let has_projects = !config.projects_root.trim().is_empty();
    if has_projects && !projects.is_absolute() {
        return Err("Choose an absolute default restore location, or leave it blank.".into());
    }
    if config.onboarding_complete && !cloud.is_absolute() {
        return Err("Choose an absolute cloud folder path.".into());
    }
    if config.onboarding_complete && paths_overlap(codex, cloud) {
        return Err(
            "The cloud folder cannot be inside the Codex data folder (or contain it).".into(),
        );
    }
    if config.onboarding_complete && paths_overlap(projectless, cloud) {
        return Err(
            "The projectless chat workspaces folder must stay outside the cloud transport folder."
                .into(),
        );
    }
    if has_projects && !config.cloud_root.is_empty() && paths_overlap(projects, cloud) {
        return Err(
            "The default restore location must stay outside the cloud transport folder.".into(),
        );
    }
    if paths_overlap(codex, projectless)
        || (has_projects && paths_overlap(codex, projects))
        || (has_projects && paths_overlap(projectless, projects))
    {
        return Err(
            "The task/history, projectless workspace, and project code folders must be separate."
                .into(),
        );
    }
    for (key, value) in config.source_roots.iter().chain(&config.destination_roots) {
        validate_project_folder(Path::new(value), config)?;
        if let (Some(source), Some(destination)) = (
            config.source_roots.get(key),
            config.destination_roots.get(key),
        ) {
            if !same_project_folder(Path::new(source), Path::new(destination)) {
                return Err(format!("Project folder {key} has different Push and Pull locations. Choose one local folder for this project on this device.").into());
            }
        }
    }
    for pattern in &config.selection.extra_exclude_patterns {
        Glob::new(pattern).map_err(|error| {
            SpiceError::User(format!(
                "The exclusion pattern {pattern:?} is invalid: {error}"
            ))
        })?;
    }
    Ok(())
}

pub fn validate_project_folder(path: &Path, config: &AppConfig) -> Result<()> {
    if !path.is_absolute()
        || path.parent().is_none()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(
            "Choose an absolute project folder below a drive root, without '..' components.".into(),
        );
    }
    // Resolve the nearest existing ancestor, including junctions, even when the
    // final restore folder has not been created yet.
    let resolved = resolved_project_folder(path);
    if paths_overlap(&resolved, Path::new(&config.codex_home))
        || paths_overlap(&resolved, Path::new(&config.projectless_root))
        || (!config.cloud_root.trim().is_empty()
            && paths_overlap(&resolved, Path::new(&config.cloud_root)))
    {
        return Err(format!("Project folder {} must stay separate from Codex task/history, projectless workspaces, and the cloud folder.", path.display()).into());
    }
    if path.is_file() {
        return Err(format!("Choose a folder, not a file: {}", path.display()).into());
    }
    Ok(())
}

pub fn resolved_project_folder(path: &Path) -> PathBuf {
    if let Ok(resolved) = dunce::canonicalize(path) {
        return resolved;
    }
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => resolved_project_folder(parent).join(name),
        _ => path.to_path_buf(),
    }
}

pub fn same_project_folder(left: &Path, right: &Path) -> bool {
    let left = resolved_project_folder(left);
    let right = resolved_project_folder(right);
    #[cfg(windows)]
    {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

pub fn cloud_store_root(config: &AppConfig) -> PathBuf {
    Path::new(&config.cloud_root).join(".spice-route")
}

pub fn shared_selection_path(config: &AppConfig) -> PathBuf {
    cloud_store_root(config).join("selection.json")
}

pub fn load_local_state(data_dir: &Path) -> Result<LocalState> {
    let path = data_dir.join("state.json");
    if path.is_file() {
        read_json(&path)
    } else {
        Ok(LocalState::default())
    }
}

pub fn save_local_state(data_dir: &Path, state: &LocalState) -> Result<()> {
    write_json(&data_dir.join("state.json"), state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_settings_keep_restore_location_optional() {
        let data_dir = tempfile::tempdir().unwrap();
        let mut value = serde_json::to_value(default_config()).unwrap();
        value.as_object_mut().unwrap().remove("projectsRoot");
        value.as_object_mut().unwrap().remove("sourceRoots");
        fs::write(
            data_dir.path().join("config.json"),
            serde_json::to_vec_pretty(&value).unwrap(),
        )
        .unwrap();

        let loaded = load_config(data_dir.path()).unwrap();
        assert!(loaded.projects_root.is_empty());
        assert!(loaded.source_roots.is_empty());
        assert!(default_restore_location(&loaded).is_absolute());
        assert_ne!(
            default_restore_location(&loaded),
            Path::new(r"D:\Codex\projects")
        );
        validate_config(&loaded).unwrap();
    }

    #[test]
    fn local_content_roots_must_be_separate() {
        let data_dir = tempfile::tempdir().unwrap();
        let mut config = default_config();
        config.codex_home = data_dir
            .path()
            .join(".codex")
            .to_string_lossy()
            .into_owned();
        config.projectless_root = data_dir
            .path()
            .join("sessions")
            .to_string_lossy()
            .into_owned();
        config.projects_root = data_dir
            .path()
            .join("projects")
            .to_string_lossy()
            .into_owned();
        validate_config(&config).unwrap();

        config.projects_root = data_dir.path().to_string_lossy().into_owned();
        let error = validate_config(&config).unwrap_err().to_string();
        assert!(error.contains("must be separate"));
    }

    #[test]
    fn project_mappings_cannot_escape_into_cloud_or_local_records() {
        let root = tempfile::tempdir().unwrap();
        let mut config = default_config();
        config.codex_home = root.path().join("records").to_string_lossy().into_owned();
        config.projectless_root = root.path().join("sessions").to_string_lossy().into_owned();
        config.cloud_root = root.path().join("cloud").to_string_lossy().into_owned();
        for invalid in [
            root.path().join("records/project"),
            root.path().join("cloud/code"),
            root.path().join("sessions/task"),
            root.path().join("safe/../cloud/code"),
        ] {
            config
                .destination_roots
                .insert("a:0".into(), invalid.to_string_lossy().into_owned());
            assert!(validate_config(&config).is_err());
        }
        config.destination_roots.insert(
            "a:0".into(),
            root.path()
                .join("work/project-a")
                .to_string_lossy()
                .into_owned(),
        );
        validate_config(&config).unwrap();
        config.source_roots.insert(
            "a:0".into(),
            root.path()
                .join("other/project-a")
                .to_string_lossy()
                .into_owned(),
        );
        assert!(validate_config(&config)
            .unwrap_err()
            .to_string()
            .contains("different Push and Pull"));
    }
}
