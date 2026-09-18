use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub schema_version: u32,
    pub device_id: String,
    pub device_name: String,
    pub codex_home: String,
    pub projectless_root: String,
    #[serde(default)]
    pub projects_root: String,
    pub cloud_root: String,
    pub cloud_provider: CloudProvider,
    pub theme: ThemeMode,
    pub onboarding_complete: bool,
    #[serde(default)]
    pub source_roots: HashMap<String, String>,
    pub destination_roots: HashMap<String, String>,
    pub selection: SelectionRules,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum CloudProvider {
    OneDrive,
    GoogleDrive,
    ICloud,
    #[default]
    Custom,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ProjectMode {
    #[default]
    Full,
    HistoryOnly,
    Excluded,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionRules {
    pub revision: String,
    pub default_project_mode: ProjectMode,
    pub project_modes: HashMap<String, ProjectMode>,
    pub excluded_thread_ids: Vec<String>,
    pub include_archived: bool,
    pub include_build_outputs: bool,
    pub include_sensitive_files: bool,
    pub extra_exclude_patterns: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompatibilityInfo {
    pub supported: bool,
    pub adapter: String,
    pub state_migration: Option<i64>,
    pub history_migration: Option<i64>,
    pub schema_fingerprint: String,
    pub explanation: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudCandidate {
    pub provider: CloudProvider,
    pub path: String,
    pub label: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentDiscovery {
    pub codex_home: Option<String>,
    pub codex_home_resolved: Option<String>,
    pub codex_executable: Option<String>,
    pub codex_version: Option<String>,
    pub codex_running: bool,
    pub cloud_candidates: Vec<CloudCandidate>,
    pub compatibility: Option<CompatibilityInfo>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSummary {
    pub id: String,
    pub title: String,
    pub preview: String,
    pub cwd: String,
    pub project_id: Option<String>,
    pub archived: bool,
    pub updated_at_ms: i64,
    pub estimated_bytes: u64,
    pub projectless: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    pub roots: Vec<String>,
    pub local_roots: Vec<String>,
    pub thread_count: usize,
    pub estimated_bytes: u64,
    pub git_repository: bool,
    pub linked_worktree: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentCatalog {
    pub threads: Vec<ThreadSummary>,
    pub projects: Vec<ProjectSummary>,
    pub total_estimated_bytes: u64,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotSummary {
    pub id: String,
    pub short_id: String,
    pub device_id: String,
    pub device_name: String,
    pub created_at: String,
    pub parent_id: Option<String>,
    pub logical_bytes: u64,
    pub stored_bytes: u64,
    pub object_count: usize,
    pub verified: bool,
    pub client_sync_state: ClientSyncState,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClientSyncState {
    Unknown,
    Waiting,
    ReportedSynced,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub latest_snapshot: Option<SnapshotSummary>,
    pub visible_heads: Vec<SnapshotSummary>,
    pub last_applied_snapshot_id: Option<String>,
    pub last_pushed_snapshot_id: Option<String>,
    pub cloud_bytes: u64,
    pub incoming_available: bool,
    pub merge_ready: bool,
    pub pending_recovery: bool,
    pub state: SyncState,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncState {
    Ready,
    NeedsPull,
    NeedsSetup,
    Blocked,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangePreview {
    pub key: String,
    pub kind: ChangeKind,
    pub action: ChangeAction,
    pub label: String,
    pub detail: String,
    pub bytes: u64,
    pub conflict: Option<ConflictDetail>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChangeKind {
    Thread,
    ProjectFile,
    Project,
    Settings,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChangeAction {
    Add,
    Update,
    Delete,
    Unchanged,
    Conflict,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictDetail {
    pub local_description: String,
    pub incoming_description: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationPreview {
    pub operation_id: String,
    pub direction: Direction,
    pub snapshot_id: Option<String>,
    pub changes: Vec<ChangePreview>,
    pub warnings: Vec<String>,
    pub blocked_reasons: Vec<String>,
    pub estimated_bytes: u64,
    pub requires_codex_close: bool,
    pub required_mappings: Vec<RequiredMapping>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequiredMapping {
    pub project_id: String,
    pub root_index: usize,
    pub project_name: String,
    pub source_path: String,
    pub suggested_path: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    Push,
    Pull,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationProgress {
    pub operation_id: String,
    pub phase: OperationPhase,
    pub message: String,
    pub completed_steps: u32,
    pub total_steps: u32,
    pub cancellation_requested: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OperationPhase {
    Ready,
    Rechecking,
    Capturing,
    Verifying,
    Publishing,
    BackingUp,
    Applying,
    FinalVerification,
    Cancelling,
    Complete,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictResolution {
    pub key: String,
    pub choice: ConflictChoice,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConflictChoice {
    Local,
    Incoming,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationResult {
    pub snapshot: SnapshotSummary,
    pub warnings: Vec<String>,
    pub recovery_id: Option<String>,
    pub status_message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudCleanupPreview {
    pub operation_id: String,
    pub snapshot_count: usize,
    pub object_count: usize,
    pub stored_bytes: u64,
    pub confirmation_phrase: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudCleanupResult {
    pub snapshots_removed: usize,
    pub objects_removed: usize,
    pub bytes_removed: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoverySummary {
    pub id: String,
    pub created_at: String,
    pub reason: String,
    pub source_snapshot_id: Option<String>,
    pub status: RecoveryStatus,
    pub size_bytes: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RecoveryStatus {
    Available,
    Pending,
    Restored,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalState {
    pub last_applied_snapshot_id: Option<String>,
    pub last_pushed_snapshot_id: Option<String>,
    pub last_selection_revision: Option<String>,
    #[serde(default)]
    pub pending_merge_parent_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotManifest {
    pub schema_version: u32,
    pub id: String,
    pub created_at: String,
    pub device_id: String,
    pub device_name: String,
    pub parent_id: Option<String>,
    #[serde(default)]
    pub additional_parent_ids: Vec<String>,
    pub selection_revision: String,
    pub selection: SelectionRules,
    pub compatibility: CompatibilityInfo,
    pub codex_version: Option<String>,
    pub threads: Vec<ThreadExport>,
    pub projects: Vec<ProjectExport>,
    pub objects: Vec<ObjectEntry>,
    pub ui_state: Value,
    pub session_index_lines: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadExport {
    pub id: String,
    pub title: String,
    pub project_id: Option<String>,
    pub projectless: bool,
    pub archived: bool,
    pub source_cwd: String,
    pub rollout_relative_path: Option<String>,
    pub projectless_relative_root: Option<String>,
    #[serde(default)]
    pub attachments: Vec<AttachmentReference>,
    pub state_rows: BTreeMap<String, Vec<DatabaseRow>>,
    pub history_rows: BTreeMap<String, Vec<DatabaseRow>>,
    pub fingerprint: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentReference {
    pub source_path: String,
    pub logical_path: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectExport {
    pub id: String,
    #[serde(default)]
    pub legacy_id: Option<String>,
    pub name: String,
    pub mode: ProjectMode,
    pub source_roots: Vec<String>,
    pub rows: BTreeMap<String, Vec<DatabaseRow>>,
    pub git: Vec<GitDescriptor>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitDescriptor {
    pub project_id: String,
    pub root_index: usize,
    pub common_id: String,
    pub linked_worktree: bool,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub bundle_object: Option<String>,
    pub index_object: Option<String>,
    #[serde(default)]
    pub index_pack_object: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseRow {
    pub values: BTreeMap<String, SqlValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum SqlValue {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectEntry {
    pub hash: String,
    pub logical_path: String,
    pub kind: ObjectKind,
    pub owner_id: String,
    pub raw_size: u64,
    pub stored_size: u64,
    pub executable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ObjectKind {
    Rollout,
    ProjectFile,
    ProjectlessFile,
    GitBundle,
    GitIndex,
    GitObjectPack,
    Artifact,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredPreview {
    pub preview: OperationPreview,
    pub config_fingerprint: String,
    pub local_fingerprint: String,
    pub snapshot_id: Option<String>,
}

impl SnapshotManifest {
    pub fn summary(&self, stored_bytes: u64, verified: bool) -> SnapshotSummary {
        SnapshotSummary {
            id: self.id.clone(),
            short_id: handoff_label(&self.id),
            device_id: self.device_id.clone(),
            device_name: self.device_name.clone(),
            created_at: self.created_at.clone(),
            parent_id: self.parent_id.clone(),
            logical_bytes: self.objects.iter().map(|item| item.raw_size).sum(),
            stored_bytes,
            object_count: self.objects.len(),
            verified,
            client_sync_state: ClientSyncState::Unknown,
        }
    }
}

/// A stable, sortable UTC date with the existing unique handoff suffix.
/// This changes presentation only. Stored snapshot identities remain unchanged.
pub fn handoff_label(id: &str) -> String {
    let suffix = id
        .chars()
        .skip(id.chars().count().saturating_sub(8))
        .collect::<String>()
        .to_uppercase();
    let date = id
        .split_once('-')
        .and_then(|(date, _)| chrono::NaiveDateTime::parse_from_str(date, "%Y%m%dT%H%M%SZ").ok());
    match date {
        Some(date) => format!("{}-{suffix}", date.format("%Y%m%d.%H%MZ")),
        None => suffix,
    }
}

#[cfg(test)]
mod handoff_label_tests {
    use super::handoff_label;

    #[test]
    fn label_keeps_date_and_distinct_suffix_for_handoffs_in_same_minute() {
        let earlier = handoff_label("20260918T173048Z-d2747f21166641e6a83bf01d12b838de");
        let later = handoff_label("20260918T173059Z-d2747f21166641e6a83bf01d12345678");
        assert_eq!(earlier, "20260918.1730Z-12B838DE");
        assert_ne!(earlier, later);
        assert_eq!(handoff_label("legacy-12345678"), "12345678");
    }
}
