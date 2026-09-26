//! Explicit storage profiles. Fixtures are schema metadata, never personal databases.
use crate::models::CompatibilityInfo;
use serde_json::Value;

pub struct Profile {
    pub state: i64,
    pub history: i64,
    pub runtime: &'static str,
    pub fingerprint: &'static str,
    pub legacy_fingerprints: &'static [&'static str],
    pub schema: &'static str,
    pub history_schema: Option<&'static str>,
}

impl Profile {
    pub fn matches(&self, info: &CompatibilityInfo) -> bool {
        info.state_migration == Some(self.state)
            && info.history_migration == Some(self.history)
            && (info.schema_fingerprint == self.fingerprint
                || self
                    .legacy_fingerprints
                    .contains(&info.schema_fingerprint.as_str()))
    }

    pub fn schema_for(&self, database: &str) -> &'static str {
        if database == "thread_history_1.sqlite" {
            self.history_schema.unwrap_or(self.schema)
        } else {
            self.schema
        }
    }
}

pub const PROFILES: &[Profile] = &[
    Profile {
        state: 52,
        history: 6,
        runtime: "0.153.4",
        fingerprint: "c2144b3e63ffbc0caa5b338f58f43d2f3293d5b080d5176c54bba03bb8ef5625",
        legacy_fingerprints: &["8f654166cba02b510074adbd0e4ebc66128b624672b74d9a6ca415b8e09eab16"],
        schema: include_str!("fixtures/schema-52.json"),
        history_schema: None,
    },
    Profile {
        state: 54,
        history: 6,
        runtime: "0.154.0-alpha.6.2",
        fingerprint: "8082e27f46c7a5691ae4dca5b004ce2103bacaafb3989f7be95607d34685c205",
        legacy_fingerprints: &["9422fcd06e5ff2ed83657ad39ff5247b27150bdd85d38f6cc4159641a52c5434"],
        schema: include_str!("fixtures/schema-54.json"),
        history_schema: None,
    },
    Profile {
        state: 55,
        history: 6,
        runtime: "0.155.0-alpha.9.2",
        fingerprint: "52ae661eafd5735306aafc892143c840a9b9662046d0d91a5f91083646ed3b36",
        legacy_fingerprints: &["c5d97837b0ea23df607c69a6721ba2612a6169733dee2b1d795a96af759208de"],
        schema: include_str!("fixtures/schema-55.json"),
        history_schema: None,
    },
    Profile {
        state: 57,
        history: 7,
        runtime: "",
        fingerprint: "48182dfc86c349d16271b236fba65b8cbe627b324955380d0497d0ceefccbcb1",
        legacy_fingerprints: &[],
        schema: include_str!("fixtures/schema-57.json"),
        history_schema: None,
    },
    // Codex can migrate its state and history databases independently. Both
    // halves below are exact tested layouts; only their pairing is new.
    Profile {
        state: 57,
        history: 6,
        runtime: "",
        fingerprint: "dac383732eddc28f1d2faca9a5b107ac23877db8dff8a6903bfd79b918a386c4",
        legacy_fingerprints: &[],
        schema: include_str!("fixtures/schema-57.json"),
        history_schema: Some(include_str!("fixtures/schema-55.json")),
    },
];

pub fn profile(info: &CompatibilityInfo) -> Option<&'static Profile> {
    PROFILES.iter().find(|profile| profile.matches(info))
}

pub(crate) fn normalize_schema_sql(sql: &str) -> String {
    sql.replace("\r\n", "\n").replace('\r', "\n")
}

pub fn expected_layout(profile: &Profile, database: &str) -> String {
    let fixture: Value =
        serde_json::from_str(profile.schema_for(database)).expect("embedded schema fixture");
    fixture[database]["objects"]
        .as_array()
        .expect("schema objects")
        .iter()
        .map(|obj| {
            format!(
                "{}:{}:{}",
                obj["type"].as_str().unwrap(),
                obj["name"].as_str().unwrap(),
                normalize_schema_sql(obj["sql"].as_str().unwrap())
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn published_platform_sensitive_fingerprints_remain_recognized() {
        for expected in PROFILES {
            for fingerprint in expected.legacy_fingerprints {
                let info = CompatibilityInfo {
                    supported: true,
                    adapter: String::new(),
                    state_migration: Some(expected.state),
                    history_migration: Some(expected.history),
                    schema_fingerprint: (*fingerprint).into(),
                    explanation: String::new(),
                };
                assert_eq!(
                    profile(&info).map(|value| value.state),
                    Some(expected.state)
                );
            }
        }
    }
}
