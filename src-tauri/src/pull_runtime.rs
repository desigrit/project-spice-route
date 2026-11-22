//! Runtime compatibility is directional. A portable text chat has an older reader
//! floor than a chat containing events whose meaning is known only to its producer.
use crate::error::{Result, SpiceError};
use crate::models::{DatabaseRow, ObjectKind, SnapshotManifest, SqlValue, ThreadExport};
use semver::Version;
use serde_json::Value;
use std::collections::HashSet;

// Earliest Codex runtime represented by a tested Spice Route storage profile.
const BASIC_READER: Version = Version::new(0, 153, 4);

fn version(label: Option<&str>) -> Option<Version> {
    let label = label?.trim();
    let number = label.strip_prefix("codex-cli ").unwrap_or(label).trim();
    Version::parse(number).ok()
}

fn text<'a>(row: &'a DatabaseRow, column: &str) -> Option<&'a str> {
    match row.values.get(column)? {
        SqlValue::Text(value) => Some(value),
        _ => None,
    }
}

fn has_keys(value: &Value, allowed: &[&str]) -> bool {
    value
        .as_object()
        .is_some_and(|fields| fields.keys().all(|key| allowed.contains(&key.as_str())))
}

fn basic_part(value: &Value) -> bool {
    has_keys(value, &["type", "text", "text_elements", "textElements"])
        && matches!(
            value["type"].as_str(),
            Some("text" | "input_text" | "output_text")
        )
        && value["text"].is_string()
        && ["text_elements", "textElements"].iter().all(|key| {
            value
                .get(*key)
                .map_or(true, |v| v.as_array().is_some_and(Vec::is_empty))
        })
}

fn basic_message(value: &Value) -> bool {
    match value["type"].as_str() {
        Some("agentMessage" | "agent_message") => {
            has_keys(value, &["type", "id", "text"]) && value["text"].is_string()
        }
        Some("userMessage" | "user_message") => {
            has_keys(value, &["type", "id", "content"])
                && value["content"]
                    .as_array()
                    .is_some_and(|parts| parts.iter().all(basic_part))
        }
        Some("message") => {
            has_keys(value, &["type", "id", "role", "content"])
                && matches!(
                    value["role"].as_str(),
                    Some("user" | "assistant" | "developer" | "system")
                )
                && value["content"]
                    .as_array()
                    .is_some_and(|parts| parts.iter().all(basic_part))
        }
        None => has_keys(value, &["text"]) && value["text"].is_string(),
        _ => false,
    }
}

pub(crate) fn basic_rollout_line(line: &Value) -> bool {
    if !has_keys(line, &["timestamp", "type", "payload"]) {
        return false;
    }
    let payload = &line["payload"];
    match line["type"].as_str() {
        Some("response_item") => basic_message(payload),
        Some("session_meta") => {
            has_keys(
                payload,
                &[
                    "id",
                    "timestamp",
                    "cwd",
                    "originator",
                    "cli_version",
                    "source",
                    "model_provider",
                ],
            ) && payload["id"].is_string()
                && payload
                    .as_object()
                    .is_some_and(|obj| obj.values().all(Value::is_string))
        }
        Some("event_msg") => match payload["type"].as_str() {
            Some("agent_message") => {
                has_keys(payload, &["type", "message"]) && payload["message"].is_string()
            }
            Some("user_message") => {
                has_keys(
                    payload,
                    &["type", "message", "images", "local_images", "text_elements"],
                ) && payload["message"].is_string()
                    && ["images", "local_images", "text_elements"]
                        .iter()
                        .all(|key| {
                            payload.get(*key).map_or(true, |v| {
                                v.is_null() || v.as_array().is_some_and(Vec::is_empty)
                            })
                        })
            }
            _ => false,
        },
        _ => false,
    }
}

fn basic_thread(thread: &ThreadExport) -> bool {
    if !thread.attachments.is_empty() {
        return false;
    }
    let simple_state = thread
        .state_rows
        .get("threads")
        .is_some_and(|rows| rows.len() == 1)
        && thread.state_rows.iter().all(|(table, rows)| {
            rows.is_empty()
                || (table == "threads"
                    && rows.iter().all(|row| {
                        row.values
                            .get("attached_thread_parent_id")
                            .map_or(true, |v| *v == SqlValue::Null)
                            && text(row, "source").is_some_and(|source| {
                                matches!(source, "cli" | "vscode" | "exec" | "appServer")
                            })
                    }))
        });
    simple_state
        && thread.history_rows.iter().all(|(table, rows)| {
            rows.is_empty()
                || match table.as_str() {
                    "thread_items" => rows.iter().all(|row| {
                        matches!(
                            text(row, "item_type"),
                            Some(
                                "" | "message"
                                    | "userMessage"
                                    | "agentMessage"
                                    | "user_message"
                                    | "agent_message"
                            )
                        ) && text(row, "item_json")
                            .and_then(|item| serde_json::from_str(item).ok())
                            .is_some_and(|item| basic_message(&item))
                    }),
                    "thread_turns" => rows.iter().all(|row| {
                        matches!(text(row, "status"), Some("completed" | "interrupted"))
                            && row
                                .values
                                .get("error_json")
                                .map_or(true, |v| *v == SqlValue::Null)
                    }),
                    "thread_history_projection_state" => true,
                    _ => false,
                }
        })
}

fn problem(message: String) -> SpiceError {
    SpiceError::User(message)
}

fn can_read(receiver: &Version, source: Option<&Version>) -> bool {
    source.is_some_and(|source| {
        receiver.major == source.major && !receiver.cmp_precedence(source).is_lt()
    })
}

fn needs_reader(item: String, producer: Option<&Version>, receiver: &Version) -> Result<()> {
    match producer {
        Some(source) if source.major == receiver.major => Err(problem(format!(
            "{item} needs Codex runtime {source} or later to restore its saved data. This device has {receiver}. Update Codex here, then refresh Pull."
        ))),
        Some(source) => Err(problem(format!(
            "{item} uses a runtime family Spice Route cannot match to this device. The source used Codex {source} and this device has {receiver}. Use matching Codex versions, then refresh Pull."
        ))),
        None => Err(problem(format!(
            "{item} uses data that requires a known source runtime. Open Codex on the source device, quit it, and create a fresh Push so Spice Route can record its version."
        ))),
    }
}

/// Called on the header before verifying objects, and again when applying a review.
/// Older manifests lack transcript evidence and conservatively use the source reader.
pub fn check(snapshot: &SnapshotManifest, receiver_label: Option<&str>) -> Result<()> {
    let receiver = version(receiver_label).ok_or_else(|| problem(
        "Pull needs the version of Codex on this device. Open Codex once, quit it, then refresh. If you use a custom install, set CODEX_INSTALL_DIR to its runtime.".into()
    ))?;
    if receiver.cmp_precedence(&BASIC_READER).is_lt() {
        return Err(problem(format!("This device has Codex runtime {receiver}. Update it to {BASIC_READER} or later before pulling.")));
    }
    let producer = version(snapshot.codex_version.as_deref());
    let rollouts: HashSet<_> = snapshot
        .objects
        .iter()
        .filter(|entry| entry.kind == ObjectKind::Rollout)
        .map(|entry| entry.owner_id.as_str())
        .collect();
    for thread in &snapshot.threads {
        // Separate CLI installations and the desktop app can write the same home.
        // A row can identify a newer writer than the desktop version at Push time.
        let writer = thread
            .state_rows
            .get("threads")
            .and_then(|rows| rows.first())
            .and_then(|row| version(text(row, "cli_version")));
        let source = producer.as_ref().map(|desktop| {
            writer
                .as_ref()
                .filter(|writer| writer.cmp_precedence(desktop).is_gt())
                .unwrap_or(desktop)
        });
        if can_read(&receiver, source) {
            continue;
        }
        let has_rollout =
            rollouts.contains(thread.id.as_str()) || thread.rollout_relative_path.is_some();
        if !basic_thread(thread)
            || (has_rollout && (snapshot.schema_version < 3 || !thread.basic_text_rollout))
        {
            return needs_reader(format!("Chat '{}'", thread.title), source, &receiver);
        }
    }
    if can_read(&receiver, producer.as_ref()) {
        return Ok(());
    }
    let project = snapshot.projects.iter().find(|project| {
        project
            .rows
            .get("projects")
            .into_iter()
            .flatten()
            .any(|row| {
                text(row, "metadata")
                    .and_then(|metadata| serde_json::from_str::<Value>(metadata).ok())
                    .map_or(true, |metadata| {
                        !metadata.as_object().is_some_and(serde_json::Map::is_empty)
                    })
            })
    });
    if let Some(project) = project {
        return needs_reader(
            format!("Project '{}'", project.name),
            producer.as_ref(),
            &receiver,
        );
    }
    Ok(())
}
