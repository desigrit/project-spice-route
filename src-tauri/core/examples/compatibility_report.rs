#![cfg_attr(windows, windows_subsystem = "windows")]
// Portable metadata-only report. Never constructs Engine or opens a database for writing.
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};
use spice_route_core::{codex, platform, settings};
use std::path::{Path, PathBuf};

fn schema(path: &Path) -> Result<Value, Box<dyn std::error::Error>> {
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    db.busy_timeout(std::time::Duration::from_secs(5))?;
    let mut query = db.prepare("SELECT type, name, coalesce(sql, '') FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name")?;
    let objects = query
        .query_map([], |row| {
            Ok(json!({
                "type": row.get::<_, String>(0)?, "name": row.get::<_, String>(1)?,
                "sql": row.get::<_, String>(2)?
            }))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let migration: Option<i64> = db.query_row(
        "SELECT max(version) FROM _sqlx_migrations WHERE success = 1",
        [],
        |row| row.get(0),
    )?;
    Ok(json!({"migration": migration, "objects": objects}))
}

fn report(home: Option<PathBuf>) -> Value {
    let executable = platform::find_codex_executable();
    let version = platform::codex_version(executable.as_deref());
    let mut result = json!({
        "reportFormat": 1, "codexHome": home, "codexExecutable": executable,
        "codexVersion": version,
        "scope": "Database schema and version metadata only. No conversation records, credentials, or workspace contents."
    });
    if let Some(home) = home {
        result["compatibility"] = match codex::inspect(&home) {
            Ok(info) => json!(codex::with_build_gate(info, version.as_deref())),
            Err(error) => json!({"error": error.to_string()}),
        };
        for filename in ["state_5.sqlite", "thread_history_1.sqlite"] {
            result[filename] = match schema(&home.join(filename)) {
                Ok(value) => value,
                Err(error) => json!({"error": error.to_string()}),
            };
        }
    }
    result
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let home = args
        .next()
        .map(PathBuf::from)
        .or_else(settings::discover_codex_home);
    let output = args.next().map(PathBuf::from).unwrap_or_else(|| {
        std::env::current_exe()
            .unwrap_or_default()
            .with_file_name("spice-route-compatibility.json")
    });
    std::fs::write(output, serde_json::to_vec_pretty(&report(home))?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reports_structure_without_reading_conversation_rows() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("test.sqlite");
        let db = Connection::open(&path).unwrap();
        db.execute_batch(
            "CREATE TABLE _sqlx_migrations(version INTEGER, success INTEGER);
            INSERT INTO _sqlx_migrations VALUES(54, 1);
            CREATE TABLE threads(id TEXT, content TEXT);
            INSERT INTO threads VALUES('private-id', 'private-conversation');",
        )
        .unwrap();
        drop(db);
        let before = std::fs::read(&path).unwrap();
        let value = schema(&path).unwrap();
        assert_eq!(value["migration"], 54);
        assert!(value.to_string().contains("CREATE TABLE threads"));
        assert!(!value.to_string().contains("private-"));
        assert_eq!(before, std::fs::read(path).unwrap());
    }
}
