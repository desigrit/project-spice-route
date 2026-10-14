use crate::codex::{self, CodexExport};
use crate::error::{Result, SpiceError};
use crate::models::{SnapshotManifest, SqlValue, ThreadExport};
use crate::util::{read_json, write_json};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

const VERSION: u32 = 2;
const FILE_NAME: &str = "retained-history-fields.json";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetainedHistory {
    version: u32,
    threads: BTreeMap<String, Vec<RetainedItem>>,
    #[serde(default)]
    turns: BTreeMap<String, Vec<RetainedTurn>>,
}

impl Default for RetainedHistory {
    fn default() -> Self {
        Self {
            version: VERSION,
            threads: BTreeMap::new(),
            turns: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RetainedItem {
    turn_id: String,
    item_id: String,
    created_at_ms: SqlValue,
    started_at_ms: Option<SqlValue>,
    completed_at_ms: Option<SqlValue>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RetainedTurn {
    turn_id: String,
    rollout_ordinal: i64,
    root_turn_id: String,
}

pub fn path(data_dir: &Path) -> PathBuf {
    data_dir.join(FILE_NAME)
}

pub fn load(data_dir: &Path, required: bool) -> Result<RetainedHistory> {
    let path = path(data_dir);
    if !path.is_file() {
        if required {
            return Err(SpiceError::User(format!(
                "Retained history metadata is missing at {}. Push and Pull are paused to protect newer Codex fields. Restore a Spice Route recovery point or contact support.",
                path.display()
            )));
        }
        return Ok(RetainedHistory::default());
    }
    let mut value: RetainedHistory = read_json(&path).map_err(|error| {
        SpiceError::User(format!(
            "Retained history metadata could not be read at {}: {error}. Push and Pull are paused to protect newer Codex fields.",
            path.display()
        ))
    })?;
    // Version 1 kept item timing only. Accept it without dropping old values,
    // then write the new version when the file next participates in a Pull.
    if value.version == 1 && value.turns.is_empty() {
        value.version = VERSION;
    }
    value.validate()?;
    Ok(value)
}

impl RetainedHistory {
    pub fn has_entries(&self) -> bool {
        !self.threads.is_empty() || !self.turns.is_empty()
    }

    fn validate(&self) -> Result<()> {
        if self.version != VERSION {
            return Err(SpiceError::User(format!(
                "Retained history metadata version {} is not supported by this Spice Route release. Push and Pull are paused.",
                self.version
            )));
        }
        for (thread_id, items) in &self.threads {
            let mut keys = HashSet::new();
            for item in items {
                if !keys.insert((&item.turn_id, &item.item_id))
                    || item.started_at_ms.is_none() && item.completed_at_ms.is_none()
                    || matches!(item.started_at_ms, Some(SqlValue::Null))
                    || matches!(item.completed_at_ms, Some(SqlValue::Null))
                {
                    return Err(SpiceError::User(format!(
                        "Retained history metadata for chat {thread_id} is invalid. Push and Pull are paused."
                    )));
                }
            }
        }
        for (thread_id, turns) in &self.turns {
            let mut keys = HashSet::new();
            if turns.iter().any(|turn| !keys.insert(&turn.turn_id)) {
                return Err(SpiceError::User(format!(
                    "Retained turn metadata for chat {thread_id} is invalid. Push and Pull are paused."
                )));
            }
        }
        Ok(())
    }

    pub fn after_pull(
        &self,
        manifest: &SnapshotManifest,
        incoming_threads: &HashSet<String>,
        deleted_threads: &HashSet<String>,
        history_migration: Option<i64>,
    ) -> Result<Self> {
        let mut updated = self.clone();
        for thread_id in incoming_threads.iter().chain(deleted_threads) {
            updated.threads.remove(thread_id);
            updated.turns.remove(thread_id);
        }
        if history_migration.is_some_and(|version| version < 7) {
            for thread in manifest
                .threads
                .iter()
                .filter(|thread| incoming_threads.contains(&thread.id))
            {
                let mut items = Vec::new();
                for row in thread
                    .history_rows
                    .get("thread_items")
                    .into_iter()
                    .flatten()
                {
                    let started_at_ms = non_null(row.values.get("started_at_ms"));
                    let completed_at_ms = non_null(row.values.get("completed_at_ms"));
                    if started_at_ms.is_none() && completed_at_ms.is_none() {
                        continue;
                    }
                    let key = |column: &str| match row.values.get(column) {
                        Some(SqlValue::Text(value)) => Ok(value.clone()),
                        _ => Err(SpiceError::CorruptSnapshot(format!(
                            "Chat {} has a timing value without a valid thread item {column}.",
                            thread.id
                        ))),
                    };
                    let created_at_ms =
                        row.values.get("created_at_ms").cloned().ok_or_else(|| {
                            SpiceError::CorruptSnapshot(format!(
                                "Chat {} has a timing value without a thread item creation time.",
                                thread.id
                            ))
                        })?;
                    items.push(RetainedItem {
                        turn_id: key("turn_id")?,
                        item_id: key("item_id")?,
                        created_at_ms,
                        started_at_ms,
                        completed_at_ms,
                    });
                }
                if !items.is_empty() {
                    updated.threads.insert(thread.id.clone(), items);
                }
            }
        }
        if history_migration.is_some_and(|version| version < 8) {
            for thread in manifest
                .threads
                .iter()
                .filter(|thread| incoming_threads.contains(&thread.id))
            {
                let mut turns = Vec::new();
                for row in thread
                    .history_rows
                    .get("thread_turns")
                    .into_iter()
                    .flatten()
                {
                    let Some(root) = non_null(row.values.get("root_turn_id")) else {
                        continue;
                    };
                    let (
                        Some(SqlValue::Text(thread_id)),
                        Some(SqlValue::Text(turn_id)),
                        Some(SqlValue::Integer(rollout_ordinal)),
                        SqlValue::Text(root_turn_id),
                    ) = (
                        row.values.get("thread_id"),
                        row.values.get("turn_id"),
                        row.values.get("rollout_ordinal"),
                        root,
                    )
                    else {
                        return Err(SpiceError::CorruptSnapshot(format!(
                            "Chat {} has root turn metadata without a valid turn identity.",
                            thread.id
                        )));
                    };
                    if thread_id != &thread.id {
                        return Err(SpiceError::CorruptSnapshot(format!(
                            "Root turn metadata belongs to a different chat than {}.",
                            thread.id
                        )));
                    }
                    turns.push(RetainedTurn {
                        turn_id: turn_id.clone(),
                        rollout_ordinal: *rollout_ordinal,
                        root_turn_id,
                    });
                }
                if !turns.is_empty() {
                    updated.turns.insert(thread.id.clone(), turns);
                }
            }
        }
        updated.validate()?;
        Ok(updated)
    }

    pub fn stage(&self, directory: &Path) -> Result<PathBuf> {
        fs::create_dir_all(directory)?;
        let staged = directory.join(FILE_NAME);
        write_json(&staged, self)?;
        Ok(staged)
    }

    pub fn covers(&self, thread: &ThreadExport) -> bool {
        let items = self.threads.get(&thread.id);
        let by_key: HashMap<(&str, &str), &RetainedItem> = items
            .into_iter()
            .flatten()
            .map(|item| ((item.turn_id.as_str(), item.item_id.as_str()), item))
            .collect();
        thread
            .history_rows
            .get("thread_items")
            .into_iter()
            .flatten()
            .filter(|row| {
                non_null(row.values.get("started_at_ms")).is_some()
                    || non_null(row.values.get("completed_at_ms")).is_some()
            })
            .all(|row| {
                let (Some(SqlValue::Text(turn)), Some(SqlValue::Text(id))) =
                    (row.values.get("turn_id"), row.values.get("item_id"))
                else {
                    return false;
                };
                by_key
                    .get(&(turn.as_str(), id.as_str()))
                    .is_some_and(|item| {
                        row.values.get("created_at_ms") == Some(&item.created_at_ms)
                            && non_null(row.values.get("started_at_ms")) == item.started_at_ms
                            && non_null(row.values.get("completed_at_ms")) == item.completed_at_ms
                    })
            })
    }

    pub fn covers_table(&self, thread: &ThreadExport, table: &str) -> bool {
        match table {
            "thread_items" => self.covers(thread),
            "thread_turns" => {
                let by_key: HashMap<&str, &RetainedTurn> = self
                    .turns
                    .get(&thread.id)
                    .into_iter()
                    .flatten()
                    .map(|turn| (turn.turn_id.as_str(), turn))
                    .collect();
                thread
                    .history_rows
                    .get(table)
                    .into_iter()
                    .flatten()
                    .filter(|row| non_null(row.values.get("root_turn_id")).is_some())
                    .all(|row| {
                        let Some(SqlValue::Text(turn_id)) = row.values.get("turn_id") else {
                            return false;
                        };
                        by_key.get(turn_id.as_str()).is_some_and(|turn| {
                            row.values.get("thread_id") == Some(&SqlValue::Text(thread.id.clone()))
                                && row.values.get("rollout_ordinal")
                                    == Some(&SqlValue::Integer(turn.rollout_ordinal))
                                && row.values.get("root_turn_id")
                                    == Some(&SqlValue::Text(turn.root_turn_id.clone()))
                        })
                    })
            }
            _ => false,
        }
    }

    pub fn overlay_export(&self, export: &mut CodexExport, strict: bool) -> Result<()> {
        self.overlay_threads(&mut export.threads, strict)
    }

    pub fn overlay_threads(&self, threads: &mut [ThreadExport], strict: bool) -> Result<()> {
        self.overlay_item_timing(threads, strict)?;
        self.overlay_turn_roots(threads, strict)
    }

    fn overlay_item_timing(&self, threads: &mut [ThreadExport], strict: bool) -> Result<()> {
        for thread in threads {
            let Some(items) = self.threads.get(&thread.id) else {
                continue;
            };
            let Some(rows) = thread.history_rows.get_mut("thread_items") else {
                if strict {
                    return Err(SpiceError::User(format!(
                        "Chat '{}' has retained item timing values but no local history table. Push is paused to prevent a silent loss. Pull can restore the incoming version.",
                        thread.title
                    )));
                }
                continue;
            };
            let by_key: HashMap<(&str, &str), &RetainedItem> = items
                .iter()
                .map(|item| ((item.turn_id.as_str(), item.item_id.as_str()), item))
                .collect();
            let mut changed = false;
            let mut matched = 0_usize;
            for row in rows {
                let (Some(SqlValue::Text(turn)), Some(SqlValue::Text(id))) =
                    (row.values.get("turn_id"), row.values.get("item_id"))
                else {
                    continue;
                };
                let Some(item) = by_key.get(&(turn.as_str(), id.as_str())) else {
                    continue;
                };
                matched += 1;
                if row.values.get("created_at_ms") != Some(&item.created_at_ms) {
                    if strict {
                        return Err(SpiceError::User(format!(
                            "A retained history item in chat '{}' changed identity. Push is paused to protect its timing values. Pull can restore the incoming version.",
                            thread.title
                        )));
                    }
                    continue;
                }
                for (column, value) in [
                    ("started_at_ms", &item.started_at_ms),
                    ("completed_at_ms", &item.completed_at_ms),
                ] {
                    if let Some(value) = value {
                        if !row
                            .values
                            .get(column)
                            .is_some_and(|existing| !matches!(existing, SqlValue::Null))
                        {
                            row.values.insert(column.to_string(), value.clone());
                            changed = true;
                        }
                    }
                }
            }
            if strict && matched != items.len() {
                return Err(SpiceError::User(format!(
                    "Chat '{}' is missing a local history item whose timing values Spice Route retained. Push is paused to prevent a silent loss. Pull can restore the incoming version.",
                    thread.title
                )));
            }
            if changed {
                codex::refresh_thread_fingerprint(thread)?;
            }
        }
        Ok(())
    }

    fn overlay_turn_roots(&self, threads: &mut [ThreadExport], strict: bool) -> Result<()> {
        for thread in threads {
            let Some(turns) = self.turns.get(&thread.id) else {
                continue;
            };
            let by_key: HashMap<&str, &RetainedTurn> = turns
                .iter()
                .map(|turn| (turn.turn_id.as_str(), turn))
                .collect();
            let mut matched = HashSet::new();
            let mut changed = false;
            for row in thread
                .history_rows
                .get_mut("thread_turns")
                .into_iter()
                .flatten()
            {
                let Some(SqlValue::Text(turn_id)) = row.values.get("turn_id") else {
                    continue;
                };
                let Some(turn) = by_key.get(turn_id.as_str()) else {
                    continue;
                };
                if row.values.get("rollout_ordinal")
                    != Some(&SqlValue::Integer(turn.rollout_ordinal))
                {
                    if strict {
                        return Err(SpiceError::User(format!(
                            "A retained turn in chat '{}' changed identity. Push is paused to protect its history metadata. Pull can restore the incoming version.",
                            thread.title
                        )));
                    }
                    continue;
                }
                matched.insert(turn.turn_id.as_str());
                if non_null(row.values.get("root_turn_id")).is_none() {
                    row.values.insert(
                        "root_turn_id".into(),
                        SqlValue::Text(turn.root_turn_id.clone()),
                    );
                    changed = true;
                }
            }
            if strict && matched.len() != turns.len() {
                return Err(SpiceError::User(format!(
                    "Chat '{}' is missing a turn whose history metadata Spice Route retained. Push is paused to prevent a silent loss. Pull can restore the incoming version.",
                    thread.title
                )));
            }
            if changed {
                codex::refresh_thread_fingerprint(thread)?;
            }
        }
        Ok(())
    }
}

pub fn unsupported_columns(table: &str, history_migration: Option<i64>) -> &'static [&'static str] {
    match table {
        "thread_items" if history_migration.is_some_and(|v| v < 7) => {
            &["started_at_ms", "completed_at_ms"]
        }
        "thread_turns" if history_migration.is_some_and(|v| v < 8) => &["root_turn_id"],
        _ => &[],
    }
}

pub fn strip_unsupported_fields(rows: &mut [crate::models::DatabaseRow], columns: &[&str]) {
    for row in rows {
        for column in columns {
            row.values.remove(*column);
        }
    }
}

fn non_null(value: Option<&SqlValue>) -> Option<SqlValue> {
    value
        .filter(|value| !matches!(value, SqlValue::Null))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn version_one_timing_data_upgrades_without_loss_and_future_formats_are_rejected() {
        let directory = tempdir().unwrap();
        let mut expected = RetainedHistory::default();
        expected.threads.insert(
            "chat".into(),
            vec![RetainedItem {
                turn_id: "turn".into(),
                item_id: "item".into(),
                created_at_ms: SqlValue::Integer(1),
                started_at_ms: Some(SqlValue::Integer(10)),
                completed_at_ms: Some(SqlValue::Integer(20)),
            }],
        );
        let mut legacy = serde_json::to_value(&expected).unwrap();
        legacy["version"] = 1.into();
        legacy.as_object_mut().unwrap().remove("turns");
        write_json(&path(directory.path()), &legacy).unwrap();
        assert_eq!(load(directory.path(), true).unwrap(), expected);
        expected.stage(directory.path()).unwrap();
        assert_eq!(load(directory.path(), true).unwrap(), expected);
        legacy["version"] = 999.into();
        write_json(&path(directory.path()), &legacy).unwrap();
        assert!(load(directory.path(), true).is_err());
    }

    #[test]
    fn duplicate_retained_turns_are_rejected_when_loading_recovery_data() {
        let directory = tempdir().unwrap();
        let mut retained = RetainedHistory::default();
        let turn = RetainedTurn {
            turn_id: "turn".into(),
            rollout_ordinal: 1,
            root_turn_id: "root".into(),
        };
        retained
            .turns
            .insert("chat".into(), vec![turn.clone(), turn]);
        retained.stage(directory.path()).unwrap();
        assert!(load(directory.path(), true).is_err());
    }
}
