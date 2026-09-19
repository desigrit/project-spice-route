//! Explicit storage profiles. Fixtures are schema metadata, never personal databases.
use crate::models::CompatibilityInfo;
use serde_json::Value;

pub struct Profile {
    pub state: i64,
    pub runtime: &'static str,
    pub fingerprint: &'static str,
    pub schema: &'static str,
}

pub const PROFILES: &[Profile] = &[
    Profile {
        state: 52,
        runtime: "0.153.4",
        fingerprint: "8f654166cba02b510074adbd0e4ebc66128b624672b74d9a6ca415b8e09eab16",
        schema: include_str!("fixtures/schema-52.json"),
    },
    Profile {
        state: 54,
        runtime: "0.154.0-alpha.6.2",
        fingerprint: "9422fcd06e5ff2ed83657ad39ff5247b27150bdd85d38f6cc4159641a52c5434",
        schema: include_str!("fixtures/schema-54.json"),
    },
    Profile {
        state: 55,
        runtime: "0.155.0-alpha.9.2",
        fingerprint: "c5d97837b0ea23df607c69a6721ba2612a6169733dee2b1d795a96af759208de",
        schema: include_str!("fixtures/schema-55.json"),
    },
];

pub fn profile(info: &CompatibilityInfo) -> Option<&'static Profile> {
    PROFILES.iter().find(|p| {
        info.state_migration == Some(p.state)
            && info.history_migration == Some(6)
            && info.schema_fingerprint == p.fingerprint
    })
}

pub fn expected_layout(profile: &Profile, database: &str) -> String {
    let fixture: Value = serde_json::from_str(profile.schema).expect("embedded schema fixture");
    fixture[database]["objects"]
        .as_array()
        .expect("schema objects")
        .iter()
        .map(|obj| {
            format!(
                "{}:{}:{}",
                obj["type"].as_str().unwrap(),
                obj["name"].as_str().unwrap(),
                obj["sql"].as_str().unwrap().replace("\r\n", "\n")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
