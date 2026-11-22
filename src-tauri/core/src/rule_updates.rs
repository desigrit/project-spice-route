//! Reviewed device-local table definitions, distributed separately from desktop releases.
use crate::error::Result;
use crate::{updates, util};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::Deserialize;
use std::collections::HashSet;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

const FEED: &str = "https://github.com/desigrit/project-spice-route/releases/download/compat-rules-v1/compatibility-rules.json";
const MAX_BYTES: usize = 128 * 1024;
const CACHE: &str = "compatibility-rules.json";
const BUNDLED: &[u8] = include_bytes!("../../../compatibility/local-tables.json");
static CACHED_REVISION: AtomicU64 = AtomicU64::new(0);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    payload: String,
    signature: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalTable {
    database: String,
    name: String,
    create_sha256: String,
    pub delete_by_thread: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rules {
    kind: String,
    format: u32,
    revision: u64,
    tables: Vec<LocalTable>,
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value.as_bytes()[0].is_ascii_lowercase()
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

impl Rules {
    fn parse(bytes: &[u8]) -> Result<Self> {
        let rules: Self = serde_json::from_slice(bytes)?;
        if rules.kind != "spice-route-local-tables"
            || rules.format != 1
            || rules.revision == 0
            || rules.tables.len() > 128
        {
            return Err("Unsupported compatibility rule set.".into());
        }
        let mut definitions = HashSet::new();
        for table in &rules.tables {
            if table.database != "state_5.sqlite"
                || !identifier(&table.name)
                || table.create_sha256.len() != 64
                || !table
                    .create_sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                || table
                    .delete_by_thread
                    .as_deref()
                    .is_some_and(|column| !identifier(column))
                || !definitions.insert((&table.name, &table.create_sha256))
            {
                return Err("Invalid local table rule.".into());
            }
        }
        Ok(rules)
    }

    fn verified(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_BYTES {
            return Err("The compatibility download is too large.".into());
        }
        let envelope: Envelope = serde_json::from_slice(bytes)?;
        let payload = STANDARD
            .decode(envelope.payload)
            .map_err(|_| "Invalid compatibility payload.")?;
        updates::verify_signed_bytes(&payload, &envelope.signature)?;
        Self::parse(&payload)
    }

    fn lookup(&self, database: &str, name: &str, sql: &str) -> Option<LocalTable> {
        let digest = util::sha256_bytes(sql.as_bytes());
        self.tables
            .iter()
            .find(|table| {
                table.database == database && table.name == name && table.create_sha256 == digest
            })
            .cloned()
    }
}

fn active() -> &'static RwLock<Rules> {
    static ACTIVE: OnceLock<RwLock<Rules>> = OnceLock::new();
    ACTIVE.get_or_init(|| RwLock::new(Rules::parse(BUNDLED).expect("bundled compatibility rules")))
}

pub fn local_table(database: &str, name: &str, normalized_create_sql: &str) -> Option<LocalTable> {
    active()
        .read()
        .ok()?
        .lookup(database, name, normalized_create_sql)
}

/// Also used by the publication workflow, before the signed asset goes live.
pub fn verified_revision(envelope: &[u8]) -> Result<u64> {
    Ok(Rules::verified(envelope)?.revision)
}

fn installed_profile(data_dir: &Path) -> bool {
    !cfg!(test)
        && dirs::data_local_dir()
            .is_some_and(|root| data_dir == root.join("com.spiceroute.codexsync"))
}

fn read_limited(reader: impl Read) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_BYTES {
        return Err("The compatibility download is too large.".into());
    }
    Ok(bytes)
}

pub fn initialize(data_dir: &Path) {
    if !installed_profile(data_dir) {
        return;
    }
    if let Ok(cached) = File::open(data_dir.join(CACHE))
        .map_err(Into::into)
        .and_then(read_limited)
        .and_then(|bytes| Rules::verified(&bytes))
    {
        if let Ok(mut rules) = active().write() {
            if cached.revision > rules.revision {
                CACHED_REVISION.store(cached.revision, Ordering::Relaxed);
                *rules = cached;
            }
        }
    }
    refresh_if_due(data_dir);
}

pub fn refresh_if_due(data_dir: &Path) {
    if !installed_profile(data_dir) {
        return;
    }
    static LAST_ATTEMPT: Mutex<Option<Instant>> = Mutex::new(None);
    let Ok(mut last) = LAST_ATTEMPT.lock() else {
        return;
    };
    if last.is_some_and(|time| time.elapsed() < Duration::from_secs(60)) {
        return;
    }
    *last = Some(Instant::now());
    let directory = data_dir.to_owned();
    let _ = std::thread::Builder::new()
        .name("compatibility-rules".into())
        .spawn(move || {
            let _ = refresh(&directory);
        });
}

fn system_curl() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        Some(
            PathBuf::from(std::env::var_os("SystemRoot")?)
                .join("System32")
                .join("curl.exe"),
        )
    }
    #[cfg(not(windows))]
    {
        Some(PathBuf::from("/usr/bin/curl"))
    }
}

fn refresh(directory: &Path) -> Result<()> {
    let executable = system_curl().ok_or("The system HTTPS client is unavailable.")?;
    let mut child = crate::platform::hidden_command(executable)
        .args([
            "--disable",
            "--silent",
            "--fail",
            "--location",
            "--max-redirs",
            "4",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-time",
            "8",
            "--max-filesize",
            &MAX_BYTES.to_string(),
            FEED,
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()?;
    let bytes = read_limited(child.stdout.take().expect("piped HTTPS response"));
    if bytes.is_err() {
        let _ = child.kill();
    }
    let status = child.wait()?;
    if !status.success() {
        return Err("The compatibility feed is unavailable.".into());
    }
    install(directory, bytes?)
}

fn install(directory: &Path, bytes: Vec<u8>) -> Result<()> {
    let incoming = Rules::verified(&bytes)?;
    let mut rules = active()
        .write()
        .map_err(|_| "Compatibility cache unavailable.")?;
    if incoming.revision <= rules.revision.max(CACHED_REVISION.load(Ordering::Relaxed)) {
        return Ok(());
    }
    // A rule may be used by an in-flight handoff. Only append classifications
    // during a running process. Replacements and withdrawals apply at startup.
    let additive = rules
        .tables
        .iter()
        .all(|table| incoming.tables.contains(table));
    let mut file = tempfile::NamedTempFile::new_in(directory)?;
    file.write_all(&bytes)?;
    file.as_file().sync_all()?;
    file.persist(directory.join(CACHE))
        .map_err(|failure| failure.error)?;
    CACHED_REVISION.store(incoming.revision, Ordering::Relaxed);
    if additive {
        *rules = incoming;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bundled_table_requires_exact_name_database_and_definition() {
        let rules = Rules::parse(BUNDLED).unwrap();
        let fixture_directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/fixtures");
        let mut definitions = Vec::new();
        for entry in std::fs::read_dir(fixture_directory).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let fixture: serde_json::Value =
                serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
            for object in fixture["state_5.sqlite"]["objects"]
                .as_array()
                .into_iter()
                .flatten()
            {
                if object["type"] == "table" {
                    definitions.push((
                        object["name"].as_str().unwrap().to_owned(),
                        object["sql"].as_str().unwrap().replace("\r\n", "\n"),
                    ));
                }
            }
        }
        for table in &rules.tables {
            let sql = definitions
                .iter()
                .find_map(|(name, sql)| {
                    (name == &table.name
                        && util::sha256_bytes(sql.as_bytes()) == table.create_sha256)
                        .then_some(sql)
                })
                .expect("each compatibility rule must match a reviewed schema fixture");
            assert!(sql.starts_with("CREATE TABLE "));
            let probe = rusqlite::Connection::open_in_memory().unwrap();
            probe.execute_batch(sql).unwrap();
            if let Some(column) = &table.delete_by_thread {
                let count: i64 = probe.query_row(
                    "SELECT count(*) FROM pragma_table_xinfo(?1) WHERE name=?2 AND upper(type)='TEXT' AND hidden=0",
                    [&table.name, column], |row| row.get(0)
                ).unwrap();
                assert_eq!(count, 1, "the declared thread column must exist");
            }
            assert_eq!(
                rules.lookup("state_5.sqlite", &table.name, sql).unwrap(),
                *table
            );
            assert!(rules
                .lookup("thread_history_1.sqlite", &table.name, sql)
                .is_none());
            assert!(rules
                .lookup("state_5.sqlite", &table.name, &format!("{sql}changed"))
                .is_none());
        }
    }

    #[test]
    fn authentic_signature_is_bound_to_the_payload_and_feed_kind() {
        let signature = include_str!("../tests/fixtures/update-signature.txt.sig");
        let signed = include_bytes!("../tests/fixtures/update-signature.txt");
        updates::verify_signed_bytes(signed, signature).unwrap();
        for bytes in [signed.as_slice(), BUNDLED] {
            let envelope =
                serde_json::json!({"payload": STANDARD.encode(bytes), "signature": signature});
            assert!(Rules::verified(&serde_json::to_vec(&envelope).unwrap()).is_err());
        }
        assert!(read_limited(&vec![0; MAX_BYTES + 1][..]).is_err());
    }
}
