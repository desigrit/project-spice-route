use super::{normalize_schema_sql, Profile, PROFILES};
use crate::models::CompatibilityInfo;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const STATE: &str = "state_5.sqlite";
pub(crate) const HISTORY: &str = "thread_history_1.sqlite";
const MAX_CHANGES: usize = 64;
const MAX_DEFINITION: usize = 16 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SchemaObject {
    #[serde(rename = "type")]
    kind: String,
    name: String,
    sql: String,
}

/// Only differences from a known pair travel with the snapshot. The receiver
/// reconstructs the layout locally and validates each difference, without
/// executing any SQL supplied in a handoff.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SchemaExtensions {
    base_state: i64,
    base_history: i64,
    pub(crate) state: Vec<SchemaObject>,
    pub(crate) history: Vec<SchemaObject>,
}

type Layout = BTreeMap<(String, String), String>;

fn read_layout(db: &Connection) -> rusqlite::Result<Layout> {
    let mut query = db.prepare("SELECT type,name,coalesce(sql,'') FROM sqlite_schema WHERE substr(name,1,7) <> 'sqlite_' ORDER BY type,name")?;
    let rows = query.query_map([], |row| {
        let sql: String = row.get(2)?;
        Ok(((row.get(0)?, row.get(1)?), normalize_schema_sql(&sql)))
    })?;
    rows.collect()
}

fn base_layout(profile: &Profile, database: &str) -> Layout {
    let fixture: serde_json::Value =
        serde_json::from_str(profile.schema_for(database)).expect("embedded schema fixture");
    let objects: Vec<SchemaObject> = serde_json::from_value(fixture[database]["objects"].clone())
        .expect("embedded schema objects");
    objects
        .into_iter()
        .map(|object| {
            (
                (object.kind, object.name),
                normalize_schema_sql(&object.sql),
            )
        })
        .collect()
}

fn base_profile(info: &CompatibilityInfo) -> Option<&'static Profile> {
    // Never reinterpret an unknown older format. Newer migrations must retain
    // the latest known shape of the corresponding database.
    let state = info
        .state_migration?
        .min(PROFILES.iter().map(|p| p.state).max()?);
    let history = info
        .history_migration?
        .min(PROFILES.iter().map(|p| p.history).max()?);
    PROFILES
        .iter()
        .find(|p| p.state == state && p.history == history)
}

fn changes(base: &Layout, actual: &Layout) -> Option<Vec<SchemaObject>> {
    if base.keys().any(|key| !actual.contains_key(key)) {
        return None;
    }
    Some(
        actual
            .iter()
            .filter(|(k, v)| base.get(*k) != Some(*v))
            .map(|((kind, name), sql)| SchemaObject {
                kind: kind.clone(),
                name: name.clone(),
                sql: sql.clone(),
            })
            .collect(),
    )
}

pub(crate) fn detect(
    state: &Connection,
    history: &Connection,
    info: &CompatibilityInfo,
) -> rusqlite::Result<Option<SchemaExtensions>> {
    let Some(base) = base_profile(info) else {
        return Ok(None);
    };
    let Some(state) = changes(&base_layout(base, STATE), &read_layout(state)?) else {
        return Ok(None);
    };
    let Some(history) = changes(&base_layout(base, HISTORY), &read_layout(history)?) else {
        return Ok(None);
    };
    let extensions = SchemaExtensions {
        base_state: base.state,
        base_history: base.history,
        state,
        history,
    };
    Ok(extensions.validate(info).map(|_| extensions))
}

fn identifier(name: &str) -> bool {
    let mut bytes = name.bytes();
    (1..=128).contains(&name.len())
        && bytes
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
        && bytes.all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

fn extra_columns(original: &str, actual: &str) -> Option<Vec<String>> {
    // SQLite inserts new columns before a trailing table-level primary key.
    // Match the original prefix and suffix verbatim, including that constraint.
    let insertion = original
        .rfind(",\n    PRIMARY KEY")
        .or_else(|| original.strip_suffix(')').map(str::len))?;
    let (prefix, suffix) = original.split_at(insertion);
    let tail = actual
        .strip_prefix(prefix)?
        .strip_suffix(suffix)?
        .trim_start()
        .strip_prefix(',')?;
    let mut columns = Vec::new();
    for definition in tail.split(',') {
        let mut words = definition.split_ascii_whitespace();
        let name = words.next()?;
        let declared = words.next()?.to_ascii_uppercase();
        if !identifier(name)
            || !matches!(
                declared.as_str(),
                "TEXT" | "INTEGER" | "REAL" | "BLOB" | "NUMERIC"
            )
            || words.next().is_some()
            || columns.len() >= MAX_CHANGES
            || columns
                .iter()
                .any(|old: &String| old.eq_ignore_ascii_case(name))
        {
            return None;
        }
        columns.push(name.to_owned());
    }
    Some(columns)
}

fn plain_index(object: &SchemaObject, base: &Layout) -> bool {
    // Accept only the small grammar CREATE INDEX name ON table(col [ASC|DESC],...).
    // This excludes unique, partial, expression and custom-collation indexes.
    let Some((header, columns)) = object.sql.split_once('(') else {
        return false;
    };
    let Some(columns) = columns.trim_end().strip_suffix(')') else {
        return false;
    };
    let words: Vec<_> = header.split_ascii_whitespace().collect();
    let [create, index, name, on, table] = words.as_slice() else {
        return false;
    };
    create.eq_ignore_ascii_case("create")
        && index.eq_ignore_ascii_case("index")
        && *name == object.name
        && on.eq_ignore_ascii_case("on")
        && base.contains_key(&("table".into(), (*table).into()))
        && columns.split(',').all(|column| {
            let mut parts = column.split_ascii_whitespace();
            parts.next().is_some_and(identifier)
                && parts.next().map_or(true, |s| {
                    s.eq_ignore_ascii_case("asc") || s.eq_ignore_ascii_case("desc")
                })
                && parts.next().is_none()
        })
}

fn apply_extensions(base: &mut Layout, database: &str, additions: &[SchemaObject]) -> Option<()> {
    let mut seen = BTreeSet::new();
    // sqlite_schema sorts indexes before tables. Validate reviewed tables first so
    // an ordinary index on a newly accepted table is independent of object order.
    let ordered = additions
        .iter()
        .filter(|obj| obj.kind == "table")
        .chain(additions.iter().filter(|obj| obj.kind != "table"));
    for object in ordered {
        if !identifier(&object.name)
            || object.name.to_ascii_lowercase().starts_with("sqlite_")
            || object.sql.len() > MAX_DEFINITION
            || !seen.insert(object.name.to_ascii_lowercase())
        {
            return None;
        }
        let key = (object.kind.clone(), object.name.clone());
        let allowed = match (object.kind.as_str(), base.get(&key)) {
            ("table", Some(original))
                if (database == STATE && object.name == "threads")
                    || (database == HISTORY
                        && matches!(object.name.as_str(), "thread_items" | "thread_turns")) =>
            {
                extra_columns(original, &object.sql).is_some()
            }
            ("index", None) => plain_index(object, base),
            ("table", None) => {
                crate::rule_updates::local_table(database, &object.name, &object.sql).is_some()
            }
            _ => false,
        };
        if !allowed {
            return None;
        }
        base.insert(key, object.sql.clone());
    }
    Some(())
}

fn signature(layout: &Layout) -> String {
    layout
        .iter()
        .filter(|((kind, _), _)| matches!(kind.as_str(), "table" | "index"))
        .map(|((_, name), sql)| format!("{name}:{sql}"))
        .collect::<Vec<_>>()
        .join("\n")
}

impl SchemaExtensions {
    // Store the baseline in the handoff: a later reader may know a newer layout.
    fn recorded_base(&self) -> Option<&'static Profile> {
        PROFILES
            .iter()
            .find(|p| p.state == self.base_state && p.history == self.base_history)
    }
    pub(crate) fn validate(&self, info: &CompatibilityInfo) -> Option<&'static Profile> {
        let base = self.recorded_base()?;
        let state_version = info.state_migration?;
        let history_version = info.history_migration?;
        if state_version < base.state
            || history_version < base.history
            || (state_version != base.state && base.state < 64)
            || (history_version != base.history && base.history < 9)
        {
            return None;
        }
        if self.state.len().checked_add(self.history.len())? > MAX_CHANGES
            || self.state.is_empty() && self.history.is_empty()
            || self.state.is_empty() && info.state_migration? > base.state
            || self.history.is_empty() && info.history_migration? > base.history
        {
            return None;
        }
        let mut state = base_layout(base, STATE);
        let mut history = base_layout(base, HISTORY);
        apply_extensions(&mut state, STATE, &self.state)?;
        apply_extensions(&mut history, HISTORY, &self.history)?;
        let fingerprint = crate::util::sha256_bytes(
            format!("{}\n{}", signature(&state), signature(&history)).as_bytes(),
        );
        (fingerprint == info.schema_fingerprint).then_some(base)
    }

    pub(crate) fn columns(&self) -> BTreeMap<String, Vec<String>> {
        let mut columns = BTreeMap::new();
        let Some(base) = self.recorded_base() else {
            return columns;
        };
        for (database, entries) in [(STATE, &self.state), (HISTORY, &self.history)] {
            let layout = base_layout(base, database);
            for entry in entries {
                if entry.kind != "table" {
                    continue;
                }
                if let Some(names) = layout
                    .get(&(entry.kind.clone(), entry.name.clone()))
                    .and_then(|original| extra_columns(original, &entry.sql))
                {
                    columns.insert(entry.name.clone(), names);
                }
            }
        }
        columns
    }

    pub(crate) fn local_deletions(&self) -> crate::error::Result<Vec<(&str, String)>> {
        let mut deletions = Vec::new();
        for table in self
            .state
            .iter()
            .filter(|obj| obj.kind == "table" && obj.name != "threads")
        {
            let Some(rule) = crate::rule_updates::local_table(STATE, &table.name, &table.sql)
            else {
                return Err(crate::error::SpiceError::UnsupportedCodex(format!(
                    "The rule for {} changed. Review the Pull again.",
                    table.name
                )));
            };
            if let Some(column) = rule.delete_by_thread {
                deletions.push((table.name.as_str(), column));
            }
        }
        Ok(deletions)
    }
}
