//! Bounded, read-only profile diagnostics and an allowlisted Pull journal.
//! Never serialize Codex rows, conversation text, config files or error messages.
use crate::{codex, error::SpiceError, models::*, platform, settings, util};
use chrono::Utc;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

const MAX_ROWS: usize = 50_000;
const MAX_REFERENCES: usize = 2_000;
const MAX_JOURNALS: usize = 20;
const MAX_JSON_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub severity: String,
    pub title: String,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsReport {
    pub schema_version: u32,
    pub generated_at: String,
    pub summary: String,
    pub findings: Vec<Finding>,
    pub report: Value,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileCounts {
    pub path: String,
    pub canonical_path: Option<String>,
    pub path_id: String,
    pub exists: bool,
    pub state_database: DatabaseCounts,
    pub history_database: DatabaseCounts,
    pub sidebar: SidebarCounts,
    pub compatibility: Value,
    pub transcripts_checked: usize,
    pub transcripts_present: usize,
    pub transcripts_missing: usize,
    pub transcripts_outside_profile: usize,
    pub transcript_checks_limited: bool,
    pub missing_project_references: Option<u64>,
    pub missing_history_thread_references: Option<u64>,
    pub visibility_counts: std::collections::BTreeMap<String, u64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseCounts {
    pub exists: bool,
    pub status: String,
    pub migration: Option<i64>,
    pub counts: std::collections::BTreeMap<String, u64>,
    pub counts_limited: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SidebarCounts {
    pub exists: bool,
    pub status: String,
    pub local_projects: usize,
    pub project_order: usize,
    pub thread_project_assignments: usize,
    pub projectless_threads: usize,
    pub project_thread_orders: usize,
    pub missing_thread_references: Option<usize>,
    pub checks_limited: bool,
    pub mapped_hosts: usize,
    pub mapped_projects: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullJournal {
    pub schema_version: u32,
    pub id: String,
    pub engine_version: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub status: String,
    pub phase: String,
    pub phases: Vec<Phase>,
    pub operation_id: Option<String>,
    pub source_snapshot: Option<Value>,
    #[serde(default)]
    pub destination_codex_version: Option<String>,
    pub configured_profile: ProfileCounts,
    pub post_verification: Option<ProfileCounts>,
    pub selected_for_restore: Option<Value>,
    pub restored_records: Option<Value>,
    pub recovery_id: Option<String>,
    pub failure_category: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Phase {
    pub phase: String,
    pub at: String,
}

/// Persistence is best effort: diagnostics must never turn a successful Pull into a failure.
pub struct PullLog {
    path: PathBuf,
    journal: PullJournal,
}

impl PullLog {
    pub fn start(data: &Path, config: &AppConfig, operation_id: Option<&str>, phase: &str) -> Self {
        let id = format!(
            "{}-{}",
            Utc::now().format("%Y%m%dT%H%M%S%3fZ"),
            uuid::Uuid::new_v4().simple()
        );
        let root = data.join("pull-diagnostics");
        let journal = PullJournal {
            schema_version: 1,
            id: id.clone(),
            engine_version: env!("CARGO_PKG_VERSION").into(),
            started_at: now(),
            finished_at: None,
            status: "inProgress".into(),
            phase: phase.into(),
            phases: vec![Phase {
                phase: phase.into(),
                at: now(),
            }],
            operation_id: operation_id.map(opaque),
            source_snapshot: None,
            destination_codex_version: None,
            configured_profile: inspect_profile(Path::new(&config.codex_home)),
            post_verification: None,
            selected_for_restore: None,
            restored_records: None,
            recovery_id: None,
            failure_category: None,
        };
        let log = Self {
            path: root.join(format!("{id}.json")),
            journal,
        };
        log.persist();
        retain_journals(&root);
        log
    }
    fn persist(&self) {
        let _ = util::write_json(&self.path, &self.journal);
    }
    pub fn phase(&mut self, phase: &str) {
        self.journal.phase = phase.into();
        self.journal.phases.push(Phase {
            phase: phase.into(),
            at: now(),
        });
        self.persist();
    }
    pub fn source(&mut self, source: &SnapshotManifest) {
        self.journal.source_snapshot = Some(source_metadata(source));
        self.persist();
    }
    pub fn destination_version(&mut self, version: Option<&str>) {
        self.journal.destination_codex_version = safe_version(version);
        self.persist();
    }
    pub fn selection(&mut self, threads: &HashSet<String>, projects: &HashSet<String>) {
        self.journal.selected_for_restore =
            Some(json!({"threads":threads.len(),"projects":projects.len()}));
        self.persist();
    }
    pub fn verified(
        &mut self,
        config: &AppConfig,
        threads: &HashSet<String>,
        projects: &HashSet<String>,
    ) {
        self.journal.post_verification = Some(inspect_profile(Path::new(&config.codex_home)));
        self.journal.restored_records = Some(matching_records(
            Path::new(&config.codex_home),
            threads,
            projects,
        ));
        self.phase("postVerificationComplete");
    }
    pub fn recovery(&mut self, id: &str) {
        self.journal.recovery_id = Some(opaque(id));
        self.persist();
    }
    pub fn finish<T>(&mut self, result: &crate::error::Result<T>, success: &str) {
        self.journal.status = if result.is_ok() {
            success.into()
        } else {
            "failed".into()
        };
        self.journal.finished_at = Some(now());
        self.journal.failure_category = result.as_ref().err().map(|e| failure_category(e).into());
        self.persist();
    }
}

fn now() -> String {
    Utc::now().to_rfc3339()
}
fn opaque(value: &str) -> String {
    format!("sha256:{}", util::sha256_bytes(value.as_bytes()))
}

fn failure_category(error: &SpiceError) -> &'static str {
    match error {
        SpiceError::User(message) if message.starts_with("Settings changed after") => {
            "settingsChanged"
        }
        SpiceError::User(message)
            if message.starts_with("Codex or a destination project changed after") =>
        {
            "destinationChanged"
        }
        SpiceError::User(message)
            if message.starts_with("The visible cloud history changed after")
                || message.starts_with("The selected cloud branch is no longer") =>
        {
            "cloudHeadsChanged"
        }
        SpiceError::User(message)
            if message
                .starts_with("The source snapshot or destination Codex version changed after") =>
        {
            "transferCompatibilityChanged"
        }
        SpiceError::User(message) if message.starts_with("This preview expired.") => {
            "previewExpired"
        }
        SpiceError::User(message) if message.starts_with("Choose which version to keep for") => {
            "conflictChoiceRequired"
        }
        SpiceError::User(message)
            if message.starts_with("Choose a destination for every incoming project root") =>
        {
            "destinationMappingRequired"
        }
        SpiceError::MissingPath(_) => "missingPath",
        SpiceError::CodexRunning => "codexRunning",
        SpiceError::CodexReopened => "codexReopened",
        SpiceError::UnsupportedCodex(_) => "unsupportedCodex",
        SpiceError::CorruptSnapshot(_) => "corruptSnapshot",
        SpiceError::Cancelled => "cancelled",
        SpiceError::PendingRecovery => "pendingRecovery",
        SpiceError::Database(_) => "database",
        SpiceError::Io(_) => "io",
        SpiceError::Json(_) => "invalidMetadata",
        _ => "validationOrOperation",
    }
}

fn redact_path(path: &Path) -> String {
    let value = path.to_string_lossy().replace('\\', "/");
    if let Some(home) = dirs::home_dir() {
        let home = home.to_string_lossy().replace('\\', "/");
        if value.eq_ignore_ascii_case(&home) {
            return "%USERPROFILE%".into();
        }
        if value
            .to_lowercase()
            .starts_with(&(home.to_lowercase() + "/"))
        {
            return format!("%USERPROFILE%{}", &value[home.len()..]);
        }
    }
    // Also redact another user's home if a copied configuration points there.
    let parts: Vec<_> = value.split('/').collect();
    if parts.len() > 3 && (parts[1].eq_ignore_ascii_case("users") || parts[1] == "home") {
        return format!("{}/Users/%USER%/{}", parts[0], parts[3..].join("/"));
    }
    value
}

fn local_file(path: &Path, max_bytes: u64) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() || meta.len() > max_bytes {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // Do not hydrate cloud placeholders (OFFLINE, RECALL_ON_OPEN, RECALL_ON_DATA_ACCESS).
        if meta.file_attributes() & (0x1000 | 0x40000 | 0x400000) != 0 {
            return false;
        }
    }
    true
}

fn read_bounded<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    if !local_file(path, MAX_JSON_BYTES) {
        return None;
    }
    let file = fs::File::open(path).ok()?;
    serde_json::from_reader(std::io::BufReader::new(file)).ok()
}

fn open_database(path: &Path) -> Option<Connection> {
    if !local_file(path, u64::MAX) {
        return None;
    }
    let db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    db.busy_timeout(Duration::from_millis(200)).ok()?;
    db.execute_batch("PRAGMA query_only=ON;").ok()?;
    Some(db)
}

fn count(db: &Connection, query: &str) -> Option<u64> {
    db.query_row(query, [], |row| row.get::<_, i64>(0))
        .ok()
        .map(|n| n.max(0) as u64)
}

fn db_counts(path: &Path, tables: &[&str]) -> (DatabaseCounts, Option<Connection>) {
    let mut result = DatabaseCounts {
        exists: path.is_file(),
        status: "missing".into(),
        ..Default::default()
    };
    let Some(db) = open_database(path) else {
        if result.exists {
            result.status = "unavailable".into();
        }
        return (result, None);
    };
    result.status = "readable".into();
    result.migration = db
        .query_row(
            "SELECT max(version) FROM _sqlx_migrations WHERE success=1",
            [],
            |r| r.get(0),
        )
        .ok()
        .flatten();
    for table in tables {
        // The only SQL identifiers interpolated here are compile-time allowlisted table names.
        if let Some(n) = count(
            &db,
            &format!(
                "SELECT count(*) FROM (SELECT 1 FROM {table} LIMIT {})",
                MAX_ROWS + 1
            ),
        ) {
            result
                .counts
                .insert((*table).into(), n.min(MAX_ROWS as u64));
            result.counts_limited |= n > MAX_ROWS as u64;
        } else {
            result.status = "partial".into();
        }
    }
    (result, Some(db))
}

pub fn inspect_profile(home: &Path) -> ProfileCounts {
    let canonical = dunce::canonicalize(home).ok();
    let identity = canonical
        .as_deref()
        .unwrap_or(home)
        .to_string_lossy()
        .to_lowercase();
    let (state_database, state) = db_counts(
        &home.join("state_5.sqlite"),
        &["threads", "projects", "project_roots", "thread_sections"],
    );
    let (history_database, history) = db_counts(
        &home.join("thread_history_1.sqlite"),
        &[
            "thread_items",
            "thread_realtime_items",
            "thread_turns",
            "thread_history_projection_state",
        ],
    );
    let mut result = ProfileCounts {
        path: redact_path(home),
        canonical_path: canonical.as_deref().map(redact_path),
        path_id: opaque(&identity),
        exists: home.is_dir(),
        state_database,
        history_database,
        ..Default::default()
    };
    // This checks only schema metadata, never history contents.
    if state.is_some() && history.is_some() {
        if let Ok(info) = codex::inspect(home) {
            result.compatibility = json!({"supported":info.supported,"adapter":codex::ADAPTER_NAME,
                "stateMigration":info.state_migration,"historyMigration":info.history_migration,"schemaFingerprint":info.schema_fingerprint});
        }
    }
    if let Some(db) = state.as_ref() {
        for (name, expression) in [
            ("archivedThreads", "archived != 0"),
            ("unarchivedThreads", "archived = 0"),
            ("threadsWithoutPreview", "preview = ''"),
            ("threadsWithoutUserEvent", "has_user_event = 0"),
            ("projectAssociatedThreads", "project_id IS NOT NULL"),
            ("desktopOriginThreads", "originator = 'desktop'"),
            (
                "otherOriginThreads",
                "originator IS NOT NULL AND originator != 'desktop'",
            ),
            ("cliSourceThreads", "source = 'cli'"),
            ("subagentSourceThreads", "source LIKE '%subagent%'"),
            ("guardianSourceThreads", "source LIKE '%guardian%'"),
        ] {
            if let Some(n) = count(db, &format!("SELECT count(*) FROM (SELECT archived, preview, has_user_event, project_id, source{} FROM threads LIMIT {MAX_ROWS}) WHERE {expression}", if expression.contains("originator") { ", originator" } else { "" })) {
                result.visibility_counts.insert(name.into(), n);
            }
        }
        result.missing_project_references = count(db, &format!("SELECT count(*) FROM (SELECT project_id FROM threads LIMIT {MAX_ROWS}) t WHERE t.project_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM projects p WHERE p.id=t.project_id)"));
        if let Ok(mut query) = db.prepare(&format!("SELECT rollout_path FROM (SELECT rollout_path FROM threads LIMIT {MAX_ROWS}) WHERE rollout_path IS NOT NULL AND rollout_path != '' LIMIT {}", MAX_REFERENCES + 1)) {
            if let Ok(rows) = query.query_map([], |r| r.get::<_, String>(0)) {
                for (index, path) in rows.filter_map(std::result::Result::ok).enumerate() {
                    if index == MAX_REFERENCES { result.transcript_checks_limited = true; break; }
                    let path = PathBuf::from(path);
                    let path = if path.is_absolute() { path } else { home.join(path) };
                    result.transcripts_checked += 1;
                    if path.is_file() { result.transcripts_present += 1; } else { result.transcripts_missing += 1; }
                    if !util::paths_overlap(home, &path) { result.transcripts_outside_profile += 1; }
                }
            }
        }
    }
    if let (Some(state), Some(history)) = (state.as_ref(), history.as_ref()) {
        if let Ok(mut query) = history.prepare(&format!("SELECT DISTINCT thread_id FROM (SELECT thread_id FROM thread_items LIMIT {MAX_ROWS}) LIMIT {MAX_REFERENCES}")) {
            if let Ok(rows) = query.query_map([], |r| r.get::<_, String>(0)) {
                result.missing_history_thread_references = Some(rows.filter_map(std::result::Result::ok).filter(|id| !record_exists(state,"threads",id)).count() as u64);
            }
        }
    }
    result.sidebar = sidebar_counts(home, state.as_ref());
    result
}

fn record_exists(db: &Connection, table: &str, id: &str) -> bool {
    db.query_row(
        &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE id=?1)"),
        [id],
        |r| r.get::<_, bool>(0),
    )
    .unwrap_or(false)
}

fn sidebar_counts(home: &Path, state: Option<&Connection>) -> SidebarCounts {
    let path = home.join(".codex-global-state.json");
    let mut result = SidebarCounts {
        exists: path.is_file(),
        status: "missing".into(),
        ..Default::default()
    };
    let Some(value) = read_bounded::<Value>(&path) else {
        if result.exists {
            result.status = "unavailable".into();
        }
        return result;
    };
    result.status = "readable".into();
    let length = |key: &str| {
        value
            .get(key)
            .map(|v| {
                v.as_object()
                    .map(|v| v.len())
                    .or_else(|| v.as_array().map(|v| v.len()))
                    .unwrap_or(0)
            })
            .unwrap_or(0)
    };
    result.local_projects = length("local-projects");
    result.project_order = length("project-order");
    result.thread_project_assignments = length("thread-project-assignments");
    result.projectless_threads = length("projectless-thread-ids");
    result.project_thread_orders = length("sidebar-project-thread-orders");
    if let Some(hosts) = value
        .get("app-server-project-id-by-legacy-project-id-by-host")
        .and_then(Value::as_object)
    {
        result.mapped_hosts = hosts.len();
        result.mapped_projects = hosts
            .values()
            .filter_map(Value::as_object)
            .map(|v| v.len())
            .sum();
    }
    if let (Some(db), Some(assignments)) = (
        state,
        value
            .get("thread-project-assignments")
            .and_then(Value::as_object),
    ) {
        result.checks_limited = assignments.len() > MAX_REFERENCES;
        result.missing_thread_references = Some(
            assignments
                .keys()
                .take(MAX_REFERENCES)
                .filter(|id| !record_exists(db, "threads", id))
                .count(),
        );
    }
    result
}

fn matching_records(home: &Path, threads: &HashSet<String>, projects: &HashSet<String>) -> Value {
    let Some(db) = open_database(&home.join("state_5.sqlite")) else {
        return json!({"status":"unavailable"});
    };
    let mut thread_ids: Vec<_> = threads.iter().collect();
    thread_ids.sort();
    let mut project_ids: Vec<_> = projects.iter().collect();
    project_ids.sort();
    json!({"status":"checked", "threadsExpected":threads.len(), "threadsPresent":thread_ids.iter().take(MAX_REFERENCES).filter(|id| record_exists(&db,"threads",id)).count(),
        "projectsExpected":projects.len(),"projectsPresent":project_ids.iter().take(MAX_REFERENCES).filter(|id| record_exists(&db,"projects",id)).count(),
        "checksLimited":threads.len()>MAX_REFERENCES || projects.len()>MAX_REFERENCES})
}

fn source_metadata(source: &SnapshotManifest) -> Value {
    json!({"id":opaque(&source.id),"schemaVersion":source.schema_version,"createdAt":safe_timestamp(&source.created_at),
        "deviceId":opaque(&source.device_id),"codexVersion":safe_version(source.codex_version.as_deref()),
        "stateMigration":source.compatibility.state_migration,"historyMigration":source.compatibility.history_migration,
        "threads":source.threads.len(),"projects":source.projects.len(),"archivedThreads":source.threads.iter().filter(|t|t.archived).count(),
        "transcripts":source.threads.iter().filter(|t|t.rollout_relative_path.is_some()).count(),
        "objects":source.objects.len()})
}

fn safe_timestamp(value: &str) -> Option<String> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|v| v.to_rfc3339())
}
fn safe_version(value: Option<&str>) -> Option<String> {
    value
        .filter(|s| {
            s.len() < 80
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || ".-+ ".contains(c))
        })
        .map(str::to_string)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestMetadata {
    id: String,
    schema_version: u32,
    created_at: String,
    device_id: String,
    codex_version: Option<String>,
    compatibility: MigrationMetadata,
    #[serde(default)]
    threads: Vec<ThreadMetadata>,
    #[serde(default)]
    projects: Vec<IdMetadata>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MigrationMetadata {
    state_migration: Option<i64>,
    history_migration: Option<i64>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadMetadata {
    id: String,
    #[serde(default)]
    archived: bool,
    rollout_relative_path: Option<String>,
}
#[derive(Deserialize)]
struct IdMetadata {
    id: String,
}

fn baseline_metadata(
    config: &AppConfig,
    id: &str,
) -> Option<(Value, HashSet<String>, HashSet<String>)> {
    if id.is_empty()
        || id.len() > 200
        || id.contains(['/', '\\'])
        || id.contains("..")
        || config.cloud_root.is_empty()
    {
        return None;
    }
    let path = settings::cloud_store_root(config)
        .join("snapshots")
        .join(format!("{id}.json"));
    let source: ManifestMetadata = read_bounded(&path)?;
    if source.id != id {
        return None;
    }
    let value = json!({"id":opaque(&source.id),"schemaVersion":source.schema_version,"createdAt":safe_timestamp(&source.created_at),
        "deviceId":opaque(&source.device_id),"codexVersion":safe_version(source.codex_version.as_deref()),
        "stateMigration":source.compatibility.state_migration,"historyMigration":source.compatibility.history_migration,
        "threads":source.threads.len(),"projects":source.projects.len(),"archivedThreads":source.threads.iter().filter(|t|t.archived).count(),
        "transcripts":source.threads.iter().filter(|t|t.rollout_relative_path.is_some()).count(),"metadataOrigin":"locallyAvailableAppliedManifest"});
    Some((
        value,
        source.threads.into_iter().map(|t| t.id).collect(),
        source.projects.into_iter().map(|p| p.id).collect(),
    ))
}

fn journal_paths(root: &Path) -> Vec<PathBuf> {
    let mut paths = fs::read_dir(root)
        .into_iter()
        .flatten()
        .take(500)
        .filter_map(std::result::Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|e| e == "json")
                && p.file_stem().is_some_and(|n| {
                    n.to_string_lossy()
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-')
                })
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths.reverse();
    paths
}
fn retain_journals(root: &Path) {
    for path in journal_paths(root).into_iter().skip(MAX_JOURNALS) {
        let _ = fs::remove_file(path);
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecoveryMetadata {
    id: String,
    created_at: String,
    source_snapshot_id: Option<String>,
    status: RecoveryStatus,
}
fn latest_recovery(data: &Path) -> Option<RecoveryMetadata> {
    let root = data.join("recoveries");
    let mut paths = fs::read_dir(root)
        .ok()?
        .take(200)
        .filter_map(std::result::Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .collect::<Vec<_>>();
    paths.sort();
    paths.reverse();
    paths
        .into_iter()
        .take(20)
        .find_map(|p| read_bounded(&p.join("manifest.json")))
}

pub fn report(data: &Path, config: &AppConfig) -> DiagnosticsReport {
    let current = inspect_profile(Path::new(&config.codex_home));
    let mut candidates = Vec::new();
    let mut candidate_paths = Vec::new();
    if let Some(path) = std::env::var_os("CODEX_HOME") {
        candidate_paths.push(("environmentCodexHome", PathBuf::from(path)));
    }
    if let Some(home) = dirs::home_dir() {
        candidate_paths.push(("defaultUserProfile", home.join(".codex")));
    }
    if let Some(path) = settings::discover_codex_home() {
        candidate_paths.push(("discoveredProfile", path));
    }
    let mut inspected = std::collections::HashMap::new();
    inspected.insert(current.path_id.clone(), current.clone());
    for (role, path) in candidate_paths {
        let canonical = dunce::canonicalize(&path).unwrap_or_else(|_| path.clone());
        let id = opaque(&canonical.to_string_lossy().to_lowercase());
        let profile = inspected
            .entry(id)
            .or_insert_with(|| inspect_profile(&path));
        // The counts can be reused across aliases, while displaying each candidate's own path.
        let mut profile = profile.clone();
        profile.path = redact_path(&path);
        candidates.push(json!({"role":role,"matchesConfigured":profile.path_id==current.path_id,"profile":profile}));
    }
    let journals = journal_paths(&data.join("pull-diagnostics"))
        .into_iter()
        .take(MAX_JOURNALS)
        .filter_map(|p| read_bounded::<PullJournal>(&p))
        .collect::<Vec<_>>();
    let latest = journals
        .iter()
        .find(|j| j.operation_id.is_some())
        .or_else(|| journals.first());
    let state = read_bounded::<LocalState>(&data.join("state.json")).unwrap_or_default();
    let recovery = latest_recovery(data);
    let applied = state.last_applied_snapshot_id.as_deref().or_else(|| {
        recovery
            .as_ref()
            .and_then(|r| r.source_snapshot_id.as_deref())
    });
    let baseline = applied.and_then(|id| baseline_metadata(config, id));
    let mut findings = Vec::new();
    let mut add = |severity: &str, title: &str, detail: &str| {
        findings.push(Finding {
            severity: severity.into(),
            title: title.into(),
            detail: detail.into(),
        })
    };
    if !current.exists {
        add("error","Configured Codex folder is missing","Pull targets the configured profile. A missing folder means this configuration cannot show the restored chats. Check the Codex data folder in Settings.");
    }
    if !current.state_database.exists || !current.history_database.exists {
        add("error","Codex databases are missing","The configured folder does not contain both supported Codex databases. Codex may be opening another profile, or this profile has not been initialized.");
    }
    if current.state_database.status == "unavailable"
        || current.history_database.status == "unavailable"
    {
        add("warning","A database could not be inspected","The database may be locked, unavailable locally, or unreadable. Counts marked unavailable are unknown, not zero. Refresh after Codex settles or closes.");
    }
    if candidates
        .iter()
        .any(|p| p["matchesConfigured"] == false && p["profile"]["stateDatabase"]["exists"] == true)
    {
        add("warning","Another Codex profile exists","A discovered or environment profile differs from the Pull destination. If Codex opens that profile, a successful Pull into the configured folder will not appear. Candidate paths are evidence of possible mismatch, not proof of the active runtime profile.");
    }
    if current.compatibility.get("supported") == Some(&Value::Bool(false)) {
        add("warning","Database format differs from the tested adapter","The current database schema does not match a tested storage profile. Codex may have upgraded after Pull. Counts remain diagnostic only.");
    }
    if current.state_database.counts.get("threads") == Some(&0) {
        add("warning","No chat records in this profile","The configured database contains zero chat records. Compare source selection and restored counts, then confirm the configured folder matches the profile Codex opens.");
    }
    if current
        .state_database
        .counts
        .get("threads")
        .copied()
        .unwrap_or(0)
        > 0
    {
        add("info","Chat records exist; Codex visibility is unconfirmed","These records are present in the configured profile. Pull verification checks that profile's databases and files. It does not prove the Codex desktop opens that folder or displays every imported record. Archived status, missing previews, source filters and sidebar host mappings may affect visibility.");
    }
    if current.state_database.counts.get("projects") == Some(&0) {
        add("info","No saved projects in this profile","This can be expected for projectless chats. If projects were selected on the source, compare the applied manifest and restored project counts.");
    }
    if current.transcripts_missing > 0 {
        add("error","Referenced transcripts are missing","Some chat database records point to transcript files that are absent. Their history may not open even when the database row was restored.");
    }
    if current.transcripts_outside_profile > 0 {
        add("warning","Transcript paths leave this Codex profile","Some records point outside the configured Codex folder. A moved profile or a stale source-computer path can prevent history from opening.");
    }
    if current.missing_project_references.unwrap_or(0) > 0
        || current.missing_history_thread_references.unwrap_or(0) > 0
        || current.sidebar.missing_thread_references.unwrap_or(0) > 0
    {
        add("warning","Some saved references have no matching record","Project, history, or sidebar references do not match the current chat database. This can explain missing project history or sidebar entries.");
    }
    if current
        .state_database
        .counts
        .get("projects")
        .copied()
        .unwrap_or(0)
        > 0
        && current.sidebar.local_projects == 0
    {
        add("warning","Project database and sidebar metadata differ","Projects exist in the database but no legacy sidebar projects were found. This may reflect a newer sidebar format or incomplete sidebar restoration. Compare the Codex build and project counts.");
    }
    if let Some(log) = latest {
        if log.status == "failed" {
            add("warning","The latest Pull attempt stopped","The persistent log records its last phase and a redacted failure category. Review Recovery before another Pull if a recovery point remains pending.");
        }
        if log.status == "inProgress" {
            add("warning","A Pull log has no completion record","The app may have stopped during Pull, or a Pull is still running. Review its last persisted phase and Recovery before retrying.");
        }
        if log.phase == "keptLocalVersions" {
            add("info","The Pull kept local versions","This Pull acknowledged the snapshot without replacing local records. A success result in this mode does not mean incoming chats were imported.");
        }
    } else {
        add("info","No persistent Pull log from this version","Earlier app versions did not save this log. The current profile, saved applied-snapshot identity, local manifest metadata and latest recovery point still provide evidence.");
    }
    if applied.is_some() && baseline.is_none() {
        add("info","Applied manifest metadata is unavailable locally","The saved applied-snapshot identity is available, but its manifest is missing, too large, unreadable, or cloud-only. Diagnostics did not download cloud content.");
    }
    let source = baseline
        .as_ref()
        .map(|b| b.0.clone())
        .or_else(|| latest.and_then(|l| l.source_snapshot.clone()));
    let current_source_records = baseline.as_ref().map(|(_, threads, projects)| {
        matching_records(Path::new(&config.codex_home), threads, projects)
    });
    if let Some(records) = &current_source_records {
        if records["status"] == "checked"
            && records["checksLimited"] == false
            && (records["threadsExpected"].as_u64() > records["threadsPresent"].as_u64()
                || records["projectsExpected"].as_u64() > records["projectsPresent"].as_u64())
        {
            add("warning","Some source records are absent from this profile","The saved applied snapshot references records not currently present. Local conflict choices, later deletions, or a different destination profile can explain this. Compare the Pull log selection before concluding restoration failed.");
        }
    }
    let summary = if findings.iter().any(|f| f.severity == "error") {
        "Diagnostics found missing profile data or transcripts."
    } else if findings.iter().any(|f| f.severity == "warning") {
        "Diagnostics found possible causes for missing chats or projects."
    } else {
        "Profile metadata collected. Compare the active Codex profile with the Pull destination."
    }
    .into();
    let recovery = recovery.as_ref().map(|r|json!({"id":opaque(&r.id),"createdAt":safe_timestamp(&r.created_at),"sourceSnapshotId":r.source_snapshot_id.as_deref().map(opaque),"status":r.status}));
    DiagnosticsReport {
        schema_version: 1,
        generated_at: now(),
        summary,
        findings,
        report: json!({
            "engineVersion":env!("CARGO_PKG_VERSION"),"adapter":codex::ADAPTER_NAME,"supportedCodexBuilds":codex::SUPPORTED_CODEX_BUILDS,
            "codexRunning":platform::codex_running(),"configuredProfile":current,"candidateProfiles":candidates,
            "activeRuntimeProfile":"notEstablishedByReadOnlyDiscovery","latestPull":latest,"recentPulls":journals,
            "lastAppliedSnapshotId":applied.map(opaque),"sourceSnapshot":source,"currentSourceRecords":current_source_records,
            "latestRecovery":recovery,"limits":{"rowsPerTable":MAX_ROWS,"referencedFiles":MAX_REFERENCES,"journals":MAX_JOURNALS,"jsonBytes":MAX_JSON_BYTES},
            "limitations":["Paths retain folder names; user home names are redacted. Review paths before sharing.","No conversation bodies, titles, credentials, SQL rows, raw error messages, workspace files or cloud objects are included.","Read-only live observations are not an atomic snapshot across databases and sidebar metadata.","Counts may be capped. Candidate profiles do not establish the active desktop runtime profile."]
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{engine::Engine, snapshot};
    use tempfile::tempdir;

    fn fixture(home: &Path) {
        fs::create_dir_all(home).unwrap();
        let schema: Value = serde_json::from_str(crate::compatibility::PROFILES[0].schema).unwrap();
        for name in ["state_5.sqlite", "thread_history_1.sqlite"] {
            let db = Connection::open(home.join(name)).unwrap();
            for kind in ["table", "index", "trigger"] {
                for object in schema[name]["objects"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|o| o["type"] == kind)
                {
                    db.execute_batch(object["sql"].as_str().unwrap()).unwrap();
                }
            }
            db.execute("INSERT INTO _sqlx_migrations(version,description,success,checksum,execution_time) VALUES(?1,'fixture',1,X'00',0)", [schema[name]["migration"].as_i64().unwrap()]).unwrap();
        }
    }

    #[test]
    fn restored_profile_counts_and_source_metadata_exclude_private_content() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        let stage = temp.path().join("stage");
        fixture(&source);
        fixture(&target);
        let transcript = source.join("sessions/test.jsonl");
        fs::create_dir_all(transcript.parent().unwrap()).unwrap();
        fs::write(
            &transcript,
            "{\"body\":\"PRIVATE_CONVERSATION_SENTINEL\"}\n",
        )
        .unwrap();
        let state = Connection::open(source.join("state_5.sqlite")).unwrap();
        state.execute("INSERT INTO projects(id,name,metadata,position,created_at_ms,updated_at_ms) VALUES('private-project','PRIVATE_PROJECT_TITLE','{}',0,1,1)",[]).unwrap();
        state.execute("INSERT INTO threads(id,rollout_path,created_at,updated_at,source,model_provider,cwd,title,sandbox_policy,approval_mode,project_id,preview) VALUES('private-thread',?1,1,1,'cli','openai',?2,'PRIVATE_CHAT_TITLE','{}','never','private-project','PRIVATE_PREVIEW')", rusqlite::params![transcript.to_string_lossy(),source.to_string_lossy()]).unwrap();
        let history = Connection::open(source.join("thread_history_1.sqlite")).unwrap();
        history.execute("INSERT INTO thread_items(thread_id,turn_id,item_id,rollout_ordinal,created_at_ms,item_json,item_type,updated_at_ordinal) VALUES('private-thread','turn','item',1,1,'{\"text\":\"PRIVATE_CONVERSATION_SENTINEL\"}','message',1)",[]).unwrap();
        util::write_json(&source.join(".codex-global-state.json"), &json!({"thread-project-assignments":{"private-thread":"private-project"},"authToken":"PRIVATE_CREDENTIAL_SENTINEL"})).unwrap();
        let mut config = settings::default_config();
        config.codex_home = source.to_string_lossy().into();
        config.projectless_root = source.join("workspaces").to_string_lossy().into();
        config.selection.default_project_mode = ProjectMode::HistoryOnly;
        codex::snapshot_databases(&source, &stage).unwrap();
        let export = codex::export_selected(
            &source,
            &stage,
            &config.selection,
            Path::new(&config.projectless_root),
        )
        .unwrap();
        let store = snapshot::ObjectStore::new(temp.path().join("objects")).unwrap();
        let mut manifest = snapshot::build_manifest(&config, export, &store, None).unwrap();
        manifest.codex_version = Some("0.153.4".into());
        let incoming_threads = HashSet::from(["private-thread".to_string()]);
        let incoming_projects = HashSet::from(["private-project".to_string()]);
        let target_transcript = target.join("sessions/test.jsonl");
        fs::create_dir_all(target_transcript.parent().unwrap()).unwrap();
        fs::copy(transcript, &target_transcript).unwrap();
        config.codex_home = target.to_string_lossy().into();
        config.cloud_root = temp.path().join("cloud").to_string_lossy().into();
        let mut log = PullLog::start(
            &temp.path().join("data"),
            &config,
            Some("private-operation"),
            "validation",
        );
        log.source(&manifest);
        log.selection(&incoming_threads, &incoming_projects);
        codex::apply_bundle(
            &target,
            &manifest,
            &incoming_threads,
            &incoming_projects,
            &HashSet::new(),
            &HashSet::new(),
            &config,
            &std::collections::HashMap::from([(
                "private-thread".into(),
                target_transcript.to_string_lossy().into_owned(),
            )]),
            &std::collections::HashMap::new(),
        )
        .unwrap();
        log.verified(&config, &incoming_threads, &incoming_projects);
        log.phase("complete");
        log.finish(&Ok::<_, SpiceError>(()), "succeeded");
        let restored = inspect_profile(&target);
        assert_eq!(restored.state_database.counts["threads"], 1);
        assert_eq!(restored.state_database.counts["projects"], 1);
        assert_eq!(restored.history_database.counts["thread_items"], 1);
        assert_eq!(restored.transcripts_present, 1);
        assert_eq!(restored.transcripts_missing, 0);
        assert_eq!(
            log.journal.restored_records.as_ref().unwrap()["threadsPresent"],
            1
        );
        let encoded = serde_json::to_string(&log.journal).unwrap();
        for secret in [
            "PRIVATE_CONVERSATION_SENTINEL",
            "PRIVATE_PROJECT_TITLE",
            "PRIVATE_CHAT_TITLE",
            "PRIVATE_PREVIEW",
            "PRIVATE_CREDENTIAL_SENTINEL",
            "private-thread",
            "private-project",
            "private-operation",
        ] {
            assert!(!encoded.contains(secret), "leaked {secret}");
        }
        // Old-version success evidence works without any new Pull journal.
        let legacy_data = temp.path().join("legacy");
        settings::save_local_state(
            &legacy_data,
            &LocalState {
                last_applied_snapshot_id: Some(manifest.id.clone()),
                ..Default::default()
            },
        )
        .unwrap();
        util::write_json(
            &settings::cloud_store_root(&config)
                .join("snapshots")
                .join(format!("{}.json", manifest.id)),
            &manifest,
        )
        .unwrap();
        let legacy = report(&legacy_data, &config);
        assert!(legacy.report["latestPull"].is_null());
        assert_eq!(legacy.report["sourceSnapshot"]["threads"], 1);
        assert_eq!(legacy.report["currentSourceRecords"]["threadsPresent"], 1);
        let encoded = serde_json::to_string(&legacy).unwrap();
        assert!(!encoded.contains("PRIVATE_"));
    }

    #[test]
    fn missing_profile_is_read_only_and_early_pull_failures_are_persisted() {
        let temp = tempdir().unwrap();
        let mut config = settings::default_config();
        config.codex_home = temp.path().join("missing-profile").to_string_lossy().into();
        let engine = Engine::new(temp.path().join("app")).unwrap();
        let report = engine.diagnostics_report(&config);
        assert!(report
            .findings
            .iter()
            .any(|f| f.title == "Configured Codex folder is missing"));
        assert!(!Path::new(&config.codex_home).exists());
        assert!(engine
            .execute_pull(&config, "PRIVATE_OPERATION_IDENTIFIER", &[])
            .is_err());
        let report = engine.diagnostics_report(&config);
        assert_eq!(report.report["latestPull"]["status"], "failed");
        assert_eq!(report.report["latestPull"]["phase"], "validation");
        assert!(report.report["latestPull"]["failureCategory"].is_string());
        assert!(!serde_json::to_string(&report)
            .unwrap()
            .contains("PRIVATE_OPERATION_IDENTIFIER"));
        assert!(!Path::new(&config.codex_home).exists());
        let mut log = PullLog::start(
            &engine.data_dir,
            &config,
            Some("failed-settings"),
            "validation",
        );
        let failure = Err::<(), _>(SpiceError::User(
            "Settings changed after this preview. PRIVATE_ERROR_DETAIL".into(),
        ));
        log.finish(&failure, "succeeded");
        let saved: PullJournal = read_bounded(&log.path).unwrap();
        assert_eq!(saved.failure_category.as_deref(), Some("settingsChanged"));
        assert!(!serde_json::to_string(&saved)
            .unwrap()
            .contains("PRIVATE_ERROR_DETAIL"));
    }

    #[test]
    fn broken_references_and_visibility_counts_are_detected_without_bodies() {
        let temp = tempdir().unwrap();
        fixture(temp.path());
        let db = Connection::open(temp.path().join("state_5.sqlite")).unwrap();
        db.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        db.execute("INSERT INTO threads(id,rollout_path,created_at,updated_at,source,model_provider,cwd,title,sandbox_policy,approval_mode,project_id,archived) VALUES('chat',?1,1,1,'cli','openai','.','secret','{}','never','absent',1)",[temp.path().join("absent.jsonl").to_string_lossy().as_ref()]).unwrap();
        util::write_json(&temp.path().join(".codex-global-state.json"),&json!({"thread-project-assignments":{"missing-chat":"absent"},"app-server-project-id-by-legacy-project-id-by-host":{"PRIVATE_HOST":{"legacy":"project"}}})).unwrap();
        let profile = inspect_profile(temp.path());
        assert_eq!(profile.transcripts_missing, 1);
        assert_eq!(profile.missing_project_references, Some(1));
        assert_eq!(profile.sidebar.missing_thread_references, Some(1));
        assert_eq!(profile.sidebar.mapped_hosts, 1);
        assert_eq!(profile.visibility_counts["archivedThreads"], 1);
        assert!(!serde_json::to_string(&profile)
            .unwrap()
            .contains("PRIVATE_HOST"));
    }
}
