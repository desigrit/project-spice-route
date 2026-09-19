//! Regression tests use schema metadata and disposable databases only.
use crate::codex;
use crate::compatibility;
use crate::models::{AppConfig, ObjectKind, ProjectMode, SnapshotManifest, SqlValue};
use crate::snapshot::{self, ObjectStore};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::{tempdir, TempDir};

const IMAGE_BYTES: &[u8] = b"disposable local image fixture";
const OPAQUE_PAYLOAD: &str = "Keep exact bytes: {not-json}\nC:\\historical\\file.txt";

fn create_home(home: &Path, migration: i64) {
    fs::create_dir_all(home).unwrap();
    let profile = compatibility::PROFILES
        .iter()
        .find(|profile| profile.state == migration)
        .expect("tested schema profile");
    let fixture: Value = serde_json::from_str(profile.schema).unwrap();
    for database in ["state_5.sqlite", "thread_history_1.sqlite"] {
        let db = Connection::open(home.join(database)).unwrap();
        let objects = fixture[database]["objects"].as_array().unwrap();
        for kind in ["table", "index", "trigger"] {
            for object in objects.iter().filter(|object| object["type"] == kind) {
                db.execute_batch(object["sql"].as_str().unwrap()).unwrap();
            }
        }
        db.execute(
            "INSERT INTO _sqlx_migrations(version, description, success, checksum, execution_time) VALUES(?1, 'fixture', 1, X'00', 0)",
            [fixture[database]["migration"].as_i64().unwrap()],
        )
        .unwrap();
    }
    assert!(codex::inspect(home).unwrap().supported);
}

fn attachment_layout(migration: i64) -> (&'static str, &'static str) {
    if migration == 55 {
        ("thread_attachments", "attachment_type")
    } else {
        ("thread_artifacts", "artifact_type")
    }
}

fn seed_thread(home: &Path, migration: i64, id: &str) -> PathBuf {
    let image = home.join("uploads").join(id).join("reference.png");
    fs::create_dir_all(image.parent().unwrap()).unwrap();
    fs::write(&image, IMAGE_BYTES).unwrap();
    let content = json!({
        "text": "A historical path stays C:\\historical\\file.txt",
        "content": [{"type": "localImage", "path": image.to_string_lossy()}]
    });
    let rollout = home.join("sessions").join(format!("{id}.jsonl"));
    fs::create_dir_all(rollout.parent().unwrap()).unwrap();
    fs::write(
        &rollout,
        format!("{}\n", json!({"thread_id": id, "payload": content})),
    )
    .unwrap();
    let state = Connection::open(home.join("state_5.sqlite")).unwrap();
    state.execute(
        "INSERT INTO threads(id, rollout_path, created_at, updated_at, cwd, title, name, first_user_message, preview, source, model_provider, sandbox_policy, approval_mode, agent_path)
         VALUES(?1, ?2, 1, 2, ?3, ?1, ?1, ?1, ?1, 'cli', 'openai', '{\"type\":\"disabled\"}', 'never', 'local-agent.toml')",
        params![id, rollout.to_string_lossy(), home.join("workspaces").join(id).to_string_lossy()],
    ).unwrap();
    let (table, type_column) = attachment_layout(migration);
    for (suffix, kind, payload, created_at) in [
        ("opaque", "file", OPAQUE_PAYLOAD.to_owned(), 3),
        ("image", "image", content.to_string(), 4),
    ] {
        state.execute(
            &format!("INSERT INTO {table}(id, thread_id, {type_column}, identity_key, payload, created_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6)"),
            params![format!("{id}-{suffix}"), id, kind, suffix, payload, created_at],
        ).unwrap();
    }
    let history = Connection::open(home.join("thread_history_1.sqlite")).unwrap();
    history.execute(
        "INSERT INTO thread_items(thread_id, turn_id, item_id, rollout_ordinal, created_at_ms, item_json, item_type, updated_at_ordinal) VALUES(?1, 'turn', 'item', 1, 1, ?2, 'message', 1)",
        params![id, content.to_string()],
    ).unwrap();
    history.execute(
        "INSERT INTO thread_turns(thread_id, turn_id, rollout_ordinal, status) VALUES(?1, 'turn', 1, 'completed')",
        [id],
    ).unwrap();
    history.execute(
        "INSERT INTO thread_history_projection_state(thread_id, next_rollout_byte_offset, next_rollout_ordinal) VALUES(?1, 1, 2)",
        [id],
    ).unwrap();
    history.execute(
        "INSERT INTO thread_realtime_items(thread_id, item_id, rollout_ordinal, created_at_ms, item_type, item_json) VALUES(?1, 'realtime', 1, 1, 'realtime_session_started', '{\"text\":\"realtime history\"}')",
        [id],
    ).unwrap();
    image
}

fn config(home: &Path) -> AppConfig {
    let mut config = crate::settings::default_config();
    config.codex_home = home.to_string_lossy().into_owned();
    config.projectless_root = home.join("workspaces").to_string_lossy().into_owned();
    config.selection.default_project_mode = ProjectMode::HistoryOnly;
    config
}

struct Captured {
    manifest: SnapshotManifest,
    store: ObjectStore,
    _directory: TempDir,
}

fn capture(home: &Path, excluded: &[&str]) -> Captured {
    let directory = tempdir().unwrap();
    let mut config = config(home);
    config.selection.excluded_thread_ids = excluded.iter().map(|id| (*id).into()).collect();
    let databases = directory.path().join("databases");
    codex::snapshot_databases(home, &databases).unwrap();
    let export = codex::export_selected(
        home,
        &databases,
        &config.selection,
        Path::new(&config.projectless_root),
    )
    .unwrap();
    let store = ObjectStore::new(directory.path().join("objects")).unwrap();
    let mut manifest = snapshot::build_manifest(&config, export, &store, None).unwrap();
    manifest.codex_version = Some(
        compatibility::profile(&manifest.compatibility)
            .unwrap()
            .runtime
            .into(),
    );
    // Exercise the on-disk portable representation, including table and column keys.
    let manifest = serde_json::from_slice(&serde_json::to_vec(&manifest).unwrap()).unwrap();
    Captured {
        manifest,
        store,
        _directory: directory,
    }
}

fn apply_rows(
    home: &Path,
    manifest: &SnapshotManifest,
    deleted: &[&str],
    attachment_paths: &HashMap<String, HashMap<String, String>>,
) -> crate::error::Result<()> {
    codex::apply_bundle(
        home,
        manifest,
        &manifest
            .threads
            .iter()
            .map(|thread| thread.id.clone())
            .collect(),
        &manifest
            .projects
            .iter()
            .map(|project| project.id.clone())
            .collect(),
        &deleted.iter().map(|id| (*id).into()).collect(),
        &HashSet::new(),
        &config(home),
        &manifest
            .threads
            .iter()
            .map(|thread| {
                (
                    thread.id.clone(),
                    home.join("sessions")
                        .join(format!("{}.jsonl", thread.id))
                        .to_string_lossy()
                        .into_owned(),
                )
            })
            .collect(),
        attachment_paths,
    )
}

fn restore(home: &Path, captured: &Captured, deleted: &[&str]) {
    let mut replacements: HashMap<String, HashMap<String, String>> = HashMap::new();
    for object in captured
        .manifest
        .objects
        .iter()
        .filter(|object| object.kind == ObjectKind::Artifact)
    {
        let thread = captured
            .manifest
            .threads
            .iter()
            .find(|thread| thread.id == object.owner_id)
            .unwrap();
        let reference = thread
            .attachments
            .iter()
            .find(|reference| reference.logical_path == object.logical_path)
            .unwrap();
        let destination = home
            .join("restored-attachments")
            .join(&thread.id)
            .join(Path::new(&object.logical_path).file_name().unwrap());
        captured.store.materialize(object, &destination).unwrap();
        replacements.entry(thread.id.clone()).or_default().insert(
            reference.source_path.clone(),
            destination.to_string_lossy().into_owned(),
        );
    }
    for object in captured
        .manifest
        .objects
        .iter()
        .filter(|object| object.kind == ObjectKind::Rollout)
    {
        let destination = home
            .join("sessions")
            .join(format!("{}.jsonl", object.owner_id));
        captured.store.materialize(object, &destination).unwrap();
        if let Some(paths) = replacements.get(&object.owner_id) {
            codex::rewrite_rollout_local_image_paths(&destination, paths).unwrap();
        }
    }
    apply_rows(home, &captured.manifest, deleted, &replacements).unwrap();
    codex::verify_databases(home).unwrap();
}

fn assert_restored_content(home: &Path, migration: i64) {
    let (table, type_column) = attachment_layout(migration);
    let state = Connection::open(home.join("state_5.sqlite")).unwrap();
    let mut query = state.prepare(&format!("SELECT id, {type_column}, identity_key, payload, created_at FROM {table} WHERE thread_id='portable' ORDER BY created_at")).unwrap();
    let records: Vec<(String, String, String, String, i64)> = query
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        records.len(),
        2,
        "repeated imports must replace attachment records"
    );
    assert_eq!(
        records[0],
        (
            "portable-opaque".into(),
            "file".into(),
            "opaque".into(),
            OPAQUE_PAYLOAD.into(),
            3
        )
    );
    assert_eq!(
        (&records[1].0, &records[1].1, &records[1].2, records[1].4),
        (
            &"portable-image".to_owned(),
            &"image".to_owned(),
            &"image".to_owned(),
            4
        )
    );
    let payload: Value = serde_json::from_str(&records[1].3).unwrap();
    let image = payload
        .pointer("/content/0/path")
        .unwrap()
        .as_str()
        .unwrap();
    assert!(Path::new(image).starts_with(home.join("restored-attachments")));
    assert_eq!(fs::read(image).unwrap(), IMAGE_BYTES);
    assert_eq!(
        payload["text"],
        "A historical path stays C:\\historical\\file.txt"
    );
    let history = Connection::open(home.join("thread_history_1.sqlite")).unwrap();
    let item: String = history
        .query_row(
            "SELECT item_json FROM thread_items WHERE thread_id='portable'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(serde_json::from_str::<Value>(&item).unwrap(), payload);
    let rollout: Value = serde_json::from_str(
        fs::read_to_string(home.join("sessions/portable.jsonl"))
            .unwrap()
            .trim(),
    )
    .unwrap();
    assert_eq!(rollout["payload"], payload);
    for table in [
        "thread_items",
        "thread_turns",
        "thread_history_projection_state",
        "thread_realtime_items",
    ] {
        let count: i64 = history
            .query_row(
                &format!("SELECT count(*) FROM {table} WHERE thread_id='portable'"),
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            count, 1,
            "{table} must retain one complete history projection"
        );
    }
}

#[test]
fn schema55_transfer_matrix_preserves_records_images_selection_and_destination() {
    for (source_version, destination_version) in [(54, 55), (55, 54), (55, 55), (55, 52), (52, 55)]
    {
        let source = tempdir().unwrap();
        let destination = tempdir().unwrap();
        create_home(source.path(), source_version);
        create_home(destination.path(), destination_version);
        seed_thread(source.path(), source_version, "portable");
        seed_thread(source.path(), source_version, "excluded");
        seed_thread(destination.path(), destination_version, "local");
        seed_thread(destination.path(), destination_version, "portable");
        let state = Connection::open(destination.path().join("state_5.sqlite")).unwrap();
        let (table, type_column) = attachment_layout(destination_version);
        state.execute(&format!("UPDATE {table} SET payload='stale destination payload' WHERE thread_id='portable'"), []).unwrap();
        state.execute(&format!("INSERT INTO {table}(id, thread_id, {type_column}, identity_key, payload, created_at) VALUES('stale-only', 'portable', 'file', 'stale-only', 'remove this stale attachment', 5)"), []).unwrap();
        state.execute("UPDATE threads SET sandbox_policy='destination policy', approval_mode='destination approval', agent_path='destination-agent.toml' WHERE id='portable'", []).unwrap();
        state.execute("INSERT INTO projects(id, name, position, created_at_ms, updated_at_ms) VALUES('local-project', 'Unrelated project', 0, 1, 1)", []).unwrap();
        drop(state);
        for (file, contents) in [
            ("auth.json", "{\"token\":\"disposable-destination-token\"}"),
            ("config.toml", "profile = 'destination'\n"),
        ] {
            fs::write(destination.path().join(file), contents).unwrap();
        }
        fs::write(
            destination.path().join(".codex-global-state.json"),
            json!({"queued-follow-ups": {"local": "keep"}, "use-copilot-auth-if-available": true})
                .to_string(),
        )
        .unwrap();
        let local_before = capture(destination.path(), &["portable"]);
        let compatibility_before = codex::inspect(destination.path()).unwrap();
        let captured = capture(source.path(), &["excluded"]);
        codex::validate_snapshot_source(&captured.manifest).unwrap();
        assert!(codex::transfer_issues(&captured.manifest, &compatibility_before).is_empty());
        assert_eq!(captured.manifest.threads.len(), 1);
        assert_eq!(captured.manifest.threads[0].attachments.len(), 1);
        assert_eq!(captured.manifest.objects.len(), 2);
        assert!(captured
            .manifest
            .objects
            .iter()
            .all(|object| object.owner_id == "portable"));
        assert_eq!(
            captured.manifest.threads[0].state_rows["thread_artifacts"].len(),
            2
        );
        assert!(!captured.manifest.threads[0]
            .state_rows
            .contains_key("thread_attachments"));
        for row in &captured.manifest.threads[0].state_rows["thread_artifacts"] {
            assert!(row.values.contains_key("artifact_type"));
            assert!(!row.values.contains_key("attachment_type"));
        }
        restore(destination.path(), &captured, &[]);
        restore(destination.path(), &captured, &[]);
        assert_restored_content(destination.path(), destination_version);
        let compatibility_after = codex::inspect(destination.path()).unwrap();
        assert!(compatibility_after.supported);
        assert_eq!(
            compatibility_after.state_migration,
            Some(destination_version)
        );
        assert_eq!(compatibility_after.history_migration, Some(6));
        assert_eq!(
            compatibility_before.schema_fingerprint,
            compatibility_after.schema_fingerprint
        );
        let state = Connection::open(destination.path().join("state_5.sqlite")).unwrap();
        let count: i64 = state
            .query_row("SELECT count(*) FROM threads", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 2);
        let count: i64 = state.query_row("SELECT count(*) FROM projects WHERE id='local-project' AND name='Unrelated project'", [], |row| row.get(0)).unwrap();
        assert_eq!(count, 1);
        let local_fields: (String, String, String) = state
            .query_row(
                "SELECT sandbox_policy, approval_mode, agent_path FROM threads WHERE id='portable'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            local_fields,
            (
                "destination policy".into(),
                "destination approval".into(),
                "destination-agent.toml".into()
            )
        );
        let ledger: (i64, String, i64, Vec<u8>, i64) = state.query_row("SELECT version, description, success, checksum, execution_time FROM _sqlx_migrations", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))).unwrap();
        assert_eq!(
            ledger,
            (destination_version, "fixture".into(), 1, vec![0], 0)
        );
        drop(state);
        assert_eq!(
            fs::read_to_string(destination.path().join("auth.json")).unwrap(),
            "{\"token\":\"disposable-destination-token\"}"
        );
        assert_eq!(
            fs::read_to_string(destination.path().join("config.toml")).unwrap(),
            "profile = 'destination'\n"
        );
        let ui: Value = serde_json::from_slice(
            &fs::read(destination.path().join(".codex-global-state.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(ui["queued-follow-ups"]["local"], "keep");
        assert_eq!(ui["use-copilot-auth-if-available"], true);
        let local_after = capture(destination.path(), &["portable"]);
        assert_eq!(
            serde_json::to_value(&local_before.manifest.threads[0]).unwrap(),
            serde_json::to_value(&local_after.manifest.threads[0]).unwrap()
        );
        let returned = capture(destination.path(), &["local"]);
        assert_eq!(
            returned.manifest.threads[0].fingerprint, captured.manifest.threads[0].fingerprint,
            "{source_version} -> {destination_version}"
        );
        restore(source.path(), &returned, &[]);
        assert_restored_content(source.path(), source_version);
        assert_eq!(
            capture(source.path(), &["excluded"]).manifest.threads[0].fingerprint,
            captured.manifest.threads[0].fingerprint,
            "{source_version} -> {destination_version} -> {source_version}"
        );
    }
}

#[test]
fn schema54_and_55_export_identical_semantic_fingerprints() {
    let old = tempdir().unwrap();
    let new = tempdir().unwrap();
    create_home(old.path(), 54);
    create_home(new.path(), 55);
    seed_thread(old.path(), 54, "portable");
    seed_thread(new.path(), 55, "portable");
    let old_capture = capture(old.path(), &[]);
    let new_capture = capture(new.path(), &[]);
    assert_eq!(
        old_capture.manifest.threads[0].fingerprint,
        new_capture.manifest.threads[0].fingerprint
    );
    let state = Connection::open(new.path().join("state_5.sqlite")).unwrap();
    state.execute("UPDATE thread_attachments SET attachment_type='different-kind' WHERE id='portable-opaque'", []).unwrap();
    assert_ne!(
        old_capture.manifest.threads[0].fingerprint,
        capture(new.path(), &[]).manifest.threads[0].fingerprint,
        "attachment type is part of semantic identity"
    );
    state.execute("UPDATE thread_attachments SET attachment_type='file', payload='changed payload' WHERE id='portable-opaque'", []).unwrap();
    assert_ne!(
        old_capture.manifest.threads[0].fingerprint,
        capture(new.path(), &[]).manifest.threads[0].fingerprint,
        "attachment payload is part of semantic identity"
    );
}

#[test]
fn schema55_deletion_clears_attachment_rows_without_touching_unrelated_threads() {
    let home = tempdir().unwrap();
    create_home(home.path(), 55);
    seed_thread(home.path(), 55, "portable");
    seed_thread(home.path(), 55, "local");
    let empty = capture(home.path(), &["portable", "local"]);
    assert!(empty.manifest.threads.is_empty());
    restore(home.path(), &empty, &["portable"]);
    restore(home.path(), &empty, &["portable"]);
    let state = Connection::open(home.path().join("state_5.sqlite")).unwrap();
    for (table, column) in [("threads", "id"), ("thread_attachments", "thread_id")] {
        let deleted: i64 = state
            .query_row(
                &format!("SELECT count(*) FROM {table} WHERE {column}='portable'"),
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(deleted, 0);
    }
    let retained: i64 = state
        .query_row(
            "SELECT count(*) FROM thread_attachments WHERE thread_id='local'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(retained, 2);
    let history = Connection::open(home.path().join("thread_history_1.sqlite")).unwrap();
    for table in [
        "thread_items",
        "thread_realtime_items",
        "thread_turns",
        "thread_history_projection_state",
    ] {
        let deleted: i64 = history
            .query_row(
                &format!("SELECT count(*) FROM {table} WHERE thread_id='portable'"),
                [],
                |row| row.get(0),
            )
            .unwrap();
        let retained: i64 = history
            .query_row(
                &format!("SELECT count(*) FROM {table} WHERE thread_id='local'"),
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!((deleted, retained), (0, 1));
    }
}

#[test]
fn schema55_nonnull_fields_block_downgrade_before_database_or_ui_mutation() {
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    create_home(source.path(), 55);
    create_home(destination.path(), 52);
    seed_thread(source.path(), 55, "portable");
    seed_thread(destination.path(), 52, "local");
    fs::write(
        destination.path().join(".codex-global-state.json"),
        "{\"local\":true}",
    )
    .unwrap();
    let state = Connection::open(source.path().join("state_5.sqlite")).unwrap();
    for (originator, daybreak) in [(Some("desktop"), None), (None, Some(0)), (None, Some(1))] {
        state
            .execute(
                "UPDATE threads SET originator=?1, daybreak_enabled=?2",
                params![originator, daybreak],
            )
            .unwrap();
        let captured = capture(source.path(), &[]);
        let issues = codex::transfer_issues(
            &captured.manifest,
            &codex::inspect(destination.path()).unwrap(),
        );
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("portable"));
        let files = [
            "state_5.sqlite",
            "thread_history_1.sqlite",
            ".codex-global-state.json",
        ];
        let before: Vec<_> = files
            .iter()
            .map(|file| fs::read(destination.path().join(file)).unwrap())
            .collect();
        assert!(apply_rows(
            destination.path(),
            &captured.manifest,
            &["local"],
            &HashMap::new()
        )
        .is_err());
        let after: Vec<_> = files
            .iter()
            .map(|file| fs::read(destination.path().join(file)).unwrap())
            .collect();
        assert_eq!(
            before, after,
            "downgrade preflight must run before requested deletion"
        );
        assert!(capture(source.path(), &["portable"])
            .manifest
            .threads
            .is_empty());
    }
}

#[test]
fn schema55_complete_layout_is_required_and_runtime_is_advisory() {
    let home = tempdir().unwrap();
    create_home(home.path(), 55);
    let info = codex::inspect(home.path()).unwrap();
    assert!(codex::with_build_gate(info.clone(), Some("codex-cli 0.155.0-alpha.9.2")).supported);
    for version in [None, Some("0.154.0-alpha.6.2"), Some("0.155.0-alpha.9.3")] {
        assert!(codex::with_build_gate(info.clone(), version).supported);
    }
    let state = Connection::open(home.path().join("state_5.sqlite")).unwrap();
    state
        .execute_batch("DROP INDEX idx_thread_attachments_thread_created_id")
        .unwrap();
    assert!(
        !codex::inspect(home.path()).unwrap().supported,
        "the renamed table alone is insufficient to accept schema55"
    );
}

#[test]
fn schema55_preserves_older_missing_fields_and_newer_explicit_nulls() {
    let old = tempdir().unwrap();
    let new = tempdir().unwrap();
    let peer = tempdir().unwrap();
    create_home(old.path(), 52);
    create_home(new.path(), 55);
    create_home(peer.path(), 54);
    seed_thread(old.path(), 52, "portable");
    seed_thread(new.path(), 55, "portable");
    let state = Connection::open(new.path().join("state_5.sqlite")).unwrap();
    state
        .execute(
            "UPDATE threads SET originator='desktop', daybreak_enabled=1",
            [],
        )
        .unwrap();
    restore(new.path(), &capture(old.path(), &[]), &[]);
    let captured = capture(new.path(), &[]);
    let fields = &captured.manifest.threads[0].state_rows["threads"][0].values;
    assert!(matches!(&fields["originator"], SqlValue::Text(value) if value == "desktop"));
    assert!(matches!(fields["daybreak_enabled"], SqlValue::Integer(1)));
    restore(peer.path(), &captured, &[]);
    assert_eq!(
        capture(peer.path(), &[]).manifest.threads[0].fingerprint,
        captured.manifest.threads[0].fingerprint
    );
    state
        .execute(
            "UPDATE threads SET originator=NULL, daybreak_enabled=NULL",
            [],
        )
        .unwrap();
    restore(peer.path(), &capture(new.path(), &[]), &[]);
    let cleared = capture(peer.path(), &[]);
    let fields = &cleared.manifest.threads[0].state_rows["threads"][0].values;
    assert!(matches!(fields["originator"], SqlValue::Null));
    assert!(matches!(fields["daybreak_enabled"], SqlValue::Null));
}
