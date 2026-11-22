#[test]
fn automatic_schema_extension_preserves_destination_values_and_validates_source_witness() {
    let source = tempdir().unwrap();
    let peer = tempdir().unwrap();
    let version = crate::compatibility::PROFILES
        .iter()
        .map(|profile| profile.state)
        .max()
        .unwrap();
    for path in [source.path(), peer.path()] {
        create_profile_schema(path, version);
        insert_thread(path, "selected", "Chat", path, None, "history");
    }
    let base = inspect(source.path()).unwrap();
    let db = Connection::open(source.path().join("state_5.sqlite")).unwrap();
    db.execute_batch("ALTER TABLE threads ADD COLUMN future_hint TEXT; CREATE INDEX future_lookup ON threads(updated_at DESC, id)").unwrap();
    db.execute("UPDATE _sqlx_migrations SET version = version + 1", [])
        .unwrap();
    let adapted = inspect(source.path()).unwrap();
    assert!(adapted.supported, "{}", adapted.explanation);
    assert_ne!(adapted.schema_fingerprint, base.schema_fingerprint);
    assert_eq!(
        crate::compatibility::profile(&adapted).unwrap().state,
        version
    );
    let handoff = fixture_manifest(source.path(), vec![]);
    validate_snapshot_source(&handoff).unwrap();
    assert!(!handoff.threads[0].state_rows["threads"][0]
        .values
        .contains_key("future_hint"));
    import_fixture(peer.path(), &handoff, HashSet::new()).unwrap();

    let mut wire = serde_json::to_value(&handoff).unwrap();
    let index = wire["compatibility"]["schemaExtensions"]["state"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|obj| obj["type"] == "index")
        .unwrap();
    index["sql"] = serde_json::json!("CREATE UNIQUE INDEX future_lookup ON threads(id)");
    assert!(validate_snapshot_source(&serde_json::from_value(wire).unwrap()).is_err());

    db.execute(
        "UPDATE threads SET future_hint = 'keep local' WHERE id = 'selected'",
        [],
    )
    .unwrap();
    let config = crate::settings::default_config();
    let projectless = source.path().join("workspaces");
    let error = export_selected(
        source.path(),
        source.path(),
        &config.selection,
        &projectless,
    )
    .unwrap_err();
    assert!(error.to_string().contains("future_hint"));
    let current = export_for_capture(
        source.path(),
        source.path(),
        &config.selection,
        &projectless,
        false,
    )
    .unwrap();
    assert_eq!(
        row_string(&current.threads[0].state_rows["threads"][0], "future_hint"),
        Some("keep local")
    );
    let old = fixture_manifest(peer.path(), vec![]);
    import_fixture(source.path(), &old, HashSet::new()).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT future_hint FROM threads WHERE id='selected'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "keep local"
    );
    let mut incoming = old;
    incoming.threads[0].state_rows.get_mut("threads").unwrap()[0]
        .values
        .insert("future_hint".into(), SqlValue::Text("overwrite".into()));
    assert!(import_fixture(source.path(), &incoming, HashSet::new()).is_err());
    assert!(fixture_manifest(source.path(), vec!["selected".into()])
        .threads
        .is_empty());
}

#[test]
fn automatic_schema_extension_blocks_constraints_triggers_and_unclassified_content() {
    let base = crate::compatibility::PROFILES
        .iter()
        .map(|profile| profile.state)
        .max()
        .unwrap();
    for change in [
        "CREATE UNIQUE INDEX future_lookup ON threads(title)",
        "CREATE INDEX future_lookup ON threads(lower(title))",
        "CREATE INDEX future_lookup ON threads(title COLLATE NOCASE)",
        "CREATE INDEX future_lookup ON threads(title) WHERE archived = 0",
        "CREATE TABLE future_messages(thread_id TEXT, content TEXT)",
        "CREATE VIEW sqliteXshadow AS SELECT id FROM threads",
        "ALTER TABLE threads ADD COLUMN future_hint TEXT DEFAULT 'default'",
        "ALTER TABLE threads ADD COLUMN future_hint TEXT REFERENCES projects(id)",
        "CREATE TRIGGER future_trigger AFTER DELETE ON threads BEGIN DELETE FROM projects; END",
        "CREATE INDEX future_lookup ON threads(id); UPDATE _sqlx_migrations SET success=0",
    ] {
        let home = tempdir().unwrap();
        create_profile_schema(home.path(), base);
        let db = Connection::open(home.path().join("state_5.sqlite")).unwrap();
        db.execute_batch(change).unwrap();
        assert!(!inspect(home.path()).unwrap().supported, "{change}");
    }
}

#[test]
fn automatic_schema_extension_uses_reviewed_local_table_deletion_on_pull() {
    // Prove a signed-rule classification through the real transfer path using
    // the same reviewed table SQL that ships in the bundled rule pack.
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    for home in [source.path(), destination.path()] {
        create_profile_schema(home, 60);
        insert_thread(home, "keep", "Keep", home, None, "history");
    }
    insert_thread(
        destination.path(),
        "remove",
        "Remove",
        destination.path(),
        None,
        "history",
    );
    let fixture: JsonValue = serde_json::from_str(include_str!("fixtures/schema-61.json")).unwrap();
    let receipt = fixture["state_5.sqlite"]["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["name"] == "thread_read_receipts")
        .unwrap();
    let state = Connection::open(destination.path().join("state_5.sqlite")).unwrap();
    state
        .execute_batch(receipt["sql"].as_str().unwrap())
        .unwrap();
    state
        .execute_batch("CREATE INDEX local_receipt_revision ON thread_read_receipts(revision)")
        .unwrap();
    state.execute_batch("INSERT INTO thread_read_receipts(thread_id,revision) VALUES ('keep','kept'), ('remove','removed')").unwrap();
    assert!(inspect(destination.path()).unwrap().supported);
    let handoff = fixture_manifest(destination.path(), vec![]);
    validate_snapshot_source(&handoff).unwrap();
    assert!(handoff
        .threads
        .iter()
        .all(|chat| !chat.state_rows.contains_key("thread_read_receipts")));
    import_fixture(
        destination.path(),
        &fixture_manifest(source.path(), vec![]),
        HashSet::from(["remove".into()]),
    )
    .unwrap();
    let receipts: Vec<(String, String)> = state
        .prepare("SELECT thread_id,revision FROM thread_read_receipts")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(receipts, vec![("keep".into(), "kept".into())]);
}

#[test]
fn automatic_schema_extension_protects_unknown_item_and_turn_values_before_replacing_history() {
    let home = tempdir().unwrap();
    let older = tempdir().unwrap();
    for path in [home.path(), older.path()] {
        create_profile_schema(path, 64);
        insert_thread(path, "old", "Old chat", path, None, r#"{"text":"hello"}"#);
    }
    let history = Connection::open(home.path().join("thread_history_1.sqlite")).unwrap();
    history.execute_batch("ALTER TABLE thread_items ADD COLUMN new_counter INTEGER; ALTER TABLE thread_turns ADD COLUMN new_label TEXT; UPDATE _sqlx_migrations SET version=10").unwrap();
    let adapted = inspect(home.path()).unwrap();
    assert!(adapted.supported, "{}", adapted.explanation);
    let incoming = fixture_manifest(older.path(), vec![]);
    import_fixture(home.path(), &incoming, HashSet::new()).unwrap();
    let outgoing = fixture_manifest(home.path(), vec![]);
    validate_snapshot_source(&outgoing).unwrap();
    assert_eq!(
        outgoing.threads[0].fingerprint,
        incoming.threads[0].fingerprint
    );
    import_fixture(older.path(), &outgoing, HashSet::new()).unwrap();

    let config = crate::settings::default_config();
    for (table, field) in [
        ("thread_items", "new_counter"),
        ("thread_turns", "new_label"),
    ] {
        history
            .execute(
                &format!("UPDATE {table} SET {field}=42 WHERE thread_id='old'"),
                [],
            )
            .unwrap();
        let error =
            export_selected(home.path(), home.path(), &config.selection, home.path()).unwrap_err();
        assert!(error.to_string().contains(field));
        let error = import_fixture(home.path(), &incoming, HashSet::new()).unwrap_err();
        assert!(error.to_string().contains(field));
        let populated = history
            .query_row(
                &format!("SELECT count(*) FROM {table} WHERE {field} IS NOT NULL"),
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap();
        assert_eq!(populated, 1);
        history
            .execute(&format!("UPDATE {table} SET {field}=NULL"), [])
            .unwrap();
    }
}
