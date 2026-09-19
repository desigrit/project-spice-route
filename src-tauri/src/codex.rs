use crate::error::{Result, SpiceError};
use crate::models::{
    AppConfig, AttachmentReference, CompatibilityInfo, ContentCatalog, DatabaseRow, ProjectExport,
    ProjectMode, ProjectSummary, SelectionRules, SnapshotManifest, SqlValue, ThreadExport,
    ThreadSummary,
};
use crate::util::{hash_json, write_json};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use rusqlite::backup::Backup;
use rusqlite::types::{Value, ValueRef};
use rusqlite::{params_from_iter, Connection, OpenFlags, OptionalExtension};
use serde_json::{Map, Value as JsonValue};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const ADAPTER_NAME: &str = "codex-state-v5.52-54-55/history-v1.6";
pub const SUPPORTED_CODEX_BUILDS: &[&str] = &["0.153.4", "0.154.0-alpha.6.2", "0.155.0-alpha.9.2"];
const LOCAL_ONLY_THREAD_COLUMNS: &[&str] = &["sandbox_policy", "approval_mode", "agent_path"];
const SAFE_IMPORTED_SANDBOX_POLICY: &str =
    r#"{"type":"managed","file_system":{"type":"restricted","entries":[]},"network":"restricted"}"#;
const SAFE_IMPORTED_APPROVAL_MODE: &str = "on-request";
const DISPLAY_TITLE_LIMIT: usize = 160;
const DISPLAY_PREVIEW_LIMIT: usize = 320;

const STATE_THREAD_TABLES: &[(&str, &str)] = &[
    ("threads", "id"),
    ("thread_artifacts", "thread_id"),
    ("thread_dynamic_tools", "thread_id"),
    ("thread_spawn_edges", "child_thread_id"),
];
const HISTORY_THREAD_TABLES: &[(&str, &str)] = &[
    ("thread_items", "thread_id"),
    ("thread_realtime_items", "thread_id"),
    ("thread_turns", "thread_id"),
    ("thread_history_projection_state", "thread_id"),
];

// Keep portable rows in the legacy artifact representation so an unchanged chat
// has the same fingerprint before and after Codex's schema 55 rename.
fn artifact_table(state_migration: Option<i64>) -> &'static str {
    match state_migration {
        Some(55) => "thread_attachments",
        _ => "thread_artifacts",
    }
}

fn state_storage_table(table: &str, state_migration: Option<i64>) -> &str {
    if table == "thread_artifacts" {
        artifact_table(state_migration)
    } else {
        table
    }
}

fn rename_artifact_type(rows: &mut [DatabaseRow], from: &str, to: &str) -> Result<()> {
    for row in rows {
        if row.values.contains_key(to) {
            return Err(SpiceError::UnsupportedCodex(format!(
                "The attachment row contains an unexpected {to} column. Nothing was discarded."
            )));
        }
        if let Some(value) = row.values.remove(from) {
            row.values.insert(to.to_string(), value);
        }
    }
    Ok(())
}

fn state_rows_for_storage<'a>(
    table: &str,
    rows: &'a [DatabaseRow],
    state_migration: Option<i64>,
) -> Result<Cow<'a, [DatabaseRow]>> {
    if table != "thread_artifacts" || state_migration != Some(55) {
        return Ok(Cow::Borrowed(rows));
    }
    let mut rows = rows.to_vec();
    rename_artifact_type(&mut rows, "artifact_type", "attachment_type")?;
    Ok(Cow::Owned(rows))
}

#[derive(Clone, Debug)]
pub struct PendingFile {
    pub logical_path: String,
    pub owner_id: String,
    pub source: PathBuf,
    pub kind: PendingFileKind,
}

#[derive(Clone, Debug)]
pub enum PendingFileKind {
    Rollout,
    ProjectlessRoot,
    Attachment,
}

#[derive(Clone, Debug)]
pub struct CodexExport {
    pub compatibility: CompatibilityInfo,
    pub threads: Vec<ThreadExport>,
    pub projects: Vec<ProjectExport>,
    pub pending_files: Vec<PendingFile>,
    pub ui_state: JsonValue,
    pub session_index_lines: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Default)]
struct UiAssociations {
    projectless: HashSet<String>,
    assignments: HashMap<String, String>,
    projectless_outputs: HashMap<String, String>,
    legacy_by_project: HashMap<String, String>,
    raw: JsonValue,
}

pub fn inspect(home: &Path) -> Result<CompatibilityInfo> {
    let state_path = home.join("state_5.sqlite");
    let history_path = home.join("thread_history_1.sqlite");
    if !state_path.is_file() || !history_path.is_file() {
        return Ok(CompatibilityInfo {
            supported: false,
            adapter: ADAPTER_NAME.to_string(),
            state_migration: None,
            history_migration: None,
            schema_fingerprint: String::new(),
            explanation: "Codex has not created both thread databases yet. Open and fully quit Codex once, then refresh.".to_string(),
        });
    }
    let state = open_read_only(&state_path)?;
    let history = open_read_only(&history_path)?;
    let state_migration = migration_version(&state);
    let history_migration = migration_version(&history);
    let state_schema = schema_signature(&state)?;
    let history_schema = schema_signature(&history)?;
    let fingerprint =
        crate::util::sha256_bytes(format!("{state_schema}\n{history_schema}").as_bytes());
    let required_state = [
        "threads",
        "projects",
        "project_roots",
        "thread_sections",
        artifact_table(state_migration),
        "thread_dynamic_tools",
        "thread_spawn_edges",
    ];
    let required_history = [
        "thread_items",
        "thread_realtime_items",
        "thread_turns",
        "thread_history_projection_state",
    ];
    let missing: Vec<_> = required_state
        .iter()
        .filter(|name| !table_exists(&state, name))
        .chain(
            required_history
                .iter()
                .filter(|name| !table_exists(&history, name)),
        )
        .copied()
        .collect();
    let profile = crate::compatibility::profile(&CompatibilityInfo {
        supported: false,
        adapter: ADAPTER_NAME.to_string(),
        state_migration,
        history_migration,
        schema_fingerprint: fingerprint.clone(),
        explanation: String::new(),
    });
    let supported = if let Some(profile) = profile {
        missing.is_empty()
            && complete_schema_signature(&state)?
                == crate::compatibility::expected_layout(profile, "state_5.sqlite")
            && complete_schema_signature(&history)?
                == crate::compatibility::expected_layout(profile, "thread_history_1.sqlite")
            && migrations_succeeded(&state)?
            && migrations_succeeded(&history)?
    } else {
        false
    };
    let explanation = if supported {
        format!("Database schema {}/6 matches a tested storage profile, including indexes and triggers.", state_migration.unwrap_or_default())
    } else if !crate::compatibility::PROFILES
        .iter()
        .any(|p| Some(p.state) == state_migration)
        || history_migration != Some(6)
    {
        format!("Found database migrations {}/{}; tested profiles are 52/6, 54/6, and 55/6. Update Spice Route for this Codex format. Push and Pull remain blocked.", state_migration.map(|v| v.to_string()).unwrap_or_else(|| "unknown".into()), history_migration.map(|v| v.to_string()).unwrap_or_else(|| "unknown".into()))
    } else if !missing.is_empty() {
        format!("Required tables are missing: {}. Diagnostics are available, but Push and Pull are blocked.", missing.join(", "))
    } else {
        format!("Database migrations match, but the layout, triggers, or migration completion do not match a tested profile (fingerprint {fingerprint}). Export a compatibility report for an adapter update. Push and Pull remain blocked.")
    };
    Ok(CompatibilityInfo {
        supported,
        adapter: ADAPTER_NAME.to_string(),
        state_migration,
        history_migration,
        schema_fingerprint: fingerprint,
        explanation,
    })
}

pub fn with_build_gate(
    mut compatibility: CompatibilityInfo,
    version: Option<&str>,
) -> CompatibilityInfo {
    if !compatibility.supported {
        return compatibility;
    }
    let build = version.and_then(|value| {
        value
            .split(|character: char| {
                character.is_whitespace() || matches!(character, '/' | '\\' | '(' | ')')
            })
            .find(|part| {
                part.chars()
                    .next()
                    .is_some_and(|first| first.is_ascii_digit())
            })
    });
    let profile = crate::compatibility::profile(&compatibility);
    if profile.is_some_and(|profile| Some(profile.runtime) == build) {
        compatibility.explanation = format!(
            "{} Codex build {} is also validated.",
            compatibility.explanation,
            build.unwrap_or_default()
        );
    } else {
        compatibility.supported = false;
        compatibility.explanation = match version {
            Some(version) => format!(
                "Runtime {version} and database schema {}/{} are not a tested pair. Tested pairs: 0.153.4 with 52/6; 0.154.0-alpha.6.2 with 54/6; 0.155.0-alpha.9.2 with 55/6. Update Spice Route, or finish updating and restart Codex if an update is pending.",
                compatibility.state_migration.unwrap_or_default(), compatibility.history_migration.unwrap_or_default()
            ),
            None => format!(
                "The Codex build could not be detected. This release enables Push and Pull only for validated build {}.",
                SUPPORTED_CODEX_BUILDS.join(", ")
            ),
        };
    }
    compatibility
}

/// Re-evaluate source metadata locally; a manifest's `supported` flag is not authority.
pub fn validate_snapshot_source(manifest: &SnapshotManifest) -> Result<()> {
    let mut info = manifest.compatibility.clone();
    info.supported = crate::compatibility::profile(&info).is_some();
    if !info.supported {
        return Err(SpiceError::UnsupportedCodex("The snapshot's database profile is unknown to this Spice Route release. Update Spice Route before importing it.".into()));
    }
    let info = with_build_gate(info, manifest.codex_version.as_deref());
    if !info.supported {
        return Err(SpiceError::UnsupportedCodex(info.explanation));
    }
    Ok(())
}

/// An older database cannot represent newer fields with values. Never discard them.
pub fn transfer_issues(
    manifest: &SnapshotManifest,
    destination: &CompatibilityInfo,
) -> Vec<String> {
    if destination.state_migration != Some(52) {
        return Vec::new();
    }
    manifest.threads.iter().filter_map(|thread| {
        let fields: Vec<_> = ["originator", "daybreak_enabled"].into_iter().filter(|field| {
            thread.state_rows.get("threads").into_iter().flatten().any(|row|
                row.values.get(*field).is_some_and(|value| !matches!(value, SqlValue::Null)))
        }).collect();
        (!fields.is_empty()).then(|| format!("Chat ‘{}’ contains newer Codex fields ({}). This destination's schema 52 cannot preserve them. Update Codex on this PC to a supported 54/6 or 55/6 build, or exclude this chat and Push again from the source. No data has been changed.", thread.title, fields.join(", ")))
    }).collect()
}

pub fn snapshot_databases(home: &Path, destination: &Path) -> Result<(PathBuf, PathBuf)> {
    fs::create_dir_all(destination)?;
    let state_out = destination.join("state_5.sqlite");
    let history_out = destination.join("thread_history_1.sqlite");
    sqlite_backup(&home.join("state_5.sqlite"), &state_out)?;
    sqlite_backup(&home.join("thread_history_1.sqlite"), &history_out)?;
    Ok((state_out, history_out))
}

pub fn verify_databases(home: &Path) -> Result<()> {
    for name in ["state_5.sqlite", "thread_history_1.sqlite"] {
        let path = home.join(name);
        let connection = open_read_only(&path)?;
        let result: String = connection.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
        if result != "ok" {
            return Err(SpiceError::User(format!(
                "Codex database verification failed for {}: {result}",
                path.display()
            )));
        }
    }
    let compatibility = inspect(home)?;
    if !compatibility.supported {
        return Err(SpiceError::UnsupportedCodex(compatibility.explanation));
    }
    Ok(())
}

fn sqlite_backup(source: &Path, destination: &Path) -> Result<()> {
    let source_db = open_read_only(source)?;
    let mut destination_db = Connection::open(destination)?;
    let backup = Backup::new(&source_db, &mut destination_db)?;
    backup.run_to_completion(64, Duration::from_millis(10), None)?;
    drop(backup);
    destination_db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    Ok(())
}

fn open_read_only(path: &Path) -> Result<Connection> {
    Ok(Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?)
}

fn migration_version(connection: &Connection) -> Option<i64> {
    connection
        .query_row(
            "SELECT max(version) FROM _sqlx_migrations WHERE success = 1",
            [],
            |row| row.get(0),
        )
        .ok()
        .flatten()
}

fn table_exists(connection: &Connection, name: &str) -> bool {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
            [name],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        == 1
}

fn schema_signature(connection: &Connection) -> Result<String> {
    let mut statement = connection.prepare("SELECT name, coalesce(sql, '') FROM sqlite_master WHERE type IN ('table','index') AND name NOT LIKE 'sqlite_%' ORDER BY type, name")?;
    let rows = statement.query_map([], |row| {
        Ok(format!(
            "{}:{}",
            row.get::<_, String>(0)?,
            crate::compatibility::normalize_schema_sql(&row.get::<_, String>(1)?)
        ))
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?.join("\n"))
}

fn complete_schema_signature(connection: &Connection) -> Result<String> {
    let mut statement = connection.prepare("SELECT type, name, coalesce(sql, '') FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name")?;
    let rows = statement.query_map([], |row| {
        Ok(format!(
            "{}:{}:{}",
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            crate::compatibility::normalize_schema_sql(&row.get::<_, String>(2)?)
        ))
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?.join("\n"))
}

fn migrations_succeeded(connection: &Connection) -> Result<bool> {
    Ok(connection.query_row(
        "SELECT count(*) = 0 FROM _sqlx_migrations WHERE success IS NOT 1",
        [],
        |row| row.get(0),
    )?)
}

pub fn list_content(home: &Path) -> Result<ContentCatalog> {
    let compatibility = inspect(home)?;
    let state = open_read_only(&home.join("state_5.sqlite"))?;
    let associations = read_ui_associations(home);
    let history_sizes = history_sizes(home).unwrap_or_default();
    let mut threads = Vec::new();
    if table_exists(&state, "threads") {
        let columns = table_columns(&state, "threads")?;
        let has = |name: &str| columns.iter().any(|column| column == name);
        if has("id") && has("cwd") {
            let title_expr = if has("name") {
                "coalesce(nullif(trim(name), ''), nullif(trim(title), ''), 'Untitled chat')"
            } else {
                "coalesce(nullif(trim(title), ''), 'Untitled chat')"
            };
            let preview_expr = if has("preview") {
                "coalesce(preview, first_user_message, '')"
            } else if has("first_user_message") {
                "coalesce(first_user_message, '')"
            } else {
                "''"
            };
            let project_expr = if has("project_id") {
                "project_id"
            } else {
                "NULL"
            };
            let archived_expr = if has("archived") { "archived" } else { "0" };
            let updated_expr = if has("updated_at_ms") {
                "coalesce(updated_at_ms, updated_at * 1000, 0)"
            } else if has("updated_at") {
                "updated_at * 1000"
            } else {
                "0"
            };
            let rollout_expr = if has("rollout_path") {
                "rollout_path"
            } else {
                "''"
            };
            let sql = format!("SELECT id, {title_expr}, {preview_expr}, cwd, {project_expr}, {archived_expr}, {updated_expr}, {rollout_expr} FROM threads ORDER BY {updated_expr} DESC");
            let mut statement = state.prepare(&sql)?;
            let mapped = statement.query_map([], |row| {
                let id: String = row.get(0)?;
                let db_project: Option<String> = row.get(4)?;
                let project_id = db_project.or_else(|| associations.assignments.get(&id).cloned());
                let rollout: String = row.get(7)?;
                let rollout_size = fs::metadata(&rollout).map(|meta| meta.len()).unwrap_or(0);
                Ok(ThreadSummary {
                    projectless: project_id.is_none(),
                    estimated_bytes: rollout_size + history_sizes.get(&id).copied().unwrap_or(0),
                    id,
                    title: display_thread_title(&row.get::<_, String>(1)?),
                    preview: display_label(&row.get::<_, String>(2)?, DISPLAY_PREVIEW_LIMIT),
                    cwd: row.get(3)?,
                    project_id,
                    archived: row.get::<_, i64>(5)? != 0,
                    updated_at_ms: row.get(6)?,
                })
            })?;
            threads = mapped.collect::<std::result::Result<Vec<_>, _>>()?;
        }
    }
    omit_internal_threads(&state, &mut threads)?;
    inherit_parent_projects(&state, &associations, &mut threads)?;
    let mut projects = read_projects(&state, &threads)?;
    for project in &mut projects {
        // The engine estimates mapped workspaces using the active file exclusions.
        // Catalog discovery must not recursively scan the original roots (including
        // dependency/build trees) before those mappings and rules have been applied.
        project.git_repository = project
            .roots
            .iter()
            .any(|root| Path::new(root).join(".git").exists());
        project.linked_worktree = project
            .roots
            .iter()
            .any(|root| Path::new(root).join(".git").is_file());
    }
    let total_estimated_bytes = threads
        .iter()
        .map(|thread| thread.estimated_bytes)
        .sum::<u64>()
        + projects
            .iter()
            .map(|project| project.estimated_bytes)
            .sum::<u64>();
    let mut warnings = vec![
        "Task sandbox policies, approval settings, and agent paths remain local to each device."
            .to_string(),
    ];
    if !compatibility.supported {
        warnings.push(compatibility.explanation);
    }
    Ok(ContentCatalog {
        threads,
        projects,
        total_estimated_bytes,
        warnings,
    })
}

/// These are presentation labels only. Database names, titles, and history are
/// exported unchanged, even when Codex used an entire first message as a title.
pub fn display_thread_title(value: &str) -> String {
    let title = display_label(value, DISPLAY_TITLE_LIMIT);
    if title.is_empty() {
        "Untitled chat".into()
    } else {
        title
    }
}

fn display_label(value: &str, limit: usize) -> String {
    let mut label = String::new();
    let mut characters = 0;
    let mut preceding_space = true;
    for character in value.chars() {
        // Bidi formatting controls should not reorder a review label. Historical
        // text is left intact in the exported database rows.
        if matches!(character, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}')
        {
            continue;
        }
        let character = if character.is_whitespace() || character.is_control() {
            ' '
        } else if character == '\u{2014}' {
            '-'
        } else {
            character
        };
        if character == ' ' && preceding_space {
            continue;
        }
        if characters == limit {
            label.pop();
            label.push('…');
            break;
        }
        label.push(character);
        characters += 1;
        preceding_space = character == ' ';
    }
    label.trim_end().to_string()
}

fn is_internal_source(source: &str) -> bool {
    serde_json::from_str::<JsonValue>(source)
        .ok()
        .and_then(|value| {
            value
                .pointer("/subagent/other")
                .and_then(JsonValue::as_str)
                .map(|kind| kind == "guardian")
        })
        .unwrap_or(false)
}

/// Codex's guardian records are internal approval assessments, not user tasks.
/// Match explicit source metadata only, never conversation text or task names.
pub fn is_internal_thread(thread: &ThreadExport) -> bool {
    thread
        .state_rows
        .get("threads")
        .into_iter()
        .flatten()
        .any(|row| row_string(row, "source").is_some_and(is_internal_source))
}

fn omit_internal_threads(state: &Connection, threads: &mut Vec<ThreadSummary>) -> Result<()> {
    if !table_exists(state, "threads")
        || !table_columns(state, "threads")?
            .iter()
            .any(|column| column == "source")
    {
        return Ok(());
    }
    let mut statement =
        state.prepare("SELECT id, source FROM threads WHERE source LIKE '%guardian%'")?;
    let mut internal = HashSet::new();
    for result in statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })? {
        let (id, source) = result?;
        if is_internal_source(&source) {
            internal.insert(id);
        }
    }
    let parents = thread_parents(state)?;
    loop {
        let children: Vec<_> = parents
            .iter()
            .filter(|(child, parent)| !internal.contains(*child) && internal.contains(*parent))
            .map(|(child, _)| child.clone())
            .collect();
        if children.is_empty() {
            break;
        }
        internal.extend(children);
    }
    threads.retain(|thread| !internal.contains(&thread.id));
    Ok(())
}

fn history_sizes(home: &Path) -> Result<HashMap<String, u64>> {
    let connection = open_read_only(&home.join("thread_history_1.sqlite"))?;
    if !table_exists(&connection, "thread_items") {
        return Ok(HashMap::new());
    }
    let mut statement = connection
        .prepare("SELECT thread_id, sum(length(CAST(item_json AS BLOB))) FROM thread_items GROUP BY thread_id")?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<i64>>(1)?.unwrap_or(0).max(0) as u64,
        ))
    })?;
    Ok(rows.collect::<std::result::Result<HashMap<_, _>, _>>()?)
}

fn read_projects(state: &Connection, threads: &[ThreadSummary]) -> Result<Vec<ProjectSummary>> {
    if !table_exists(state, "projects") {
        return Ok(Vec::new());
    }
    let mut roots: HashMap<String, Vec<(i64, String)>> = HashMap::new();
    if table_exists(state, "project_roots") {
        let mut statement = state.prepare(
            "SELECT project_id, position, path FROM project_roots ORDER BY project_id, position",
        )?;
        for result in statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })? {
            let (id, position, path) = result?;
            roots.entry(id).or_default().push((position, path));
        }
    }
    let counts: HashMap<String, usize> = threads
        .iter()
        .filter_map(|thread| thread.project_id.clone())
        .fold(HashMap::new(), |mut map, id| {
            *map.entry(id).or_default() += 1;
            map
        });
    let mut statement = state.prepare("SELECT id, name FROM projects ORDER BY position, name")?;
    let rows = statement.query_map([], |row| {
        let id: String = row.get(0)?;
        let mut project_roots = roots.remove(&id).unwrap_or_default();
        project_roots.sort_by_key(|item| item.0);
        Ok(ProjectSummary {
            name: row.get(1)?,
            thread_count: counts.get(&id).copied().unwrap_or(0),
            roots: project_roots.iter().map(|item| item.1.clone()).collect(),
            local_roots: project_roots.into_iter().map(|item| item.1).collect(),
            id,
            estimated_bytes: 0,
            git_repository: false,
            linked_worktree: false,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

fn read_ui_associations(home: &Path) -> UiAssociations {
    let path = home.join(".codex-global-state.json");
    let raw: JsonValue = fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_else(|| JsonValue::Object(Map::new()));
    let projectless = raw
        .get("projectless-thread-ids")
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
        .filter_map(JsonValue::as_str)
        .map(str::to_string)
        .collect();
    let mut legacy_by_project = HashMap::new();
    let mut project_by_legacy = HashMap::new();
    if let Some(hosts) = raw
        .get("app-server-project-id-by-legacy-project-id-by-host")
        .and_then(JsonValue::as_object)
    {
        for mapping in hosts.values().filter_map(JsonValue::as_object) {
            for (legacy_id, project_id) in mapping
                .iter()
                .filter_map(|(legacy, value)| value.as_str().map(|project| (legacy, project)))
            {
                legacy_by_project
                    .entry(project_id.to_string())
                    .or_insert_with(|| legacy_id.clone());
                project_by_legacy
                    .entry(legacy_id.clone())
                    .or_insert_with(|| project_id.to_string());
            }
        }
    }
    let assignments = raw
        .get("thread-project-assignments")
        .and_then(JsonValue::as_object)
        .into_iter()
        .flat_map(|object| object.iter())
        .filter_map(|(id, value)| {
            value
                .as_str()
                .or_else(|| value.get("projectId").and_then(JsonValue::as_str))
                .map(|project| {
                    let project = project_by_legacy
                        .get(project)
                        .cloned()
                        .unwrap_or_else(|| project.to_string());
                    (id.clone(), project)
                })
        })
        .collect();
    let projectless_outputs = raw
        .get("thread-projectless-output-directories")
        .and_then(JsonValue::as_object)
        .into_iter()
        .flat_map(|object| object.iter())
        .filter_map(|(id, value)| value.as_str().map(|path| (id.clone(), path.to_string())))
        .collect();
    UiAssociations {
        projectless,
        assignments,
        projectless_outputs,
        legacy_by_project,
        raw,
    }
}

// Spawned agents do not necessarily have their own project_id or sidebar assignment.
// Their explicit parent relationship, rather than their cwd, determines which project
// governs their history and files. Do not move deliberately projectless tasks into a project.
fn inherit_parent_projects(
    state: &Connection,
    associations: &UiAssociations,
    threads: &mut [ThreadSummary],
) -> Result<()> {
    let parents = thread_parents(state)?;
    let mut projects: HashMap<String, String> = threads
        .iter()
        .filter_map(|thread| {
            thread
                .project_id
                .clone()
                .map(|project| (thread.id.clone(), project))
        })
        .collect();
    // Bounded fixed point also handles nested agents and malformed relationship cycles.
    for _ in 0..threads.len() {
        let mut changed = false;
        for thread in threads.iter_mut() {
            if thread.project_id.is_some() || associations.projectless.contains(&thread.id) {
                continue;
            }
            if let Some(project) = parents
                .get(&thread.id)
                .and_then(|parent| projects.get(parent))
                .cloned()
            {
                projects.insert(thread.id.clone(), project.clone());
                thread.project_id = Some(project);
                thread.projectless = false;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    Ok(())
}

fn thread_parents(state: &Connection) -> Result<HashMap<String, String>> {
    let mut parents = HashMap::new();
    if table_exists(state, "threads")
        && table_columns(state, "threads")?
            .iter()
            .any(|column| column == "source")
    {
        let mut statement =
            state.prepare("SELECT id, source FROM threads WHERE source LIKE '%subagent%'")?;
        for result in statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })? {
            let (id, source) = result?;
            if let Some(parent) =
                serde_json::from_str::<JsonValue>(&source)
                    .ok()
                    .and_then(|source| {
                        source
                            .pointer("/subagent/thread_spawn/parent_thread_id")
                            .and_then(JsonValue::as_str)
                            .map(str::to_string)
                    })
            {
                parents.insert(id, parent);
            }
        }
    }
    if table_exists(state, "thread_spawn_edges") {
        let mut statement =
            state.prepare("SELECT child_thread_id, parent_thread_id FROM thread_spawn_edges")?;
        for result in statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })? {
            let (child, parent) = result?;
            parents.insert(child, parent);
        }
    }
    Ok(parents)
}

/// Effective exclusions for comparison with a previous snapshot. Derive these from
/// its relationships rather than interpreting absent, excluded agent histories as
/// source deletions. These are derived choices, not new individual user exclusions.
pub fn selection_excluded_thread_ids(
    selection: &SelectionRules,
    threads: &[ThreadExport],
) -> HashSet<String> {
    let mut excluded: HashSet<_> = selection.excluded_thread_ids.iter().cloned().collect();
    let mut parents = HashMap::new();
    for thread in threads {
        if is_internal_thread(thread)
            || (thread.archived && !selection.include_archived)
            || thread
                .project_id
                .as_deref()
                .is_some_and(|id| project_mode(selection, id) == ProjectMode::Excluded)
        {
            excluded.insert(thread.id.clone());
        }
        let source_parent = thread
            .state_rows
            .get("threads")
            .and_then(|rows| rows.first())
            .and_then(|row| row_string(row, "source"))
            .and_then(|source| serde_json::from_str::<JsonValue>(source).ok())
            .and_then(|source| {
                source
                    .pointer("/subagent/thread_spawn/parent_thread_id")
                    .and_then(JsonValue::as_str)
                    .map(str::to_string)
            });
        let edge_parent = thread
            .state_rows
            .get("thread_spawn_edges")
            .into_iter()
            .flatten()
            .find(|row| row_string(row, "child_thread_id") == Some(thread.id.as_str()))
            .and_then(|row| row_string(row, "parent_thread_id"))
            .map(str::to_string);
        if let Some(parent) = edge_parent.or(source_parent) {
            parents.insert(thread.id.clone(), parent);
        }
    }
    loop {
        let descendants: Vec<_> = parents
            .iter()
            .filter(|(child, parent)| !excluded.contains(*child) && excluded.contains(*parent))
            .map(|(child, _)| child.clone())
            .collect();
        if descendants.is_empty() {
            break;
        }
        excluded.extend(descendants);
    }
    excluded
}

pub fn export_selected(
    home: &Path,
    db_dir: &Path,
    selection: &SelectionRules,
    projectless_root: &Path,
) -> Result<CodexExport> {
    let compatibility = inspect(db_dir)?;
    if !compatibility.supported {
        return Err(SpiceError::UnsupportedCodex(
            compatibility.explanation.clone(),
        ));
    }
    let catalog = list_content_from_paths(
        home,
        &db_dir.join("state_5.sqlite"),
        &db_dir.join("thread_history_1.sqlite"),
    )?;
    let associations = read_ui_associations(home);
    let state = open_read_only(&db_dir.join("state_5.sqlite"))?;
    let history = open_read_only(&db_dir.join("thread_history_1.sqlite"))?;
    let parents = thread_parents(&state)?;
    let selected_project_ids: HashSet<String> = catalog
        .projects
        .iter()
        .filter(|project| project_mode(selection, &project.id) != ProjectMode::Excluded)
        .map(|project| project.id.clone())
        .collect();
    let mut selected_threads: Vec<_> = catalog
        .threads
        .iter()
        .filter(|thread| {
            if selection.excluded_thread_ids.contains(&thread.id)
                || (thread.archived && !selection.include_archived)
            {
                return false;
            }
            match &thread.project_id {
                Some(id) => selected_project_ids.contains(id),
                None => true,
            }
        })
        .collect();
    // Excluding a parent must also exclude its attached agent histories, including nested
    // descendants. Orphaned histories remain selectable when the parent no longer exists.
    let known_ids: HashSet<_> = catalog
        .threads
        .iter()
        .map(|thread| thread.id.as_str())
        .collect();
    loop {
        let selected_ids: HashSet<_> = selected_threads
            .iter()
            .map(|thread| thread.id.as_str())
            .collect();
        let omitted: HashSet<_> = selected_threads
            .iter()
            .filter(|thread| {
                parents.get(&thread.id).is_some_and(|parent| {
                    known_ids.contains(parent.as_str()) && !selected_ids.contains(parent.as_str())
                })
            })
            .map(|thread| thread.id.clone())
            .collect();
        if omitted.is_empty() {
            break;
        }
        selected_threads.retain(|thread| !omitted.contains(&thread.id));
    }
    let thread_ids: HashSet<String> = selected_threads
        .iter()
        .map(|thread| thread.id.clone())
        .collect();
    let mut pending_files = Vec::new();
    let mut warnings = Vec::new();
    let mut threads = Vec::new();
    for summary in selected_threads {
        let mut state_rows = BTreeMap::new();
        for (table, key) in STATE_THREAD_TABLES {
            let storage_table = state_storage_table(table, compatibility.state_migration);
            let mut rows = query_rows_eq(&state, storage_table, key, &summary.id)?;
            if *table == "thread_artifacts" && compatibility.state_migration == Some(55) {
                rename_artifact_type(&mut rows, "attachment_type", "artifact_type")?;
            }
            state_rows.insert((*table).to_string(), rows);
        }
        let edges = state_rows
            .entry("thread_spawn_edges".to_string())
            .or_default();
        edges.retain(|row| {
            row_string(row, "parent_thread_id")
                .map(|id| thread_ids.contains(id))
                .unwrap_or(false)
        });
        if let Some(section_id) = state_rows
            .get("threads")
            .and_then(|rows| rows.first())
            .and_then(|row| row_string(row, "thread_section_id"))
        {
            state_rows.insert(
                "thread_sections".to_string(),
                query_rows_eq(&state, "thread_sections", "id", section_id)?,
            );
        }
        let mut history_rows = BTreeMap::new();
        for (table, key) in HISTORY_THREAD_TABLES {
            history_rows.insert(
                (*table).to_string(),
                query_rows_eq(&history, table, key, &summary.id)?,
            );
        }
        let rollout = state_rows
            .get("threads")
            .and_then(|rows| rows.first())
            .and_then(|row| row_string(row, "rollout_path"))
            .map(PathBuf::from);
        let rollout_relative_path = rollout
            .as_ref()
            .and_then(|path| relative_under(path, home))
            .map(|path| path.to_string_lossy().replace('\\', "/"));
        if let Some(path) = rollout.as_ref().filter(|path| path.is_file()) {
            pending_files.push(PendingFile {
                logical_path: format!("codex/rollouts/{}.jsonl", summary.id),
                owner_id: summary.id.clone(),
                source: path.clone(),
                kind: PendingFileKind::Rollout,
            });
        } else {
            warnings.push(format!("Chat {} has no readable rollout file; its database-backed history will still be included.", summary.title));
        }
        let projectless_relative_root = if summary.projectless {
            let output_root = associations
                .projectless_outputs
                .get(&summary.id)
                .and_then(|output| Path::new(output).parent())
                .map(Path::to_path_buf);
            let cwd_root = PathBuf::from(&summary.cwd);
            let candidates = output_root.into_iter().chain(std::iter::once(cwd_root));
            let mut existing_outside_root = None;
            let selected = candidates.filter(|root| root.is_dir()).find_map(|root| {
                let relative = relative_under(&root, projectless_root);
                match relative {
                    Some(relative) if !relative.as_os_str().is_empty() => Some((root, relative)),
                    _ => {
                        existing_outside_root.get_or_insert(root);
                        None
                    }
                }
            });
            if let Some((root, relative)) = selected {
                pending_files.push(PendingFile {
                    logical_path: format!("projectless/{}/", summary.id),
                    owner_id: summary.id.clone(),
                    source: root,
                    kind: PendingFileKind::ProjectlessRoot,
                });
                Some(relative.to_string_lossy().replace('\\', "/"))
            } else {
                if let Some(root) = existing_outside_root {
                    warnings.push(format!(
                        "Chat {} has workspace files outside the configured projectless root, so those files were not captured: {}",
                        summary.title,
                        root.display()
                    ));
                }
                None
            }
        } else {
            None
        };
        let mut attachments = Vec::new();
        let mut attachment_sources = collect_local_image_paths(&state_rows, &history_rows);
        if let Some(path) = rollout.as_ref().filter(|path| path.is_file()) {
            attachment_sources.extend(collect_rollout_local_image_paths(path)?);
        }
        let mut attachment_seen = HashSet::new();
        attachment_sources
            .retain(|path| attachment_seen.insert(path.to_string_lossy().to_lowercase()));
        for (index, path) in attachment_sources.into_iter().enumerate() {
            let extension = path
                .extension()
                .and_then(|value| value.to_str())
                .filter(|value| {
                    !value.is_empty()
                        && value.len() <= 12
                        && value
                            .chars()
                            .all(|character| character.is_ascii_alphanumeric())
                })
                .map(|value| format!(".{}", value.to_ascii_lowercase()))
                .unwrap_or_else(|| ".bin".to_string());
            let logical_path = format!(
                "codex/attachments/{}/attachment-{index:04}{extension}",
                summary.id
            );
            attachments.push(AttachmentReference {
                source_path: path.to_string_lossy().into_owned(),
                logical_path: logical_path.clone(),
            });
            pending_files.push(PendingFile {
                logical_path,
                owner_id: summary.id.clone(),
                source: path,
                kind: PendingFileKind::Attachment,
            });
        }
        scrub_local_thread_fields(&mut state_rows);
        let fingerprint = hash_thread_rows(&state_rows, &history_rows)?;
        threads.push(ThreadExport {
            id: summary.id.clone(),
            title: summary.title.clone(),
            project_id: summary.project_id.clone(),
            projectless: summary.projectless,
            archived: summary.archived,
            source_cwd: summary.cwd.clone(),
            rollout_relative_path,
            projectless_relative_root,
            attachments,
            state_rows,
            history_rows,
            fingerprint,
        });
    }
    let mut projects = Vec::new();
    for summary in catalog
        .projects
        .iter()
        .filter(|project| selected_project_ids.contains(&project.id))
    {
        let mut rows = BTreeMap::new();
        rows.insert(
            "projects".to_string(),
            query_rows_eq(&state, "projects", "id", &summary.id)?,
        );
        rows.insert(
            "project_roots".to_string(),
            query_rows_eq(&state, "project_roots", "project_id", &summary.id)?,
        );
        projects.push(ProjectExport {
            id: summary.id.clone(),
            legacy_id: associations.legacy_by_project.get(&summary.id).cloned(),
            name: summary.name.clone(),
            mode: project_mode(selection, &summary.id),
            source_roots: summary.roots.clone(),
            rows,
            git: Vec::new(),
        });
    }
    let selected_legacy_project_ids: HashSet<String> = selected_project_ids
        .iter()
        .filter_map(|id| associations.legacy_by_project.get(id).cloned())
        .collect();
    let ui_state = filter_ui_state(&associations.raw, &thread_ids, &selected_legacy_project_ids);
    let session_index_lines = filter_session_index(&home.join("session_index.jsonl"), &thread_ids);
    Ok(CodexExport {
        compatibility,
        threads,
        projects,
        pending_files,
        ui_state,
        session_index_lines,
        warnings,
    })
}

fn list_content_from_paths(
    home: &Path,
    state_path: &Path,
    history_path: &Path,
) -> Result<ContentCatalog> {
    // The catalog reads associations and rollout sizes from the live home, but rows from stable DB backups.
    let state = open_read_only(state_path)?;
    let associations = read_ui_associations(home);
    let history = open_read_only(history_path)?;
    let mut sizes = HashMap::new();
    if table_exists(&history, "thread_items") {
        let mut statement = history.prepare(
            "SELECT thread_id, sum(length(CAST(item_json AS BLOB))) FROM thread_items GROUP BY thread_id",
        )?;
        for row in statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<i64>>(1)?.unwrap_or(0).max(0) as u64,
            ))
        })? {
            let (id, size) = row?;
            sizes.insert(id, size);
        }
    }
    let mut statement = state.prepare("SELECT id, coalesce(nullif(trim(name), ''), nullif(trim(title), ''), 'Untitled chat'), coalesce(preview, first_user_message, ''), cwd, project_id, archived, coalesce(updated_at_ms, updated_at * 1000, 0), rollout_path FROM threads ORDER BY coalesce(updated_at_ms, updated_at * 1000, 0) DESC")?;
    let mut threads = statement
        .query_map([], |row| {
            let id: String = row.get(0)?;
            let project_id: Option<String> = row
                .get::<_, Option<String>>(4)?
                .or_else(|| associations.assignments.get(&id).cloned());
            let rollout: String = row.get(7)?;
            Ok(ThreadSummary {
                projectless: project_id.is_none(),
                estimated_bytes: sizes.get(&id).copied().unwrap_or(0)
                    + fs::metadata(rollout).map(|meta| meta.len()).unwrap_or(0),
                id,
                title: display_thread_title(&row.get::<_, String>(1)?),
                preview: display_label(&row.get::<_, String>(2)?, DISPLAY_PREVIEW_LIMIT),
                cwd: row.get(3)?,
                project_id,
                archived: row.get::<_, i64>(5)? != 0,
                updated_at_ms: row.get(6)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    omit_internal_threads(&state, &mut threads)?;
    inherit_parent_projects(&state, &associations, &mut threads)?;
    let projects = read_projects(&state, &threads)?;
    Ok(ContentCatalog {
        total_estimated_bytes: 0,
        threads,
        projects,
        warnings: Vec::new(),
    })
}

fn project_mode(selection: &SelectionRules, id: &str) -> ProjectMode {
    selection
        .project_modes
        .get(id)
        .copied()
        .unwrap_or(selection.default_project_mode)
}

fn query_rows_eq(
    connection: &Connection,
    table: &str,
    column: &str,
    value: &str,
) -> Result<Vec<DatabaseRow>> {
    if !table_exists(connection, table) {
        return Ok(Vec::new());
    }
    query_rows(
        connection,
        &format!(
            "SELECT * FROM \"{}\" WHERE \"{}\" = ?1",
            safe_identifier(table)?,
            safe_identifier(column)?
        ),
        [value],
    )
}

fn query_rows<'a>(
    connection: &Connection,
    sql: &str,
    values: impl IntoIterator<Item = &'a str>,
) -> Result<Vec<DatabaseRow>> {
    let mut statement = connection.prepare(sql)?;
    let column_names: Vec<String> = statement
        .column_names()
        .into_iter()
        .map(str::to_string)
        .collect();
    let rows = statement.query_map(params_from_iter(values), |row| {
        let mut map = BTreeMap::new();
        for (index, name) in column_names.iter().enumerate() {
            let value = match row.get_ref(index)? {
                ValueRef::Null => SqlValue::Null,
                ValueRef::Integer(value) => SqlValue::Integer(value),
                ValueRef::Real(value) => SqlValue::Real(value),
                ValueRef::Text(value) => {
                    SqlValue::Text(String::from_utf8_lossy(value).into_owned())
                }
                ValueRef::Blob(value) => SqlValue::Blob(BASE64.encode(value)),
            };
            map.insert(name.clone(), value);
        }
        Ok(DatabaseRow { values: map })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

fn table_columns(connection: &Connection, table: &str) -> Result<Vec<String>> {
    let mut statement = connection.prepare(&format!(
        "PRAGMA table_info(\"{}\")",
        safe_identifier(table)?
    ))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

fn safe_identifier(value: &str) -> Result<&str> {
    if value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        Ok(value)
    } else {
        Err(SpiceError::User(format!(
            "Unsafe database identifier: {value}"
        )))
    }
}

fn row_string<'a>(row: &'a DatabaseRow, column: &str) -> Option<&'a str> {
    match row.values.get(column) {
        Some(SqlValue::Text(value)) => Some(value),
        _ => None,
    }
}

fn hash_thread_rows(
    state: &BTreeMap<String, Vec<DatabaseRow>>,
    history: &BTreeMap<String, Vec<DatabaseRow>>,
) -> Result<String> {
    let mut normalized = state.clone();
    let mut normalized_history = history.clone();
    if let Some(rows) = normalized.get_mut("threads") {
        for row in rows {
            row.values.remove("rollout_path");
            row.values.remove("cwd");
            for column in LOCAL_ONLY_THREAD_COLUMNS {
                row.values.remove(*column);
            }
            // A nullable column added by a later migration is equivalent to its absence.
            for column in ["originator", "daybreak_enabled"] {
                if matches!(row.values.get(column), Some(SqlValue::Null)) {
                    row.values.remove(column);
                }
            }
        }
    }
    normalize_local_image_paths_in_rows(&mut normalized);
    normalize_local_image_paths_in_rows(&mut normalized_history);
    hash_json(&(normalized, normalized_history))
}

fn scrub_local_thread_fields(state: &mut BTreeMap<String, Vec<DatabaseRow>>) {
    if let Some(rows) = state.get_mut("threads") {
        for row in rows {
            for column in LOCAL_ONLY_THREAD_COLUMNS {
                row.values.remove(*column);
            }
        }
    }
}

fn collect_local_image_paths(
    state: &BTreeMap<String, Vec<DatabaseRow>>,
    history: &BTreeMap<String, Vec<DatabaseRow>>,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    for rows in state.values().chain(history.values()) {
        for row in rows {
            for value in row.values.values() {
                let SqlValue::Text(text) = value else {
                    continue;
                };
                let Ok(json) = serde_json::from_str::<JsonValue>(text) else {
                    continue;
                };
                collect_local_images_from_json(&json, &mut paths, &mut seen);
            }
        }
    }
    paths
}

fn collect_local_images_from_json(
    value: &JsonValue,
    paths: &mut Vec<PathBuf>,
    seen: &mut HashSet<String>,
) {
    match value {
        JsonValue::Object(object) => {
            if object.get("type").and_then(JsonValue::as_str) == Some("localImage") {
                if let Some(path) = object
                    .get("path")
                    .and_then(JsonValue::as_str)
                    .map(PathBuf::from)
                    // A moved project may only exist at its configured source
                    // override. Resolve availability during file capture.
                    .filter(|path| path.is_absolute())
                {
                    let key = path.to_string_lossy().to_lowercase();
                    if seen.insert(key) {
                        paths.push(path);
                    }
                }
            }
            for child in object.values() {
                collect_local_images_from_json(child, paths, seen);
            }
        }
        JsonValue::Array(values) => {
            for child in values {
                collect_local_images_from_json(child, paths, seen);
            }
        }
        _ => {}
    }
}

fn collect_rollout_local_image_paths(path: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    let mut reader = BufReader::new(File::open(path)?);
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let content = line.trim_end_matches(['\r', '\n']);
        if let Ok(json) = serde_json::from_str::<JsonValue>(content) {
            collect_local_images_from_json(&json, &mut paths, &mut seen);
        }
    }
    Ok(paths)
}

pub fn portable_rollout_fingerprint(path: &Path) -> Result<String> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut hasher = Sha256::new();
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let content = line.trim_end_matches(['\r', '\n']);
        if let Ok(mut json) = serde_json::from_str::<JsonValue>(content) {
            normalize_local_images_in_json(&mut json, None);
            hasher.update(json.to_string().as_bytes());
        } else {
            hasher.update(content.as_bytes());
        }
        hasher.update(b"\n");
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn rewrite_rollout_local_image_paths(
    path: &Path,
    replacements: &HashMap<String, String>,
) -> Result<usize> {
    if replacements.is_empty() {
        return Ok(0);
    }
    let parent = path.parent().ok_or_else(|| {
        SpiceError::User(format!("Transcript path has no parent: {}", path.display()))
    })?;
    let mut reader = BufReader::new(File::open(path)?);
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    let mut changed_lines = 0_usize;
    {
        let mut writer = BufWriter::new(temporary.as_file_mut());
        let mut line = String::new();
        loop {
            line.clear();
            if reader.read_line(&mut line)? == 0 {
                break;
            }
            let ending = if line.ends_with("\r\n") {
                "\r\n"
            } else if line.ends_with('\n') {
                "\n"
            } else {
                ""
            };
            let content = line.trim_end_matches(['\r', '\n']);
            if let Ok(mut json) = serde_json::from_str::<JsonValue>(content) {
                if normalize_local_images_in_json(&mut json, Some(replacements)) {
                    writer.write_all(json.to_string().as_bytes())?;
                    writer.write_all(ending.as_bytes())?;
                    changed_lines += 1;
                    continue;
                }
            }
            writer.write_all(line.as_bytes())?;
        }
        writer.flush()?;
    }
    temporary.as_file().sync_all()?;
    fs::remove_file(path)?;
    temporary
        .persist(path)
        .map_err(|error| SpiceError::Io(error.error))?;
    Ok(changed_lines)
}

fn normalize_local_image_paths_in_rows(tables: &mut BTreeMap<String, Vec<DatabaseRow>>) {
    for rows in tables.values_mut() {
        for row in rows {
            for value in row.values.values_mut() {
                let SqlValue::Text(text) = value else {
                    continue;
                };
                let Ok(mut json) = serde_json::from_str::<JsonValue>(text) else {
                    continue;
                };
                if normalize_local_images_in_json(&mut json, None) {
                    *text = json.to_string();
                }
            }
        }
    }
}

fn normalize_local_images_in_json(
    value: &mut JsonValue,
    replacements: Option<&HashMap<String, String>>,
) -> bool {
    let mut changed = false;
    match value {
        JsonValue::Object(object) => {
            if object.get("type").and_then(JsonValue::as_str) == Some("localImage") {
                if let Some(path) = object
                    .get_mut("path")
                    .and_then(|value| value.as_str())
                    .map(str::to_string)
                {
                    let replacement = match replacements {
                        Some(values) => values.get(&path).cloned(),
                        None => Some("<portable-local-image>".to_string()),
                    };
                    if let Some(replacement) = replacement {
                        object.insert("path".to_string(), JsonValue::String(replacement));
                        changed = true;
                    }
                }
            }
            for child in object.values_mut() {
                changed |= normalize_local_images_in_json(child, replacements);
            }
        }
        JsonValue::Array(values) => {
            for child in values {
                changed |= normalize_local_images_in_json(child, replacements);
            }
        }
        _ => {}
    }
    changed
}

fn rewrite_local_image_paths_in_rows(
    rows: &mut BTreeMap<String, Vec<DatabaseRow>>,
    replacements: &HashMap<String, String>,
) {
    if replacements.is_empty() {
        return;
    }
    for table_rows in rows.values_mut() {
        for row in table_rows {
            for value in row.values.values_mut() {
                let SqlValue::Text(text) = value else {
                    continue;
                };
                let Ok(mut json) = serde_json::from_str::<JsonValue>(text) else {
                    continue;
                };
                if normalize_local_images_in_json(&mut json, Some(replacements)) {
                    *text = json.to_string();
                }
            }
        }
    }
}

fn filter_ui_state(
    raw: &JsonValue,
    thread_ids: &HashSet<String>,
    legacy_project_ids: &HashSet<String>,
) -> JsonValue {
    let mut output = Map::new();
    for key in [
        "thread-project-assignments",
        "thread-projectless-output-directories",
        "thread-workspace-root-hints",
        "electron-thread-read-state-v1",
    ] {
        if let Some(object) = raw.get(key).and_then(JsonValue::as_object) {
            output.insert(
                key.to_string(),
                JsonValue::Object(
                    object
                        .iter()
                        .filter(|(id, _)| thread_ids.contains(*id))
                        .map(|(key, value)| (key.clone(), value.clone()))
                        .collect(),
                ),
            );
        }
    }
    for key in ["local-projects", "project-appearances"] {
        if let Some(object) = raw.get(key).and_then(JsonValue::as_object) {
            output.insert(
                key.to_string(),
                JsonValue::Object(
                    object
                        .iter()
                        .filter(|(id, _)| legacy_project_ids.contains(*id))
                        .map(|(key, value)| (key.clone(), value.clone()))
                        .collect(),
                ),
            );
        }
    }
    if let Some(ids) = raw
        .get("projectless-thread-ids")
        .and_then(JsonValue::as_array)
    {
        output.insert(
            "projectless-thread-ids".to_string(),
            JsonValue::Array(
                ids.iter()
                    .filter(|id| {
                        id.as_str()
                            .map(|id| thread_ids.contains(id))
                            .unwrap_or(false)
                    })
                    .cloned()
                    .collect(),
            ),
        );
    }
    if let Some(ids) = raw.get("project-order").and_then(JsonValue::as_array) {
        output.insert(
            "project-order".to_string(),
            JsonValue::Array(
                ids.iter()
                    .filter(|id| {
                        id.as_str()
                            .map(|id| legacy_project_ids.contains(id))
                            .unwrap_or(false)
                    })
                    .cloned()
                    .collect(),
            ),
        );
    }
    if let Some(orders) = raw
        .get("sidebar-project-thread-orders")
        .and_then(JsonValue::as_object)
    {
        let filtered = orders
            .iter()
            .filter(|(id, _)| legacy_project_ids.contains(*id))
            .map(|(id, value)| {
                let value = value
                    .as_array()
                    .map(|items| {
                        JsonValue::Array(
                            items
                                .iter()
                                .filter(|item| {
                                    item.as_str()
                                        .map(|id| thread_ids.contains(id))
                                        .unwrap_or(false)
                                })
                                .cloned()
                                .collect(),
                        )
                    })
                    .unwrap_or_else(|| value.clone());
                (id.clone(), value)
            })
            .collect();
        output.insert(
            "sidebar-project-thread-orders".to_string(),
            JsonValue::Object(filtered),
        );
    }
    JsonValue::Object(output)
}

fn filter_session_index(path: &Path, thread_ids: &HashSet<String>) -> Vec<String> {
    fs::read_to_string(path)
        .ok()
        .into_iter()
        .flat_map(|content| content.lines().map(str::to_string).collect::<Vec<_>>())
        .filter(|line| {
            serde_json::from_str::<JsonValue>(line)
                .ok()
                .map(|value| contains_selected_id(&value, thread_ids))
                .unwrap_or(false)
        })
        .collect()
}

fn contains_selected_id(value: &JsonValue, ids: &HashSet<String>) -> bool {
    match value {
        JsonValue::String(value) => ids.contains(value),
        JsonValue::Array(values) => values.iter().any(|value| contains_selected_id(value, ids)),
        JsonValue::Object(values) => values
            .values()
            .any(|value| contains_selected_id(value, ids)),
        _ => false,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn apply_bundle(
    staged_home: &Path,
    manifest: &SnapshotManifest,
    incoming_threads: &HashSet<String>,
    incoming_projects: &HashSet<String>,
    deleted_threads: &HashSet<String>,
    deleted_projects: &HashSet<String>,
    config: &AppConfig,
    rollout_paths: &HashMap<String, String>,
    attachment_paths: &HashMap<String, HashMap<String, String>>,
) -> Result<()> {
    let compatibility = inspect(staged_home)?;
    if !compatibility.supported {
        return Err(SpiceError::UnsupportedCodex(compatibility.explanation));
    }
    let state = Connection::open(staged_home.join("state_5.sqlite"))?;
    let history = Connection::open(staged_home.join("thread_history_1.sqlite"))?;
    // Preflight every selected row before even the staged databases are mutated.
    for thread in manifest
        .threads
        .iter()
        .filter(|thread| incoming_threads.contains(&thread.id))
    {
        for (table, rows) in &thread.state_rows {
            if !STATE_THREAD_TABLES.iter().any(|(name, _)| name == table)
                && table != "thread_sections"
            {
                return Err(SpiceError::UnsupportedCodex(format!(
                    "Unknown source table: {table}"
                )));
            }
            let storage_table = state_storage_table(table, compatibility.state_migration);
            let storage_rows = state_rows_for_storage(table, rows, compatibility.state_migration)?;
            validate_row_columns(&state, storage_table, &storage_rows)?;
        }
        for (table, rows) in &thread.history_rows {
            if !HISTORY_THREAD_TABLES.iter().any(|(name, _)| name == table) {
                return Err(SpiceError::UnsupportedCodex(format!(
                    "Unknown source history table: {table}"
                )));
            }
            validate_row_columns(&history, table, rows)?;
        }
    }
    for project in manifest
        .projects
        .iter()
        .filter(|project| incoming_projects.contains(&project.id))
    {
        for (table, rows) in &project.rows {
            if table != "projects" && table != "project_roots" {
                return Err(SpiceError::UnsupportedCodex(format!(
                    "Unknown source project table: {table}"
                )));
            }
            validate_row_columns(&state, table, rows)?;
        }
    }
    let local_thread_defaults = destination_thread_defaults(&state)?;
    state.execute_batch("PRAGMA foreign_keys=OFF; BEGIN IMMEDIATE;")?;
    history.execute_batch("BEGIN IMMEDIATE;")?;
    let apply_result = (|| -> Result<()> {
        for thread_id in deleted_threads {
            for table in [
                "thread_items",
                "thread_realtime_items",
                "thread_turns",
                "thread_history_projection_state",
            ] {
                history.execute(
                    &format!("DELETE FROM \"{table}\" WHERE thread_id = ?1"),
                    [thread_id],
                )?;
            }
            for table in [
                artifact_table(compatibility.state_migration),
                "thread_dynamic_tools",
            ] {
                state.execute(
                    &format!("DELETE FROM \"{table}\" WHERE thread_id = ?1"),
                    [thread_id],
                )?;
            }
            state.execute("DELETE FROM thread_spawn_edges WHERE parent_thread_id = ?1 OR child_thread_id = ?1", [thread_id])?;
            state.execute("DELETE FROM threads WHERE id = ?1", [thread_id])?;
        }
        for project_id in deleted_projects {
            state.execute(
                "UPDATE threads SET project_id = NULL WHERE project_id = ?1",
                [project_id],
            )?;
            state.execute(
                "DELETE FROM project_roots WHERE project_id = ?1",
                [project_id],
            )?;
            state.execute("DELETE FROM projects WHERE id = ?1", [project_id])?;
        }
        for project in manifest
            .projects
            .iter()
            .filter(|project| incoming_projects.contains(&project.id))
        {
            upsert_rows(
                &state,
                "projects",
                project
                    .rows
                    .get("projects")
                    .map(Vec::as_slice)
                    .unwrap_or(&[]),
                None,
            )?;
            state.execute(
                "DELETE FROM project_roots WHERE project_id = ?1",
                [&project.id],
            )?;
            let mut roots = project
                .rows
                .get("project_roots")
                .cloned()
                .unwrap_or_default();
            for row in &mut roots {
                if let Some(SqlValue::Integer(position)) = row.values.get("position") {
                    if let Some(destination) =
                        destination_project_root(project, *position as usize, config)
                    {
                        row.values.insert(
                            "path".to_string(),
                            SqlValue::Text(destination.to_string_lossy().into_owned()),
                        );
                    }
                }
            }
            upsert_rows(&state, "project_roots", &roots, None)?;
        }
        for thread in manifest
            .threads
            .iter()
            .filter(|thread| incoming_threads.contains(&thread.id))
        {
            let mut state_rows = thread.state_rows.clone();
            let mut history_rows = thread.history_rows.clone();
            if let Some(replacements) = attachment_paths.get(&thread.id) {
                rewrite_local_image_paths_in_rows(&mut state_rows, replacements);
                rewrite_local_image_paths_in_rows(&mut history_rows, replacements);
            }
            for table in [
                artifact_table(compatibility.state_migration),
                "thread_dynamic_tools",
            ] {
                state.execute(
                    &format!("DELETE FROM \"{table}\" WHERE thread_id = ?1"),
                    [&thread.id],
                )?;
            }
            state.execute(
                "DELETE FROM thread_spawn_edges WHERE child_thread_id = ?1",
                [&thread.id],
            )?;
            upsert_rows(
                &state,
                "thread_sections",
                state_rows
                    .get("thread_sections")
                    .map(Vec::as_slice)
                    .unwrap_or(&[]),
                None,
            )?;
            let mut thread_rows = state_rows.get("threads").cloned().unwrap_or_default();
            let local_fields = destination_thread_fields(&state, &thread.id)?
                .unwrap_or_else(|| local_thread_defaults.clone());
            for row in &mut thread_rows {
                apply_destination_thread_fields(row, &local_fields);
                if let Some(path) = rollout_paths.get(&thread.id) {
                    row.values
                        .insert("rollout_path".to_string(), SqlValue::Text(path.clone()));
                }
                let cwd = destination_cwd(thread, manifest, config);
                row.values.insert("cwd".to_string(), SqlValue::Text(cwd));
            }
            upsert_rows(&state, "threads", &thread_rows, None)?;
            for table in [
                "thread_artifacts",
                "thread_dynamic_tools",
                "thread_spawn_edges",
            ] {
                let rows = state_rows.get(table).map(Vec::as_slice).unwrap_or(&[]);
                let storage_rows =
                    state_rows_for_storage(table, rows, compatibility.state_migration)?;
                upsert_rows(
                    &state,
                    state_storage_table(table, compatibility.state_migration),
                    &storage_rows,
                    None,
                )?;
            }
            for table in [
                "thread_history_projection_state",
                "thread_items",
                "thread_realtime_items",
                "thread_turns",
            ] {
                history.execute(
                    &format!("DELETE FROM \"{table}\" WHERE thread_id = ?1"),
                    [&thread.id],
                )?;
            }
            // Clear projection state before inserting realtime rows: its DELETE trigger
            // also clears realtime history in the actual Codex schema.
            for table in [
                "thread_items",
                "thread_realtime_items",
                "thread_turns",
                "thread_history_projection_state",
            ] {
                upsert_rows(
                    &history,
                    table,
                    history_rows.get(table).map(Vec::as_slice).unwrap_or(&[]),
                    None,
                )?;
            }
        }
        state.execute_batch("COMMIT;")?;
        history.execute_batch("COMMIT;")?;
        Ok(())
    })();
    if apply_result.is_err() {
        let _ = state.execute_batch("ROLLBACK;");
        let _ = history.execute_batch("ROLLBACK;");
    }
    apply_result?;
    merge_global_state(
        staged_home,
        &manifest.ui_state,
        manifest,
        incoming_threads,
        incoming_projects,
        deleted_threads,
        deleted_projects,
        config,
    )?;
    merge_session_index(
        staged_home,
        &manifest.session_index_lines,
        incoming_threads,
        deleted_threads,
    )?;
    Ok(())
}

fn destination_thread_defaults(connection: &Connection) -> Result<BTreeMap<String, SqlValue>> {
    let latest_id = connection
        .query_row(
            "SELECT id FROM threads ORDER BY coalesce(updated_at_ms, updated_at * 1000, created_at * 1000, 0) DESC LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let mut fields = latest_id
        .as_deref()
        .map(|id| destination_thread_fields(connection, id))
        .transpose()?
        .flatten()
        .unwrap_or_default();
    fields.remove("agent_path");
    fields
        .entry("sandbox_policy".to_string())
        .or_insert_with(|| SqlValue::Text(SAFE_IMPORTED_SANDBOX_POLICY.to_string()));
    fields
        .entry("approval_mode".to_string())
        .or_insert_with(|| SqlValue::Text(SAFE_IMPORTED_APPROVAL_MODE.to_string()));
    Ok(fields)
}

fn destination_thread_fields(
    connection: &Connection,
    thread_id: &str,
) -> Result<Option<BTreeMap<String, SqlValue>>> {
    let Some(row) = query_rows_eq(connection, "threads", "id", thread_id)?
        .into_iter()
        .next()
    else {
        return Ok(None);
    };
    Ok(Some(
        LOCAL_ONLY_THREAD_COLUMNS
            .iter()
            .filter_map(|column| {
                row.values
                    .get(*column)
                    .cloned()
                    .map(|value| ((*column).to_string(), value))
            })
            .collect(),
    ))
}

fn apply_destination_thread_fields(row: &mut DatabaseRow, fields: &BTreeMap<String, SqlValue>) {
    for column in LOCAL_ONLY_THREAD_COLUMNS {
        row.values.remove(*column);
    }
    for (column, value) in fields {
        row.values.insert(column.clone(), value.clone());
    }
}

fn upsert_rows(
    connection: &Connection,
    table: &str,
    rows: &[DatabaseRow],
    mutator: Option<&dyn Fn(&mut DatabaseRow)>,
) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    validate_row_columns(connection, table, rows)?;
    let available: HashSet<String> = table_columns(connection, table)?.into_iter().collect();
    let primary_key = table_primary_key_columns(connection, table)?;
    for source in rows {
        let mut row = source.clone();
        if let Some(mutate) = mutator {
            mutate(&mut row);
        }
        let columns: Vec<String> = row
            .values
            .keys()
            .filter(|column| available.contains(*column))
            .cloned()
            .collect();
        if columns.is_empty() {
            continue;
        }
        let names = columns
            .iter()
            .map(|column| format!("\"{}\"", column))
            .collect::<Vec<_>>()
            .join(",");
        let placeholders = (1..=columns.len())
            .map(|index| format!("?{index}"))
            .collect::<Vec<_>>()
            .join(",");
        let updates: Vec<String> = columns
            .iter()
            .filter(|column| !primary_key.contains(*column))
            .map(|column| format!("\"{column}\"=excluded.\"{column}\""))
            .collect();
        let conflict_action = if updates.is_empty() {
            "DO NOTHING".to_string()
        } else {
            format!("DO UPDATE SET {}", updates.join(","))
        };
        let sql = format!(
            "INSERT INTO \"{}\" ({names}) VALUES ({placeholders}) ON CONFLICT {conflict_action}",
            safe_identifier(table)?
        );
        let values: Vec<Value> = columns
            .iter()
            .map(|column| sql_value(row.values.get(column).expect("column exists")))
            .collect::<Result<_>>()?;
        connection.execute(&sql, params_from_iter(values))?;
    }
    Ok(())
}

fn validate_row_columns(connection: &Connection, table: &str, rows: &[DatabaseRow]) -> Result<()> {
    let available: HashSet<_> = table_columns(connection, table)?.into_iter().collect();
    for row in rows {
        for (column, value) in &row.values {
            if !available.contains(column)
                && !(table == "threads"
                    && matches!(column.as_str(), "originator" | "daybreak_enabled")
                    && matches!(value, SqlValue::Null))
            {
                return Err(SpiceError::UnsupportedCodex(format!("The destination cannot represent {table}.{column}. Update Codex on the destination before importing. Nothing was discarded.")));
            }
        }
    }
    Ok(())
}

fn table_primary_key_columns(connection: &Connection, table: &str) -> Result<HashSet<String>> {
    let mut statement = connection.prepare(&format!(
        "PRAGMA table_info(\"{}\")",
        safe_identifier(table)?
    ))?;
    let columns = statement.query_map([], |row| {
        Ok((row.get::<_, String>(1)?, row.get::<_, i64>(5)?))
    })?;
    Ok(columns
        .filter_map(|entry| match entry {
            Ok((column, position)) if position > 0 => Some(Ok(column)),
            Ok(_) => None,
            Err(error) => Some(Err(error)),
        })
        .collect::<std::result::Result<_, _>>()?)
}

fn sql_value(value: &SqlValue) -> Result<Value> {
    Ok(match value {
        SqlValue::Null => Value::Null,
        SqlValue::Integer(value) => Value::Integer(*value),
        SqlValue::Real(value) => Value::Real(*value),
        SqlValue::Text(value) => Value::Text(value.clone()),
        SqlValue::Blob(value) => Value::Blob(
            BASE64
                .decode(value)
                .map_err(|error| SpiceError::User(format!("Invalid database blob: {error}")))?,
        ),
    })
}

fn destination_cwd(
    thread: &ThreadExport,
    manifest: &SnapshotManifest,
    config: &AppConfig,
) -> String {
    if let Some(project_id) = &thread.project_id {
        if let Some(project) = manifest
            .projects
            .iter()
            .find(|project| &project.id == project_id)
        {
            if project.mode == ProjectMode::HistoryOnly {
                return history_only_root(project, 0, config)
                    .to_string_lossy()
                    .into_owned();
            }
            for (index, source) in project.source_roots.iter().enumerate() {
                if let Some(relative) =
                    relative_under(Path::new(&thread.source_cwd), Path::new(source))
                {
                    if let Some(destination) = destination_project_root(project, index, config) {
                        return destination.join(relative).to_string_lossy().into_owned();
                    }
                }
            }
        }
    }
    if thread.projectless {
        let root = thread
            .projectless_relative_root
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(safe_name(&thread.title, &thread.id)));
        return Path::new(&config.projectless_root)
            .join(root)
            .to_string_lossy()
            .into_owned();
    }
    thread.source_cwd.clone()
}

pub(crate) fn relative_under(path: &Path, root: &Path) -> Option<PathBuf> {
    // Exported Windows paths can retain the extended-length prefix even though
    // project roots do not. Compare lexically before touching the source filesystem:
    // that drive may not exist on the receiving computer.
    let path = dunce::simplified(path);
    let root = dunce::simplified(root);
    if let Ok(relative) = path.strip_prefix(root) {
        return Some(relative.to_path_buf());
    }
    #[cfg(windows)]
    {
        let path_components: Vec<_> = path.components().collect();
        let root_components: Vec<_> = root.components().collect();
        if root_components.len() <= path_components.len()
            && root_components
                .iter()
                .zip(&path_components)
                .all(|(left, right)| {
                    use std::path::{Component, Prefix};
                    if let (Component::Prefix(left), Component::Prefix(right)) = (left, right) {
                        let unc = |prefix| match prefix {
                            Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
                                Some((server, share))
                            }
                            _ => None,
                        };
                        if let (
                            Some((left_server, left_share)),
                            Some((right_server, right_share)),
                        ) = (unc(left.kind()), unc(right.kind()))
                        {
                            return left_server.eq_ignore_ascii_case(right_server)
                                && left_share.eq_ignore_ascii_case(right_share);
                        }
                    }
                    left.as_os_str()
                        .to_string_lossy()
                        .eq_ignore_ascii_case(&right.as_os_str().to_string_lossy())
                })
        {
            return Some(
                path_components[root_components.len()..]
                    .iter()
                    .map(|component| component.as_os_str())
                    .collect(),
            );
        }
    }
    let canonical_path = dunce::canonicalize(path).ok()?;
    let canonical_root = dunce::canonicalize(root).ok()?;
    canonical_path
        .strip_prefix(canonical_root)
        .ok()
        .map(Path::to_path_buf)
}

pub fn destination_project_root(
    project: &ProjectExport,
    index: usize,
    config: &AppConfig,
) -> Option<PathBuf> {
    if project.mode == ProjectMode::HistoryOnly {
        return Some(history_only_root(project, index, config));
    }
    let key = format!("{}:{}", project.id, index);
    config
        .destination_roots
        .get(&key)
        .or_else(|| {
            (index == 0)
                .then(|| config.destination_roots.get(&project.id))
                .flatten()
        })
        .map(PathBuf::from)
}

pub fn history_only_root(project: &ProjectExport, index: usize, config: &AppConfig) -> PathBuf {
    let base = Path::new(&config.projectless_root)
        .join("History only")
        .join(safe_name(&project.name, &project.id));
    if index == 0 {
        base
    } else {
        base.join(format!("Root {}", index + 1))
    }
}

fn safe_name(name: &str, fallback: &str) -> String {
    let value: String = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, ' ' | '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect();
    let value = value.trim_matches([' ', '.']).trim();
    if value.is_empty() {
        fallback.chars().take(12).collect()
    } else {
        value.to_string()
    }
}

#[allow(clippy::too_many_arguments)]
fn merge_global_state(
    home: &Path,
    patch: &JsonValue,
    manifest: &SnapshotManifest,
    incoming_threads: &HashSet<String>,
    incoming_projects: &HashSet<String>,
    deleted_threads: &HashSet<String>,
    deleted_projects: &HashSet<String>,
    config: &AppConfig,
) -> Result<()> {
    let path = home.join(".codex-global-state.json");
    let mut local: JsonValue = fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_else(|| JsonValue::Object(Map::new()));
    let local_object = local
        .as_object_mut()
        .ok_or_else(|| SpiceError::User("Codex global state is not a JSON object.".to_string()))?;
    let mut deleted_legacy_projects = HashSet::new();
    if let Some(hosts) = local_object
        .get_mut("app-server-project-id-by-legacy-project-id-by-host")
        .and_then(JsonValue::as_object_mut)
    {
        for mapping in hosts.values_mut().filter_map(JsonValue::as_object_mut) {
            mapping.retain(|legacy_id, value| {
                let keep = value
                    .as_str()
                    .map(|project_id| !deleted_projects.contains(project_id))
                    .unwrap_or(true);
                if !keep {
                    deleted_legacy_projects.insert(legacy_id.clone());
                }
                keep
            });
        }
    }
    for key in [
        "thread-project-assignments",
        "thread-projectless-output-directories",
        "thread-workspace-root-hints",
        "electron-thread-read-state-v1",
    ] {
        if let Some(object) = local_object.get_mut(key).and_then(JsonValue::as_object_mut) {
            for id in deleted_threads {
                object.remove(id);
            }
        }
    }
    if let Some(assignments) = local_object
        .get_mut("thread-project-assignments")
        .and_then(JsonValue::as_object_mut)
    {
        assignments.retain(|_, value| {
            let project_id = value
                .as_str()
                .or_else(|| value.get("projectId").and_then(JsonValue::as_str));
            project_id
                .map(|id| !deleted_legacy_projects.contains(id))
                .unwrap_or(true)
        });
    }
    for key in [
        "local-projects",
        "project-appearances",
        "sidebar-project-thread-orders",
    ] {
        if let Some(object) = local_object.get_mut(key).and_then(JsonValue::as_object_mut) {
            for id in &deleted_legacy_projects {
                object.remove(id);
            }
        }
    }
    if let Some(array) = local_object
        .get_mut("projectless-thread-ids")
        .and_then(JsonValue::as_array_mut)
    {
        array.retain(|value| {
            value
                .as_str()
                .map(|id| !deleted_threads.contains(id))
                .unwrap_or(true)
        });
    }
    if let Some(array) = local_object
        .get_mut("project-order")
        .and_then(JsonValue::as_array_mut)
    {
        array.retain(|value| {
            value
                .as_str()
                .map(|id| !deleted_legacy_projects.contains(id))
                .unwrap_or(true)
        });
    }

    let incoming_legacy_projects: HashSet<String> = manifest
        .projects
        .iter()
        .filter(|project| incoming_projects.contains(&project.id))
        .filter_map(|project| project.legacy_id.clone())
        .collect();
    if let Some(patch_object) = patch.as_object() {
        for key in [
            "thread-project-assignments",
            "thread-projectless-output-directories",
            "thread-workspace-root-hints",
            "electron-thread-read-state-v1",
        ] {
            merge_keyed_values(
                local_object,
                patch_object,
                key,
                incoming_threads,
                |id, mut value| {
                    if key == "thread-workspace-root-hints" {
                        if let Some(thread) = manifest.threads.iter().find(|thread| thread.id == id)
                        {
                            value = JsonValue::String(destination_cwd(thread, manifest, config));
                        }
                    }
                    value
                },
            );
        }
        for key in [
            "local-projects",
            "project-appearances",
            "sidebar-project-thread-orders",
        ] {
            merge_keyed_values(
                local_object,
                patch_object,
                key,
                &incoming_legacy_projects,
                |legacy_id, mut value| {
                    if key == "local-projects" {
                        if let Some(project) = manifest
                            .projects
                            .iter()
                            .find(|project| project.legacy_id.as_deref() == Some(legacy_id))
                        {
                            if let Some(object) = value.as_object_mut() {
                                let roots = (0..project.source_roots.len().max(1))
                                    .filter_map(|index| {
                                        destination_project_root(project, index, config)
                                    })
                                    .map(|path| {
                                        JsonValue::String(path.to_string_lossy().into_owned())
                                    })
                                    .collect();
                                object.insert("rootPaths".to_string(), JsonValue::Array(roots));
                            }
                        }
                    }
                    value
                },
            );
        }
        merge_id_array(
            local_object,
            patch_object,
            "projectless-thread-ids",
            incoming_threads,
        );
        merge_id_array(
            local_object,
            patch_object,
            "project-order",
            &incoming_legacy_projects,
        );
    }

    let host_key = format!("local:{}", config.codex_home);
    let hosts = local_object
        .entry("app-server-project-id-by-legacy-project-id-by-host")
        .or_insert_with(|| JsonValue::Object(Map::new()));
    if !hosts.is_object() {
        *hosts = JsonValue::Object(Map::new());
    }
    let hosts = hosts.as_object_mut().expect("host mapping created");
    let mapping = hosts
        .entry(host_key)
        .or_insert_with(|| JsonValue::Object(Map::new()));
    if !mapping.is_object() {
        *mapping = JsonValue::Object(Map::new());
    }
    let mapping = mapping.as_object_mut().expect("project mapping created");
    for project in manifest
        .projects
        .iter()
        .filter(|project| incoming_projects.contains(&project.id))
    {
        if let Some(legacy_id) = &project.legacy_id {
            mapping.insert(legacy_id.clone(), JsonValue::String(project.id.clone()));
        }
    }
    if let Some(outputs) = local_object
        .get_mut("thread-projectless-output-directories")
        .and_then(JsonValue::as_object_mut)
    {
        for thread in manifest
            .threads
            .iter()
            .filter(|thread| incoming_threads.contains(&thread.id) && thread.projectless)
        {
            let cwd = destination_cwd(thread, manifest, config);
            outputs.insert(
                thread.id.clone(),
                JsonValue::String(
                    Path::new(&cwd)
                        .join("output")
                        .to_string_lossy()
                        .into_owned(),
                ),
            );
        }
    }
    write_json(&path, &local)
}

fn merge_keyed_values<F>(
    local: &mut Map<String, JsonValue>,
    patch: &Map<String, JsonValue>,
    key: &str,
    allowed_ids: &HashSet<String>,
    mut transform: F,
) where
    F: FnMut(&str, JsonValue) -> JsonValue,
{
    let Some(incoming) = patch.get(key).and_then(JsonValue::as_object) else {
        return;
    };
    let target = local
        .entry(key)
        .or_insert_with(|| JsonValue::Object(Map::new()));
    if !target.is_object() {
        *target = JsonValue::Object(Map::new());
    }
    let target = target.as_object_mut().expect("keyed state created");
    for (id, value) in incoming.iter().filter(|(id, _)| allowed_ids.contains(*id)) {
        target.insert(id.clone(), transform(id, value.clone()));
    }
}

fn merge_id_array(
    local: &mut Map<String, JsonValue>,
    patch: &Map<String, JsonValue>,
    key: &str,
    allowed_ids: &HashSet<String>,
) {
    let Some(incoming) = patch.get(key).and_then(JsonValue::as_array) else {
        return;
    };
    let target = local
        .entry(key)
        .or_insert_with(|| JsonValue::Array(Vec::new()));
    if !target.is_array() {
        *target = JsonValue::Array(Vec::new());
    }
    let values = target.as_array_mut().expect("state array created");
    let mut seen: HashSet<String> = values
        .iter()
        .filter_map(JsonValue::as_str)
        .map(str::to_string)
        .collect();
    for item in incoming {
        if let Some(id) = item.as_str() {
            if allowed_ids.contains(id) && seen.insert(id.to_string()) {
                values.push(item.clone());
            }
        }
    }
}

fn merge_session_index(
    home: &Path,
    incoming: &[String],
    incoming_threads: &HashSet<String>,
    deleted_threads: &HashSet<String>,
) -> Result<()> {
    let path = home.join("session_index.jsonl");
    let mut lines: Vec<String> = fs::read_to_string(&path)
        .unwrap_or_default()
        .lines()
        .filter(|line| {
            serde_json::from_str::<JsonValue>(line)
                .ok()
                .map(|value| {
                    !contains_selected_id(&value, deleted_threads)
                        && !contains_selected_id(&value, incoming_threads)
                })
                .unwrap_or(true)
        })
        .map(str::to_string)
        .collect();
    let mut seen: HashSet<String> = lines.iter().cloned().collect();
    for line in incoming {
        let selected = serde_json::from_str::<JsonValue>(line)
            .ok()
            .map(|value| contains_selected_id(&value, incoming_threads))
            .unwrap_or(false);
        if selected && seen.insert(line.clone()) {
            lines.push(line.clone());
        }
    }
    let content = if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    };
    crate::util::atomic_write(&path, content.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::{self, ObjectStore};
    use crate::util::read_json;
    use tempfile::tempdir;

    fn create_fixture_schema(home: &Path) {
        create_profile_schema(home, 52);
    }

    fn create_profile_schema(home: &Path, migration: i64) {
        create_profile_schema_with_line_endings(home, migration, false);
    }

    fn create_profile_schema_with_line_endings(home: &Path, migration: i64, use_lf_only: bool) {
        fs::create_dir_all(home).unwrap();
        let profile = crate::compatibility::PROFILES
            .iter()
            .find(|p| p.state == migration)
            .unwrap();
        let fixture: JsonValue = serde_json::from_str(profile.schema).unwrap();
        for database in ["state_5.sqlite", "thread_history_1.sqlite"] {
            let db = Connection::open(home.join(database)).unwrap();
            let objects = fixture[database]["objects"].as_array().unwrap();
            for kind in ["table", "index", "trigger"] {
                for object in objects.iter().filter(|object| object["type"] == kind) {
                    let sql = object["sql"].as_str().unwrap();
                    let sql = if use_lf_only {
                        Cow::Owned(sql.replace("\r\n", "\n"))
                    } else {
                        Cow::Borrowed(sql)
                    };
                    db.execute_batch(&sql).unwrap();
                }
            }
            db.execute("INSERT INTO _sqlx_migrations(version, description, success, checksum, execution_time) VALUES(?1, 'fixture', 1, X'00', 0)", [fixture[database]["migration"].as_i64().unwrap()]).unwrap();
        }
        assert!(
            inspect(home).unwrap().supported,
            "Real fixture must pass the production schema gate"
        );
    }

    #[test]
    fn schema_fingerprints_are_identical_across_windows_and_macos_line_endings() {
        for migration in [52, 54, 55] {
            let crlf_home = tempdir().unwrap();
            create_profile_schema_with_line_endings(crlf_home.path(), migration, false);
            let lf_home = tempdir().unwrap();
            create_profile_schema_with_line_endings(lf_home.path(), migration, true);

            let crlf = inspect(crlf_home.path()).unwrap();
            let lf = inspect(lf_home.path()).unwrap();
            assert!(crlf.supported, "CRLF schema {migration} must be supported");
            assert!(lf.supported, "LF schema {migration} must be supported");
            assert_eq!(crlf.schema_fingerprint, lf.schema_fingerprint);
            if migration == 54 {
                assert_eq!(
                    lf.schema_fingerprint,
                    "8082e27f46c7a5691ae4dca5b004ce2103bacaafb3989f7be95607d34685c205"
                );
            }
            assert_eq!(
                lf.schema_fingerprint,
                crate::compatibility::PROFILES
                    .iter()
                    .find(|profile| profile.state == migration)
                    .unwrap()
                    .fingerprint
            );

            if migration == 54 {
                let state = Connection::open(lf_home.path().join("state_5.sqlite")).unwrap();
                state.execute(
                    "INSERT INTO _sqlx_migrations(version, description, success, checksum, execution_time) VALUES(999, 'failed fixture', 0, X'00', 0)",
                    [],
                ).unwrap();
                let incomplete = inspect(lf_home.path()).unwrap();
                assert!(!incomplete.supported);
                assert_eq!(incomplete.schema_fingerprint, lf.schema_fingerprint);
            }
        }
    }
    fn insert_project(home: &Path, id: &str, name: &str, root: &Path, position: i64) {
        let state = Connection::open(home.join("state_5.sqlite")).unwrap();
        state.execute(
            "INSERT INTO projects(id, name, metadata, position, created_at_ms, updated_at_ms) VALUES(?1, ?2, '{}', ?3, 1, 1)",
            rusqlite::params![id, name, position],
        ).unwrap();
        state
            .execute(
                "INSERT INTO project_roots(project_id, position, path) VALUES(?1, 0, ?2)",
                rusqlite::params![id, root.to_string_lossy()],
            )
            .unwrap();
    }

    fn insert_thread(
        home: &Path,
        id: &str,
        title: &str,
        cwd: &Path,
        project_id: Option<&str>,
        item_json: &str,
    ) {
        let rollout = home.join("sessions").join(format!("{id}.jsonl"));
        fs::create_dir_all(rollout.parent().unwrap()).unwrap();
        fs::write(&rollout, format!("{{\"thread_id\":\"{id}\"}}\n")).unwrap();
        let state = Connection::open(home.join("state_5.sqlite")).unwrap();
        state.execute(
            "INSERT INTO threads(id, rollout_path, created_at, updated_at, cwd, title, name, first_user_message, preview, project_id, archived, updated_at_ms, thread_section_id, sandbox_policy, approval_mode, agent_path, source, model_provider)
             VALUES(?1, ?2, 1, 1, ?3, ?4, ?4, ?4, ?4, ?5, 0, 1, NULL, '{\"type\":\"disabled\"}', 'never', 'source-agent.toml', 'cli', 'openai')",
            rusqlite::params![id, rollout.to_string_lossy(), cwd.to_string_lossy(), title, project_id],
        ).unwrap();
        let table = artifact_table(migration_version(&state));
        let type_column = if table == "thread_attachments" {
            "attachment_type"
        } else {
            "artifact_type"
        };
        state.execute(
            &format!("INSERT INTO {table}(id, thread_id, {type_column}, identity_key, payload, created_at) VALUES(?1, ?2, 'file', 'artifact', ?3, 1)"),
            rusqlite::params![format!("artifact-{id}"), id, format!("payload-{id}")],
        ).unwrap();
        let history = Connection::open(home.join("thread_history_1.sqlite")).unwrap();
        history.execute(
            "INSERT INTO thread_items(thread_id, turn_id, item_id, rollout_ordinal, created_at_ms, item_json, item_type, updated_at_ordinal) VALUES(?1, 'turn-1', ?2, 1, 1, ?3, 'message', 1)",
            rusqlite::params![id, format!("item-{id}"), item_json],
        ).unwrap();
        history.execute(
            "INSERT INTO thread_turns(thread_id, turn_id, rollout_ordinal, status) VALUES(?1, 'turn-1', 1, 'completed')",
            [id],
        ).unwrap();
        history.execute(
            "INSERT INTO thread_history_projection_state(thread_id, next_rollout_byte_offset, next_rollout_ordinal) VALUES(?1, 1, 2)",
            [id],
        ).unwrap();
        history.execute("INSERT INTO thread_realtime_items(thread_id,item_id,rollout_ordinal,created_at_ms,item_type,item_json) VALUES(?1,'realtime-1',1,1,'realtime_session_started','{\"text\":\"realtime history\"}')", [id]).unwrap();
    }

    fn set_thread_local_fields(
        home: &Path,
        id: &str,
        sandbox_policy: &str,
        approval_mode: &str,
        agent_path: Option<&str>,
        updated_at_ms: i64,
    ) {
        let state = Connection::open(home.join("state_5.sqlite")).unwrap();
        state
            .execute(
                "UPDATE threads SET sandbox_policy=?2, approval_mode=?3, agent_path=?4, updated_at_ms=?5 WHERE id=?1",
                rusqlite::params![
                    id,
                    sandbox_policy,
                    approval_mode,
                    agent_path,
                    updated_at_ms
                ],
            )
            .unwrap();
    }

    #[test]
    fn attachment_discovery_keeps_moved_paths_for_source_mapping() {
        let location = tempdir().unwrap();
        let missing = location.path().join("moved-project").join("image.png");
        assert!(!missing.exists());
        let value = serde_json::json!({ "content": [
            { "type": "localImage", "path": missing.to_string_lossy() },
            { "type": "text", "text": missing.to_string_lossy() }
        ] });
        let mut paths = Vec::new();
        collect_local_images_from_json(&value, &mut paths, &mut HashSet::new());
        assert_eq!(paths, vec![missing]);
    }

    #[test]
    fn selected_export_restores_history_sidebar_and_portable_paths() {
        let source = tempdir().unwrap();
        let code = tempdir().unwrap();
        let destination = tempdir().unwrap();
        let stage = tempdir().unwrap();
        let objects = tempdir().unwrap();
        create_fixture_schema(source.path());
        create_fixture_schema(destination.path());

        let full_root = code.path().join("Full workspace");
        let history_root = code.path().join("History workspace");
        let excluded_root = code.path().join("Excluded workspace");
        fs::create_dir_all(full_root.join("src")).unwrap();
        fs::create_dir_all(&history_root).unwrap();
        fs::create_dir_all(&excluded_root).unwrap();
        fs::write(
            full_root.join("src").join("main.ts"),
            "export const restored = true;\n",
        )
        .unwrap();
        fs::write(full_root.join(".env"), "TOKEN=do-not-export\n").unwrap();
        fs::write(history_root.join("history-only.txt"), "must not transfer\n").unwrap();
        insert_project(source.path(), "project-full", "Full Project", &full_root, 0);
        insert_project(
            source.path(),
            "project-history",
            "History Project",
            &history_root,
            1,
        );
        insert_project(
            source.path(),
            "project-excluded",
            "Excluded Project",
            &excluded_root,
            2,
        );

        let attachment = source.path().join("Uploads").join("reference.png");
        fs::create_dir_all(attachment.parent().unwrap()).unwrap();
        fs::write(&attachment, b"portable image bytes").unwrap();
        let historical_sentence = format!("The old path was {}", full_root.display());
        let historical_text = serde_json::json!({
            "role": "user",
            "text": historical_sentence,
            "content": [{ "type": "localImage", "path": attachment.to_string_lossy() }]
        })
        .to_string();
        insert_thread(
            source.path(),
            "chat-project",
            "Project chat",
            &full_root.join("src"),
            Some("project-full"),
            &historical_text,
        );
        set_thread_local_fields(
            source.path(),
            "chat-project",
            r#"{"type":"disabled","source":"must-not-transfer"}"#,
            "never",
            Some("source-agent.toml"),
            20,
        );
        insert_thread(
            source.path(),
            "chat-individual",
            "Excluded individual chat",
            &full_root,
            Some("project-full"),
            "{\"text\":\"excluded\"}",
        );
        insert_thread(
            source.path(),
            "chat-history",
            "History chat",
            &history_root,
            Some("project-history"),
            "{\"text\":\"history\"}",
        );
        insert_thread(
            source.path(),
            "chat-excluded-project",
            "Excluded project chat",
            &excluded_root,
            Some("project-excluded"),
            "{\"text\":\"excluded project\"}",
        );
        let projectless_root = source.path().join("Projectless");
        let projectless_chat_root = projectless_root.join("Scratch chat");
        fs::create_dir_all(projectless_chat_root.join("output")).unwrap();
        fs::write(
            projectless_chat_root.join("notes.md"),
            "portable artifact\n",
        )
        .unwrap();
        insert_thread(
            source.path(),
            "chat-projectless",
            "Scratch chat",
            &projectless_chat_root,
            None,
            "{\"text\":\"scratch\"}",
        );

        let source_host = format!("local:{}", source.path().display());
        let source_ui = serde_json::json!({
            "local-projects": {
                "legacy-full": { "id": "legacy-full", "name": "Full Project", "rootPaths": [full_root.to_string_lossy()] },
                "legacy-history": { "id": "legacy-history", "name": "History Project", "rootPaths": [history_root.to_string_lossy()] },
                "legacy-excluded": { "id": "legacy-excluded", "name": "Excluded Project", "rootPaths": [excluded_root.to_string_lossy()] }
            },
            "app-server-project-id-by-legacy-project-id-by-host": {
                source_host: {
                    "legacy-full": "project-full",
                    "legacy-history": "project-history",
                    "legacy-excluded": "project-excluded"
                }
            },
            "project-order": ["legacy-full", "legacy-history", "legacy-excluded"],
            "thread-project-assignments": {
                "chat-project": { "projectKind": "local", "projectId": "legacy-full" },
                "chat-individual": { "projectKind": "local", "projectId": "legacy-full" },
                "chat-history": { "projectKind": "local", "projectId": "legacy-history" },
                "chat-excluded-project": { "projectKind": "local", "projectId": "legacy-excluded" }
            },
            "projectless-thread-ids": ["chat-projectless"],
            "thread-projectless-output-directories": {
                "chat-projectless": projectless_chat_root.join("output").to_string_lossy()
            },
            "thread-workspace-root-hints": {
                "chat-project": full_root.to_string_lossy(),
                "chat-projectless": projectless_chat_root.to_string_lossy()
            }
        });
        write_json(&source.path().join(".codex-global-state.json"), &source_ui).unwrap();
        fs::write(
            source.path().join("session_index.jsonl"),
            [
                "{\"id\":\"chat-project\",\"title\":\"Project chat\"}",
                "{\"id\":\"chat-individual\",\"title\":\"Excluded individual chat\"}",
                "{\"id\":\"chat-history\",\"title\":\"History chat\"}",
                "{\"id\":\"chat-excluded-project\",\"title\":\"Excluded project chat\"}",
                "{\"id\":\"chat-projectless\",\"title\":\"Scratch chat\"}",
            ]
            .join("\n")
                + "\n",
        )
        .unwrap();

        let mut config = crate::settings::default_config();
        config.codex_home = source.path().to_string_lossy().into_owned();
        config.projectless_root = projectless_root.to_string_lossy().into_owned();
        config
            .selection
            .project_modes
            .insert("project-history".to_string(), ProjectMode::HistoryOnly);
        config
            .selection
            .project_modes
            .insert("project-excluded".to_string(), ProjectMode::Excluded);
        config
            .selection
            .excluded_thread_ids
            .push("chat-individual".to_string());
        let database_stage = stage.path().join("export-db");
        snapshot_databases(source.path(), &database_stage).unwrap();
        let export = export_selected(
            source.path(),
            &database_stage,
            &config.selection,
            &projectless_root,
        )
        .unwrap();
        let exported_threads: HashSet<_> = export
            .threads
            .iter()
            .map(|thread| thread.id.as_str())
            .collect();
        assert_eq!(
            exported_threads,
            HashSet::from(["chat-project", "chat-history", "chat-projectless"])
        );
        assert_eq!(export.projects.len(), 2);
        assert_eq!(
            export
                .projects
                .iter()
                .find(|project| project.id == "project-full")
                .unwrap()
                .legacy_id
                .as_deref(),
            Some("legacy-full")
        );
        let exported_project_thread = export
            .threads
            .iter()
            .find(|thread| thread.id == "chat-project")
            .unwrap();
        for column in LOCAL_ONLY_THREAD_COLUMNS {
            assert!(!exported_project_thread.state_rows["threads"][0]
                .values
                .contains_key(*column));
        }

        let store = ObjectStore::new(objects.path().join("store")).unwrap();
        let manifest = snapshot::build_manifest(&config, export, &store, None).unwrap();
        assert!(manifest
            .objects
            .iter()
            .any(|object| object.logical_path.ends_with("src/main.ts")));
        assert!(manifest.objects.iter().any(|object| matches!(
            object.kind,
            crate::models::ObjectKind::Artifact
        ) && object.owner_id == "chat-project"));
        assert!(manifest
            .objects
            .iter()
            .any(|object| object.logical_path.ends_with(".env")));
        assert!(!manifest
            .objects
            .iter()
            .any(|object| object.logical_path.contains("history-only.txt")
                || object.owner_id == "chat-individual"));

        let local_root = destination.path().join("Local workspace");
        fs::create_dir_all(&local_root).unwrap();
        insert_project(
            destination.path(),
            "project-local",
            "Destination only",
            &local_root,
            0,
        );
        insert_thread(
            destination.path(),
            "chat-local",
            "Destination chat",
            &local_root,
            Some("project-local"),
            "{\"text\":\"keep me\"}",
        );
        set_thread_local_fields(
            destination.path(),
            "chat-local",
            r#"{"type":"managed","device":"destination-default"}"#,
            "on-request",
            Some("destination-default-agent.toml"),
            200,
        );
        insert_thread(
            destination.path(),
            "chat-project",
            "Stale project chat",
            &local_root,
            None,
            "{\"text\":\"stale\"}",
        );
        set_thread_local_fields(
            destination.path(),
            "chat-project",
            r#"{"type":"managed","device":"existing-task"}"#,
            "existing-local-approval",
            Some("existing-local-agent.toml"),
            100,
        );
        let destination_host = format!("local:{}", destination.path().display());
        let destination_ui = serde_json::json!({
            "local-projects": { "legacy-local": { "id": "legacy-local", "name": "Destination only", "rootPaths": [local_root.to_string_lossy()] } },
            "app-server-project-id-by-legacy-project-id-by-host": { destination_host.clone(): { "legacy-local": "project-local" } },
            "project-order": ["legacy-local"],
            "queued-follow-ups": { "private": "preserve" },
            "use-copilot-auth-if-available": true
        });
        write_json(
            &destination.path().join(".codex-global-state.json"),
            &destination_ui,
        )
        .unwrap();
        fs::write(destination.path().join("session_index.jsonl"), "{\"id\":\"chat-local\",\"title\":\"Destination chat\"}\n{\"id\":\"chat-project\",\"title\":\"Stale title\"}\n").unwrap();

        let staged_home = stage.path().join("destination");
        snapshot_databases(destination.path(), &staged_home).unwrap();
        fs::copy(
            destination.path().join(".codex-global-state.json"),
            staged_home.join(".codex-global-state.json"),
        )
        .unwrap();
        fs::copy(
            destination.path().join("session_index.jsonl"),
            staged_home.join("session_index.jsonl"),
        )
        .unwrap();
        let destination_full_root = destination.path().join("Restored Full Project");
        config.codex_home = destination.path().to_string_lossy().into_owned();
        config.projectless_root = destination
            .path()
            .join("Projectless imports")
            .to_string_lossy()
            .into_owned();
        config.destination_roots.insert(
            "project-full:0".to_string(),
            destination_full_root.to_string_lossy().into_owned(),
        );
        let incoming_threads: HashSet<String> = manifest
            .threads
            .iter()
            .map(|thread| thread.id.clone())
            .collect();
        let incoming_projects: HashSet<String> = manifest
            .projects
            .iter()
            .map(|project| project.id.clone())
            .collect();
        let mut rollout_paths = HashMap::new();
        for object in manifest
            .objects
            .iter()
            .filter(|object| matches!(object.kind, crate::models::ObjectKind::Rollout))
        {
            let path = staged_home
                .join("sessions")
                .join(format!("{}.jsonl", object.owner_id));
            store.materialize(object, &path).unwrap();
            rollout_paths.insert(
                object.owner_id.clone(),
                destination
                    .path()
                    .join("sessions")
                    .join(format!("{}.jsonl", object.owner_id))
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        let mut attachment_paths: HashMap<String, HashMap<String, String>> = HashMap::new();
        for object in manifest
            .objects
            .iter()
            .filter(|object| matches!(object.kind, crate::models::ObjectKind::Artifact))
        {
            let thread = manifest
                .threads
                .iter()
                .find(|thread| thread.id == object.owner_id)
                .unwrap();
            let reference = thread
                .attachments
                .iter()
                .find(|reference| reference.logical_path == object.logical_path)
                .unwrap();
            let path = destination
                .path()
                .join("imported-attachments")
                .join(&thread.id)
                .join(Path::new(&object.logical_path).file_name().unwrap());
            store.materialize(object, &path).unwrap();
            attachment_paths
                .entry(thread.id.clone())
                .or_default()
                .insert(
                    reference.source_path.clone(),
                    path.to_string_lossy().into_owned(),
                );
        }
        apply_bundle(
            &staged_home,
            &manifest,
            &incoming_threads,
            &incoming_projects,
            &HashSet::new(),
            &HashSet::new(),
            &config,
            &rollout_paths,
            &attachment_paths,
        )
        .unwrap();
        verify_databases(&staged_home).unwrap();

        let state = Connection::open(staged_home.join("state_5.sqlite")).unwrap();
        let ids: HashSet<String> = state
            .prepare("SELECT id FROM threads")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        assert!(ids.contains("chat-local"));
        assert!(ids.contains("chat-project"));
        assert!(ids.contains("chat-history"));
        assert!(ids.contains("chat-projectless"));
        assert!(!ids.contains("chat-individual"));
        assert!(!ids.contains("chat-excluded-project"));
        let existing_local_fields: (String, String, Option<String>) = state
            .query_row(
                "SELECT sandbox_policy, approval_mode, agent_path FROM threads WHERE id='chat-project'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            existing_local_fields,
            (
                r#"{"type":"managed","device":"existing-task"}"#.to_string(),
                "existing-local-approval".to_string(),
                Some("existing-local-agent.toml".to_string())
            )
        );
        let new_local_fields: (String, String, Option<String>) = state
            .query_row(
                "SELECT sandbox_policy, approval_mode, agent_path FROM threads WHERE id='chat-history'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            new_local_fields,
            (
                r#"{"type":"managed","device":"destination-default"}"#.to_string(),
                "on-request".to_string(),
                None
            )
        );
        let restored_cwd: String = state
            .query_row(
                "SELECT cwd FROM threads WHERE id='chat-project'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            PathBuf::from(restored_cwd),
            destination_full_root.join("src")
        );
        let full_database_root: String = state
            .query_row(
                "SELECT path FROM project_roots WHERE project_id='project-full'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(PathBuf::from(full_database_root), destination_full_root);
        let history_database_root: String = state
            .query_row(
                "SELECT path FROM project_roots WHERE project_id='project-history'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            PathBuf::from(history_database_root),
            PathBuf::from(&config.projectless_root)
                .join("History only")
                .join("History Project")
        );

        let history = Connection::open(staged_home.join("thread_history_1.sqlite")).unwrap();
        let restored_json: String = history
            .query_row(
                "SELECT item_json FROM thread_items WHERE thread_id='chat-project'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let restored_item: JsonValue = serde_json::from_str(&restored_json).unwrap();
        assert_eq!(
            restored_item.get("text").and_then(JsonValue::as_str),
            Some(historical_sentence.as_str())
        );
        let restored_attachment = restored_item
            .pointer("/content/0/path")
            .and_then(JsonValue::as_str)
            .unwrap();
        assert_ne!(Path::new(restored_attachment), attachment);
        assert_eq!(
            fs::read(restored_attachment).unwrap(),
            b"portable image bytes"
        );
        let local_count: i64 = history
            .query_row(
                "SELECT count(*) FROM thread_items WHERE thread_id='chat-local'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(local_count, 1);
        history.execute("INSERT INTO thread_items(thread_id, turn_id, item_id, rollout_ordinal, created_at_ms, item_json) VALUES('chat-project', 'turn-2', 'continued-item', 2, 2, '{\"text\":\"continued\"}')", []).unwrap();
        let continued: i64 = history
            .query_row(
                "SELECT count(*) FROM thread_items WHERE thread_id='chat-project'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(continued, 2);

        let restored_ui: JsonValue =
            read_json(&staged_home.join(".codex-global-state.json")).unwrap();
        assert_eq!(
            restored_ui
                .pointer("/queued-follow-ups/private")
                .and_then(JsonValue::as_str),
            Some("preserve")
        );
        assert_eq!(
            restored_ui
                .get("use-copilot-auth-if-available")
                .and_then(JsonValue::as_bool),
            Some(true)
        );
        assert!(restored_ui
            .pointer("/local-projects/legacy-local")
            .is_some());
        assert_eq!(
            restored_ui
                .pointer("/local-projects/legacy-full/rootPaths/0")
                .and_then(JsonValue::as_str),
            Some(destination_full_root.to_string_lossy().as_ref())
        );
        let destination_host_pointer = format!(
            "/app-server-project-id-by-legacy-project-id-by-host/{}/legacy-full",
            destination_host.replace('~', "~0").replace('/', "~1")
        );
        assert_eq!(
            restored_ui
                .pointer(&destination_host_pointer)
                .and_then(JsonValue::as_str),
            Some("project-full")
        );
        let session_index = fs::read_to_string(staged_home.join("session_index.jsonl")).unwrap();
        let project_entries = session_index
            .lines()
            .filter(|line| {
                serde_json::from_str::<JsonValue>(line)
                    .ok()
                    .and_then(|value| {
                        value
                            .get("id")
                            .and_then(JsonValue::as_str)
                            .map(|id| id == "chat-project")
                    })
                    .unwrap_or(false)
            })
            .count();
        assert_eq!(project_entries, 1);
        assert!(session_index.contains("chat-local"));
        assert!(!session_index.contains("chat-individual"));
    }

    #[test]
    fn unsupported_migrations_fail_closed() {
        let dir = tempdir().unwrap();
        for file in ["state_5.sqlite", "thread_history_1.sqlite"] {
            let db = Connection::open(dir.path().join(file)).unwrap();
            db.execute_batch("CREATE TABLE _sqlx_migrations(version INTEGER, success INTEGER);")
                .unwrap();
        }
        let result = inspect(dir.path()).unwrap();
        assert!(!result.supported);
    }

    #[test]
    fn schema55_requires_attachments_and_unknown_versions_explain_the_format() {
        let missing = tempdir().unwrap();
        create_profile_schema(missing.path(), 55);
        let state = Connection::open(missing.path().join("state_5.sqlite")).unwrap();
        state
            .execute_batch("DROP TABLE thread_attachments;")
            .unwrap();
        let result = inspect(missing.path()).unwrap();
        assert!(!result.supported);
        assert!(result
            .explanation
            .contains("Required tables are missing: thread_attachments"));
        assert!(!result.explanation.contains("thread_artifacts"));

        let unknown = tempdir().unwrap();
        create_profile_schema(unknown.path(), 55);
        let state = Connection::open(unknown.path().join("state_5.sqlite")).unwrap();
        state
            .execute("UPDATE _sqlx_migrations SET version = 56", [])
            .unwrap();
        let result = inspect(unknown.path()).unwrap();
        assert!(!result.supported);
        assert!(result
            .explanation
            .contains("Found database migrations 56/6"));
        assert!(!result.explanation.contains("thread_artifacts"));
    }

    #[test]
    fn schema55_artifact_preflight_rejects_collisions_and_unknown_fields_without_mutation() {
        let source = tempdir().unwrap();
        create_profile_schema(source.path(), 55);
        insert_thread(
            source.path(),
            "incoming",
            "Incoming",
            source.path(),
            None,
            "history",
        );
        let original = fixture_manifest(source.path(), vec![]);
        for destination_version in [52, 54, 55] {
            let destination = tempdir().unwrap();
            create_profile_schema(destination.path(), destination_version);
            insert_thread(
                destination.path(),
                "local",
                "Local",
                destination.path(),
                None,
                "local",
            );
            let before: Vec<_> = ["state_5.sqlite", "thread_history_1.sqlite"]
                .into_iter()
                .map(|database| fs::read(destination.path().join(database)).unwrap())
                .collect();
            for malformed in [
                "collision",
                "native_column",
                "unknown_column",
                "native_table",
            ] {
                let mut manifest = original.clone();
                let state_rows = &mut manifest.threads[0].state_rows;
                if malformed == "native_table" {
                    let rows = state_rows.remove("thread_artifacts").unwrap();
                    state_rows.insert("thread_attachments".into(), rows);
                } else {
                    let row = &mut state_rows.get_mut("thread_artifacts").unwrap()[0];
                    if malformed == "native_column" {
                        row.values.remove("artifact_type");
                    }
                    let column = if malformed == "unknown_column" {
                        "future_attachment_field"
                    } else {
                        "attachment_type"
                    };
                    row.values.insert(column.into(), SqlValue::Null);
                }
                let error = apply_bundle(
                    destination.path(),
                    &manifest,
                    &HashSet::from(["incoming".into()]),
                    &HashSet::new(),
                    &HashSet::from(["local".into()]),
                    &HashSet::new(),
                    &crate::settings::default_config(),
                    &HashMap::new(),
                    &HashMap::new(),
                )
                .unwrap_err();
                assert!(
                    matches!(error, SpiceError::UnsupportedCodex(_)),
                    "{malformed}"
                );
                for (index, database) in ["state_5.sqlite", "thread_history_1.sqlite"]
                    .into_iter()
                    .enumerate()
                {
                    assert_eq!(
                        before[index],
                        fs::read(destination.path().join(database)).unwrap(),
                        "{destination_version}: {malformed}"
                    );
                }
            }
        }
    }

    #[test]
    fn quick_inventory_keeps_mapped_projects_and_counts_without_workspace_sizes() {
        let directory = tempdir().unwrap();
        let home = directory.path().join("codex");
        let original = directory.path().join("original-project");
        let mapped = directory.path().join("mapped-project");
        create_fixture_schema(&home);
        fs::create_dir_all(&mapped).unwrap();
        fs::write(mapped.join("notes.txt"), b"project contents").unwrap();
        insert_project(&home, "project", "Project", &original, 0);
        insert_thread(&home, "task", "Task", &original, Some("project"), "{}");
        let mut config = crate::settings::default_config();
        config.codex_home = home.to_string_lossy().into_owned();
        config.projectless_root = directory
            .path()
            .join("projectless")
            .to_string_lossy()
            .into_owned();
        config.cloud_root = directory
            .path()
            .join("cloud")
            .to_string_lossy()
            .into_owned();
        config.projects_root.clear();
        config
            .source_roots
            .insert("project:0".into(), mapped.to_string_lossy().into_owned());
        let engine = crate::engine::Engine::new(directory.path().join("app-data")).unwrap();
        let quick = engine.list_content_quick(&config).unwrap();
        let detailed = engine.list_content(&config).unwrap();
        assert_eq!(quick.threads.len(), 1);
        assert_eq!(quick.projects[0].thread_count, 1);
        assert_eq!(
            quick.projects[0].local_roots,
            detailed.projects[0].local_roots
        );
        assert_eq!(
            quick.projects[0].local_roots,
            vec![mapped.to_string_lossy().into_owned()]
        );
        assert_eq!(quick.projects[0].estimated_bytes, 0);
        assert_eq!(
            detailed.projects[0].estimated_bytes,
            b"project contents".len() as u64
        );
    }

    #[test]
    fn internal_guardian_records_are_omitted_without_losing_user_or_helper_history() {
        let home = tempdir().unwrap();
        create_fixture_schema(home.path());
        for (id, title) in [
            ("parent", "Investigate guardian behavior"),
            ("helper", "Review recent changes"),
            ("guardian", "Internal approval fixture"),
            ("guardian-child", "Internal child fixture"),
            ("unknown", "A future kind of task"),
        ] {
            insert_thread(home.path(), id, title, home.path(), None, "{}");
        }
        let selection = crate::settings::default_config().selection;
        let mut old_export =
            export_selected(home.path(), home.path(), &selection, home.path()).unwrap();
        let state = Connection::open(home.path().join("state_5.sqlite")).unwrap();
        for (id, source) in [
            ("guardian", r#"{"subagent":{"other":"guardian"}}"#),
            (
                "guardian-child",
                r#"{"subagent":{"thread_spawn":{"parent_thread_id":"guardian"}}}"#,
            ),
            (
                "helper",
                r#"{"subagent":{"thread_spawn":{"parent_thread_id":"parent"}}}"#,
            ),
            ("unknown", r#"{"subagent":{"other":"guardian_helper"}}"#),
        ] {
            state
                .execute("UPDATE threads SET source=?1 WHERE id=?2", [source, id])
                .unwrap();
            let row = old_export
                .threads
                .iter_mut()
                .find(|thread| thread.id == id)
                .unwrap()
                .state_rows
                .get_mut("threads")
                .unwrap()
                .first_mut()
                .unwrap();
            row.values
                .insert("source".into(), SqlValue::Text(source.into()));
        }
        let expected = HashSet::from(["parent", "helper", "unknown"]);
        let catalog = list_content(home.path()).unwrap();
        assert_eq!(
            catalog
                .threads
                .iter()
                .map(|thread| thread.id.as_str())
                .collect::<HashSet<_>>(),
            expected
        );
        let export = export_selected(home.path(), home.path(), &selection, home.path()).unwrap();
        assert_eq!(
            export
                .threads
                .iter()
                .map(|thread| thread.id.as_str())
                .collect::<HashSet<_>>(),
            expected
        );
        assert!(export
            .pending_files
            .iter()
            .all(|file| expected.contains(file.owner_id.as_str())));
        // Older snapshots must treat these records as outside the transfer selection,
        // not as source deletions when a subsequent snapshot stops exporting them.
        assert_eq!(
            selection_excluded_thread_ids(&selection, &old_export.threads),
            HashSet::from(["guardian".into(), "guardian-child".into()])
        );
        assert_eq!(
            state
                .query_row("SELECT count(*) FROM threads", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            5
        );
        assert!(!is_internal_source("guardian"));
        assert!(!is_internal_source(
            r#"{"subagent":{"other":"guardian_helper"}}"#
        ));
        assert!(!is_internal_source(r#"{"source":"guardian"}"#));
    }

    #[test]
    fn long_display_labels_are_bounded_while_database_titles_remain_lossless() {
        let home = tempdir().unwrap();
        create_fixture_schema(home.path());
        let original = format!("  Long title\n\t{}", "界".repeat(400));
        insert_thread(home.path(), "long", &original, home.path(), None, "{}");
        let catalog = list_content(home.path()).unwrap();
        let task = &catalog.threads[0];
        assert_eq!(task.title.chars().count(), DISPLAY_TITLE_LIMIT);
        assert_eq!(task.preview.chars().count(), DISPLAY_PREVIEW_LIMIT);
        assert!(task.title.starts_with("Long title "));
        assert!(task.title.ends_with('…'));
        assert!(!task.title.contains('\n'));
        assert_eq!(display_thread_title(" \n\t"), "Untitled chat");
        assert_eq!(display_thread_title("A\u{202e}B\u{2014}C"), "AB-C");
        let export = export_selected(
            home.path(),
            home.path(),
            &crate::settings::default_config().selection,
            home.path(),
        )
        .unwrap();
        assert_eq!(export.threads[0].title, task.title);
        assert_eq!(
            row_string(&export.threads[0].state_rows["threads"][0], "title"),
            Some(original.as_str())
        );
        assert_eq!(
            row_string(&export.threads[0].state_rows["threads"][0], "name"),
            Some(original.as_str())
        );
    }

    #[test]
    fn agent_histories_inherit_explicit_parent_project_and_exclusions() {
        let home = tempdir().unwrap();
        create_fixture_schema(home.path());
        let root = home.path().join("project");
        fs::create_dir_all(&root).unwrap();
        insert_project(home.path(), "project", "Project", &root, 0);
        for id in ["parent", "agent", "nested-agent"] {
            insert_thread(home.path(), id, "", &root, None, "{\"text\":\"fixture\"}");
        }
        let state = Connection::open(home.path().join("state_5.sqlite")).unwrap();
        state
            .execute(
                "UPDATE threads SET name=' ', title='Parent title' WHERE id='parent'",
                [],
            )
            .unwrap();
        state
            .execute(
                "UPDATE threads SET source=?1 WHERE id='agent'",
                [r#"{"subagent":{"thread_spawn":{"parent_thread_id":"parent"}}}"#],
            )
            .unwrap();
        state.execute("INSERT INTO thread_spawn_edges(parent_thread_id, child_thread_id, status) VALUES('agent', 'nested-agent', 'completed')", []).unwrap();
        write_json(&home.path().join(".codex-global-state.json"), &serde_json::json!({
            "thread-project-assignments": {"parent": "legacy-project"},
            "app-server-project-id-by-legacy-project-id-by-host": {"local": {"legacy-project": "project"}}
        })).unwrap();

        let catalog = list_content(home.path()).unwrap();
        assert_eq!(catalog.projects[0].thread_count, 3);
        assert!(catalog
            .threads
            .iter()
            .all(|thread| thread.project_id.as_deref() == Some("project") && !thread.projectless));
        assert_eq!(
            catalog
                .threads
                .iter()
                .find(|thread| thread.id == "parent")
                .unwrap()
                .title,
            "Parent title"
        );
        assert!(catalog
            .threads
            .iter()
            .all(|thread| !thread.title.trim().is_empty()));

        let mut selection = crate::settings::default_config().selection;
        let export = export_selected(
            home.path(),
            home.path(),
            &selection,
            &home.path().join("projectless"),
        )
        .unwrap();
        assert_eq!(export.threads.len(), 3);
        assert!(
            export.warnings.is_empty(),
            "Explicit child project histories must not warn about projectless folders: {:?}",
            export.warnings
        );
        assert!(export
            .pending_files
            .iter()
            .all(|file| !matches!(file.kind, PendingFileKind::ProjectlessRoot)));

        selection.excluded_thread_ids.push("parent".into());
        assert_eq!(
            selection_excluded_thread_ids(&selection, &export.threads),
            HashSet::from(["parent".into(), "agent".into(), "nested-agent".into()])
        );
        let excluded = export_selected(
            home.path(),
            home.path(),
            &selection,
            &home.path().join("projectless"),
        )
        .unwrap();
        assert!(
            excluded.threads.is_empty(),
            "Parent exclusion must exclude nested agent records"
        );
        assert!(excluded.pending_files.is_empty());

        selection.excluded_thread_ids.clear();
        selection
            .project_modes
            .insert("project".into(), ProjectMode::Excluded);
        let excluded = export_selected(
            home.path(),
            home.path(),
            &selection,
            &home.path().join("projectless"),
        )
        .unwrap();
        assert!(
            excluded.threads.is_empty(),
            "Project exclusion must exclude all its agent records"
        );
    }

    #[test]
    fn explicit_project_assignment_overrides_stale_projectless_marker() {
        let home = tempdir().unwrap();
        create_fixture_schema(home.path());
        insert_project(home.path(), "project", "Project", home.path(), 0);
        insert_thread(
            home.path(),
            "assigned",
            "Assigned",
            home.path(),
            Some("project"),
            "{}",
        );
        write_json(
            &home.path().join(".codex-global-state.json"),
            &serde_json::json!({"projectless-thread-ids": ["assigned"]}),
        )
        .unwrap();
        let catalog = list_content(home.path()).unwrap();
        assert!(!catalog.threads[0].projectless);
        let export = export_selected(
            home.path(),
            home.path(),
            &crate::settings::default_config().selection,
            &home.path().join("elsewhere"),
        )
        .unwrap();
        assert!(!export.threads[0].projectless);
        assert!(export.warnings.is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn verbatim_source_paths_map_without_the_source_drive() {
        assert_eq!(
            relative_under(
                Path::new(r"\\?\X:\source\Project\child"),
                Path::new(r"x:\source\project")
            ),
            Some(PathBuf::from("child"))
        );
        assert_eq!(
            relative_under(
                Path::new(r"\\?\UNC\absent-server\share\Project\child"),
                Path::new(r"\\absent-server\share\Project")
            ),
            Some(PathBuf::from("child"))
        );
        assert_eq!(
            relative_under(
                Path::new(r"\\?\X:\source\Project-other"),
                Path::new(r"X:\source\Project")
            ),
            None
        );
    }

    #[test]
    fn local_operational_fields_do_not_change_thread_fingerprint() {
        let mut a = BTreeMap::new();
        a.insert(
            "threads".to_string(),
            vec![DatabaseRow {
                values: BTreeMap::from([
                    ("id".to_string(), SqlValue::Text("one".into())),
                    ("cwd".to_string(), SqlValue::Text("C:\\one".into())),
                    (
                        "rollout_path".to_string(),
                        SqlValue::Text("C:\\a.jsonl".into()),
                    ),
                    (
                        "sandbox_policy".to_string(),
                        SqlValue::Text(r#"{"type":"disabled"}"#.into()),
                    ),
                    ("approval_mode".to_string(), SqlValue::Text("never".into())),
                    (
                        "agent_path".to_string(),
                        SqlValue::Text("C:\\agent.toml".into()),
                    ),
                ]),
            }],
        );
        let mut b = a.clone();
        let values = &mut b.get_mut("threads").unwrap()[0].values;
        values.insert("cwd".into(), SqlValue::Text("D:\\two".into()));
        values.insert(
            "sandbox_policy".into(),
            SqlValue::Text(r#"{"type":"managed"}"#.into()),
        );
        values.insert("approval_mode".into(), SqlValue::Text("on-request".into()));
        values.insert("agent_path".into(), SqlValue::Text("D:\\agent.toml".into()));
        assert_eq!(
            hash_thread_rows(&a, &BTreeMap::new()).unwrap(),
            hash_thread_rows(&b, &BTreeMap::new()).unwrap()
        );
    }

    #[test]
    fn rollout_image_paths_rewrite_portably_and_leave_other_paths_untouched() {
        let directory = tempdir().unwrap();
        let source_image = directory.path().join("source image.png");
        let restored_image = directory.path().join("restored image.png");
        fs::write(&source_image, b"image").unwrap();
        let rollout = directory.path().join("rollout.jsonl");
        let line = serde_json::json!({
            "content": [
                { "type": "localImage", "path": source_image.to_string_lossy() },
                { "type": "text", "path": "D:\\historical\\mention.txt" }
            ],
            "changes": [{ "path": "src/main.rs" }]
        });
        fs::write(&rollout, format!("{line}\nnot-json\n")).unwrap();
        let before = portable_rollout_fingerprint(&rollout).unwrap();
        let replacements = HashMap::from([(
            source_image.to_string_lossy().into_owned(),
            restored_image.to_string_lossy().into_owned(),
        )]);

        assert_eq!(
            rewrite_rollout_local_image_paths(&rollout, &replacements).unwrap(),
            1
        );
        assert_eq!(portable_rollout_fingerprint(&rollout).unwrap(), before);
        let content = fs::read_to_string(&rollout).unwrap();
        let first: JsonValue = serde_json::from_str(content.lines().next().unwrap()).unwrap();
        assert_eq!(
            first.pointer("/content/0/path").and_then(JsonValue::as_str),
            Some(restored_image.to_string_lossy().as_ref())
        );
        assert_eq!(
            first.pointer("/content/1/path").and_then(JsonValue::as_str),
            Some("D:\\historical\\mention.txt")
        );
        assert_eq!(
            first.pointer("/changes/0/path").and_then(JsonValue::as_str),
            Some("src/main.rs")
        );
        assert_eq!(content.lines().nth(1), Some("not-json"));
    }

    #[test]
    fn build_gate_accepts_only_the_validated_codex_build() {
        let compatibility = CompatibilityInfo {
            supported: true,
            adapter: ADAPTER_NAME.to_string(),
            state_migration: Some(52),
            history_migration: Some(6),
            schema_fingerprint: crate::compatibility::PROFILES[0].fingerprint.to_string(),
            explanation: "schema supported".to_string(),
        };
        assert!(with_build_gate(compatibility.clone(), Some("codex-cli 0.153.4")).supported);
        assert!(!with_build_gate(compatibility.clone(), Some("codex-cli 0.154.0")).supported);
        assert!(!with_build_gate(compatibility, None).supported);
    }

    fn fixture_manifest(home: &Path, excluded: Vec<String>) -> SnapshotManifest {
        let mut config = crate::settings::default_config();
        config.codex_home = home.to_string_lossy().into_owned();
        config.projectless_root = home.join("workspaces").to_string_lossy().into_owned();
        config.selection.excluded_thread_ids = excluded;
        config.selection.default_project_mode = ProjectMode::HistoryOnly;
        let staging = tempdir().unwrap();
        snapshot_databases(home, staging.path()).unwrap();
        let export = export_selected(
            home,
            staging.path(),
            &config.selection,
            Path::new(&config.projectless_root),
        )
        .unwrap();
        let store = ObjectStore::new(staging.path().join("objects")).unwrap();
        let mut manifest = snapshot::build_manifest(&config, export, &store, None).unwrap();
        manifest.codex_version = Some(
            crate::compatibility::profile(&manifest.compatibility)
                .unwrap()
                .runtime
                .into(),
        );
        manifest
    }

    fn import_fixture(
        home: &Path,
        manifest: &SnapshotManifest,
        deleted: HashSet<String>,
    ) -> Result<()> {
        let mut config = crate::settings::default_config();
        config.codex_home = home.to_string_lossy().into_owned();
        config.projectless_root = home.join("workspaces").to_string_lossy().into_owned();
        for thread in &manifest.threads {
            let source = PathBuf::from(
                row_string(&thread.state_rows["threads"][0], "rollout_path").unwrap(),
            );
            let destination = home.join("sessions").join(format!("{}.jsonl", thread.id));
            if source != destination && source.is_file() {
                fs::create_dir_all(destination.parent().unwrap())?;
                fs::copy(source, destination)?;
            }
        }
        apply_bundle(
            home,
            manifest,
            &manifest.threads.iter().map(|t| t.id.clone()).collect(),
            &manifest.projects.iter().map(|p| p.id.clone()).collect(),
            &deleted,
            &HashSet::new(),
            &config,
            &manifest
                .threads
                .iter()
                .map(|t| {
                    (
                        t.id.clone(),
                        home.join("sessions")
                            .join(format!("{}.jsonl", t.id))
                            .to_string_lossy()
                            .into_owned(),
                    )
                })
                .collect(),
            &HashMap::new(),
        )
    }

    #[test]
    fn exact_schema_matrix_preserves_history_identity_schema_and_repeated_imports() {
        for source_version in [52, 54] {
            for destination_version in [52, 54] {
                let source = tempdir().unwrap();
                let destination = tempdir().unwrap();
                create_profile_schema(source.path(), source_version);
                create_profile_schema(destination.path(), destination_version);
                insert_thread(
                    source.path(),
                    "portable",
                    "Portable chat",
                    &source.path().join("workspaces/chat"),
                    None,
                    r#"{"text":"history with C:\\old\\path"}"#,
                );
                insert_thread(
                    source.path(),
                    "excluded",
                    "Private",
                    source.path(),
                    None,
                    "private-history",
                );
                insert_thread(
                    destination.path(),
                    "local",
                    "Local only",
                    destination.path(),
                    None,
                    "local-history",
                );
                let manifest = fixture_manifest(source.path(), vec!["excluded".into()]);
                validate_snapshot_source(&manifest).unwrap();
                assert!(
                    transfer_issues(&manifest, &inspect(destination.path()).unwrap()).is_empty()
                );
                let before = inspect(destination.path()).unwrap();
                let ledger_before = query_rows_eq(
                    &open_read_only(&destination.path().join("state_5.sqlite")).unwrap(),
                    "_sqlx_migrations",
                    "description",
                    "fixture",
                )
                .unwrap();
                import_fixture(destination.path(), &manifest, HashSet::new()).unwrap();
                import_fixture(destination.path(), &manifest, HashSet::new()).unwrap();
                verify_databases(destination.path()).unwrap();
                let after = inspect(destination.path()).unwrap();
                assert_eq!(before.schema_fingerprint, after.schema_fingerprint);
                assert_eq!(before.state_migration, after.state_migration);
                let state = open_read_only(&destination.path().join("state_5.sqlite")).unwrap();
                assert_eq!(
                    state
                        .query_row("SELECT count(*) FROM threads", [], |r| r.get::<_, i64>(0))
                        .unwrap(),
                    2
                );
                assert_eq!(
                    serde_json::to_value(ledger_before).unwrap(),
                    serde_json::to_value(
                        query_rows_eq(&state, "_sqlx_migrations", "description", "fixture")
                            .unwrap()
                    )
                    .unwrap()
                );
                let reexport = fixture_manifest(destination.path(), vec!["local".into()]);
                assert_eq!(
                    reexport.threads[0].fingerprint, manifest.threads[0].fingerprint,
                    "{source_version} -> {destination_version}"
                );
                assert_eq!(
                    serde_json::to_value(&reexport.threads[0].history_rows).unwrap(),
                    serde_json::to_value(&manifest.threads[0].history_rows).unwrap()
                );
                assert_eq!(
                    reexport.threads[0].history_rows["thread_realtime_items"].len(),
                    1,
                    "Projection cleanup trigger must not delete incoming realtime history"
                );
                // Back again, with the same identity and no duplicated records.
                import_fixture(source.path(), &reexport, HashSet::new()).unwrap();
                let round_trip = fixture_manifest(source.path(), vec!["excluded".into()]);
                assert_eq!(
                    round_trip.threads[0].fingerprint,
                    manifest.threads[0].fingerprint
                );
                // A continued history item remains portable on a subsequent handoff.
                let history =
                    Connection::open(source.path().join("thread_history_1.sqlite")).unwrap();
                history.execute("INSERT INTO thread_items(thread_id, turn_id, item_id, rollout_ordinal, created_at_ms, item_json) VALUES('portable','turn-2','continued',2,2,'continued history')", []).unwrap();
                drop(history);
                let continued = fixture_manifest(source.path(), vec!["excluded".into()]);
                import_fixture(destination.path(), &continued, HashSet::new()).unwrap();
                assert_eq!(
                    fixture_manifest(destination.path(), vec!["local".into()]).threads[0]
                        .fingerprint,
                    continued.threads[0].fingerprint
                );
            }
        }
    }

    #[test]
    fn non_null_new_fields_block_reverse_transfer_before_any_mutation() {
        let source = tempdir().unwrap();
        let destination = tempdir().unwrap();
        create_profile_schema(source.path(), 54);
        create_profile_schema(destination.path(), 52);
        insert_thread(
            source.path(),
            "new",
            "New-feature chat",
            source.path(),
            None,
            "source",
        );
        insert_thread(
            destination.path(),
            "local",
            "Local",
            destination.path(),
            None,
            "local",
        );
        let state = Connection::open(source.path().join("state_5.sqlite")).unwrap();
        for (originator, daybreak) in [(Some("desktop"), None), (None, Some(0)), (None, Some(1))] {
            state
                .execute(
                    "UPDATE threads SET originator=?1, daybreak_enabled=?2",
                    rusqlite::params![originator, daybreak],
                )
                .unwrap();
            let manifest = fixture_manifest(source.path(), vec![]);
            let issues = transfer_issues(&manifest, &inspect(destination.path()).unwrap());
            assert_eq!(issues.len(), 1);
            assert!(issues[0].contains("New-feature chat"));
            let before = fs::read(destination.path().join("state_5.sqlite")).unwrap();
            assert!(import_fixture(
                destination.path(),
                &manifest,
                HashSet::from(["local".into()])
            )
            .is_err());
            assert_eq!(
                before,
                fs::read(destination.path().join("state_5.sqlite")).unwrap()
            );
            assert!(fixture_manifest(source.path(), vec!["new".into()])
                .threads
                .is_empty());
        }
    }

    #[test]
    fn newer_destination_values_survive_an_older_source_and_transfer_between_new_profiles() {
        let older = tempdir().unwrap();
        let newer = tempdir().unwrap();
        let peer = tempdir().unwrap();
        create_profile_schema(older.path(), 52);
        create_profile_schema(newer.path(), 54);
        create_profile_schema(peer.path(), 54);
        for home in [older.path(), newer.path()] {
            insert_thread(home, "same", "Chat", home, None, "history");
        }
        let state = Connection::open(newer.path().join("state_5.sqlite")).unwrap();
        state
            .execute(
                "UPDATE threads SET originator='desktop', daybreak_enabled=1",
                [],
            )
            .unwrap();
        drop(state);
        import_fixture(
            newer.path(),
            &fixture_manifest(older.path(), vec![]),
            HashSet::new(),
        )
        .unwrap();
        let manifest = fixture_manifest(newer.path(), vec![]);
        assert!(
            matches!(&manifest.threads[0].state_rows["threads"][0].values["originator"], SqlValue::Text(value) if value == "desktop")
        );
        assert!(matches!(
            manifest.threads[0].state_rows["threads"][0].values["daybreak_enabled"],
            SqlValue::Integer(1)
        ));
        import_fixture(peer.path(), &manifest, HashSet::new()).unwrap();
        assert_eq!(
            fixture_manifest(peer.path(), vec![]).threads[0].fingerprint,
            manifest.threads[0].fingerprint
        );
        // A real explicit NULL from a newer source is an intentional update.
        let mut cleared = manifest;
        cleared.threads[0].state_rows.get_mut("threads").unwrap()[0]
            .values
            .insert("originator".into(), SqlValue::Null);
        import_fixture(peer.path(), &cleared, HashSet::new()).unwrap();
        assert!(matches!(
            fixture_manifest(peer.path(), vec![]).threads[0].state_rows["threads"][0].values
                ["originator"],
            SqlValue::Null
        ));
    }

    #[test]
    fn rejects_trigger_drift_unknown_columns_and_build_schema_mismatches() {
        let home = tempdir().unwrap();
        create_profile_schema(home.path(), 54);
        let valid = inspect(home.path()).unwrap();
        assert!(with_build_gate(valid.clone(), Some("0.154.0-alpha.6.2")).supported);
        assert!(!with_build_gate(valid.clone(), Some("0.153.4")).supported);
        assert!(!with_build_gate(valid, Some("0.154.1")).supported);
        let state = Connection::open(home.path().join("state_5.sqlite")).unwrap();
        state
            .execute_batch("DROP TRIGGER threads_updated_at_ms_after_update")
            .unwrap();
        assert!(
            !inspect(home.path()).unwrap().supported,
            "Tables alone cannot validate a schema"
        );
        let row = DatabaseRow {
            values: BTreeMap::from([("unknown_future_column".into(), SqlValue::Null)]),
        };
        assert!(
            validate_row_columns(&state, "threads", &[row]).is_err(),
            "Even unknown NULL columns need a policy"
        );
    }

    #[test]
    fn revalidates_claimed_source_support_and_recognizes_old_snapshot_profile() {
        let home = tempdir().unwrap();
        create_profile_schema(home.path(), 52);
        insert_thread(
            home.path(),
            "portable",
            "Chat",
            home.path(),
            None,
            "history",
        );
        let mut manifest = fixture_manifest(home.path(), vec![]);
        manifest.schema_version = 1;
        manifest.compatibility.adapter = "codex-state-v5.52/history-v1.6".into();
        validate_snapshot_source(&manifest).unwrap();
        manifest.compatibility.schema_fingerprint =
            crate::compatibility::PROFILES[0].legacy_fingerprints[0].into();
        validate_snapshot_source(&manifest).unwrap();
        manifest.compatibility.supported = false;
        validate_snapshot_source(&manifest).unwrap();
        manifest.compatibility.supported = true;
        manifest.compatibility.schema_fingerprint = "unverified".into();
        assert!(validate_snapshot_source(&manifest).is_err());
    }
}
