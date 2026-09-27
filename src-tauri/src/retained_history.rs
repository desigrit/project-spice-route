use crate::codex::{self, CodexExport};
use crate::error::{Result, SpiceError};
use crate::models::{SnapshotManifest, SqlValue, ThreadExport};
use crate::util::{read_json, write_json};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

const VERSION: u32 = 1;
const FILE_NAME: &str = "retained-history-fields.json";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetainedHistory {
    version: u32,
    threads: BTreeMap<String, Vec<RetainedItem>>,
}

impl Default for RetainedHistory {
    fn default() -> Self {
        Self {
            version: VERSION,
            threads: BTreeMap::new(),
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
    let value: RetainedHistory = read_json(&path).map_err(|error| {
        SpiceError::User(format!(
            "Retained history metadata could not be read at {}: {error}. Push and Pull are paused to protect newer Codex fields.",
            path.display()
        ))
    })?;
    value.validate()?;
    Ok(value)
}

impl RetainedHistory {
    pub fn has_entries(&self) -> bool {
        !self.threads.is_empty()
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

    pub fn overlay_export(&self, export: &mut CodexExport, strict: bool) -> Result<()> {
        self.overlay_threads(&mut export.threads, strict)
    }

    pub fn overlay_threads(&self, threads: &mut [ThreadExport], strict: bool) -> Result<()> {
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
}

pub fn strip_unsupported_timing(rows: &mut [crate::models::DatabaseRow]) {
    for row in rows {
        row.values.remove("started_at_ms");
        row.values.remove("completed_at_ms");
    }
}

fn non_null(value: Option<&SqlValue>) -> Option<SqlValue> {
    value
        .filter(|value| !matches!(value, SqlValue::Null))
        .cloned()
}
