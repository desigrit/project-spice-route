//! Local, versioned request protocol shared by native desktop frontends.
//! Standard output contains responses only. Diagnostics belong on stderr.
use crate::{
    engine::Engine,
    error::{Result, SpiceError},
    models::{AppConfig, ConflictResolution},
    platform, recovery, snapshot,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub id: String,
    pub method: String,
    #[serde(default = "empty_params")]
    pub params: Value,
}

fn empty_params() -> Value {
    json!({})
}

pub struct Server {
    engine: Arc<Engine>,
    mutation: Mutex<()>,
    shutting_down: Arc<AtomicBool>,
}

impl Server {
    pub fn new(engine: Arc<Engine>) -> Self {
        Self {
            engine,
            mutation: Mutex::new(()),
            shutting_down: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn handle(&self, request: Request) -> Value {
        let id = request.id.clone();
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.dispatch(&request)));
        match result {
            Ok(Ok(result)) => json!({"id": id, "result": result}),
            Ok(Err(error)) => json!({"id": id, "error": {"message": error.to_string()}}),
            Err(_) => {
                json!({"id": id, "error": {"message": "The engine stopped this request unexpectedly. Check Recovery before starting another handoff."}})
            }
        }
    }

    pub fn cancel_all(&self) {
        self.shutting_down.store(true, Ordering::SeqCst);
        self.engine.cancel_all();
    }

    fn dispatch(&self, request: &Request) -> Result<Value> {
        if self.shutting_down.load(Ordering::SeqCst) {
            return Err(SpiceError::Cancelled);
        }
        if request.id.is_empty() || request.id.len() > 128 || !request.params.is_object() {
            return Err("A request needs an ID and an object containing its parameters.".into());
        }
        // Progress and cancellation must stay available during a long operation.
        // A second mutation is rejected instead of being queued behind a restore.
        let mutating = matches!(
            request.method.as_str(),
            "save_config"
                | "preview_push"
                | "preview_pull"
                | "execute_push"
                | "execute_pull"
                | "restore_recovery"
                | "preview_cloud_cleanup"
                | "execute_cloud_cleanup"
                | "request_codex_close"
        );
        let _guard = if mutating {
            Some(self.mutation.try_lock().map_err(|_| {
                SpiceError::User(
                    "Another operation is in progress. Wait for it to finish or cancel it first."
                        .into(),
                )
            })?)
        } else {
            None
        };
        let params = &request.params;
        match request.method.as_str() {
            "get_protocol_info" => value(
                json!({"protocolVersion": PROTOCOL_VERSION, "engineVersion": env!("CARGO_PKG_VERSION")}),
            ),
            "load_config" => value(self.engine.load_config()?),
            "save_config" => value(self.engine.save_config(config(params)?)?),
            "discover_environment" => value(self.engine.discover_environment()?),
            "list_content" => value(self.engine.list_content(&config(params)?)?),
            "list_content_quick" => value(self.engine.list_content_quick(&config(params)?)?),
            "get_sync_status" => value(self.engine.sync_status(&config(params)?)?),
            "get_diagnostics_report" => value(self.engine.diagnostics_report(&config(params)?)),
            "list_snapshots" => {
                let config = config(params)?;
                let mut manifests = snapshot::list_manifests(&config)?;
                manifests.sort_by(|a, b| b.created_at.cmp(&a.created_at));
                value(
                    manifests
                        .iter()
                        .map(|manifest| {
                            manifest.summary(
                                manifest
                                    .objects
                                    .iter()
                                    .map(|object| object.stored_size)
                                    .sum(),
                                false,
                            )
                        })
                        .collect::<Vec<_>>(),
                )
            }
            "preview_push" => value(self.engine.preview_push(&config(params)?)?),
            "preview_pull" => value(self.engine.preview_pull(
                &config(params)?,
                params.get("snapshotId").and_then(Value::as_str),
            )?),
            "execute_push" => value(
                self.engine
                    .execute_push(&config(params)?, string(params, "operationId")?)?,
            ),
            "execute_pull" => {
                let resolutions: Vec<ConflictResolution> = serde_json::from_value(
                    params
                        .get("resolutions")
                        .cloned()
                        .unwrap_or_else(|| json!([])),
                )?;
                value(self.engine.execute_pull(
                    &config(params)?,
                    string(params, "operationId")?,
                    &resolutions,
                )?)
            }
            "cancel_operation" => {
                self.engine.cancel(string(params, "operationId")?)?;
                Ok(Value::Null)
            }
            "get_operation_progress" => value(
                self.engine
                    .operation_progress(string(params, "operationId")?)?,
            ),
            "list_recoveries" => value(recovery::list(&self.engine.data_dir)?),
            "restore_recovery" => {
                recovery::restore_cancellable(
                    &self.engine.data_dir,
                    string(params, "recoveryId")?,
                    self.shutting_down.clone(),
                )?;
                Ok(Value::Null)
            }
            "request_codex_close" => value(platform::request_codex_close()?),
            "preview_cloud_cleanup" => value(self.engine.preview_cloud_cleanup(&config(params)?)?),
            "execute_cloud_cleanup" => value(self.engine.execute_cloud_cleanup(
                &config(params)?,
                string(params, "operationId")?,
                string(params, "confirmation")?,
            )?),
            _ => Err(SpiceError::User(format!(
                "Unsupported engine operation: {}",
                request.method
            ))),
        }
    }
}

fn config(params: &Value) -> Result<AppConfig> {
    serde_json::from_value(params.get("config").cloned().ok_or_else(|| {
        SpiceError::User("This operation needs the current configuration.".into())
    })?)
    .map_err(Into::into)
}
fn string<'a>(params: &'a Value, key: &str) -> Result<&'a str> {
    params
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| SpiceError::User(format!("Missing request parameter: {key}")))
}
fn value(input: impl Serialize) -> Result<Value> {
    serde_json::to_value(input).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn request_contract_preserves_ids_results_and_actionable_errors() {
        let temp = tempdir().unwrap();
        let server = Server::new(Arc::new(Engine::new(temp.path().join("app")).unwrap()));
        let ask = |id: &str, method: &str, params: Value| {
            server.handle(Request {
                id: id.into(),
                method: method.into(),
                params,
            })
        };
        let hello = ask("hello-1", "get_protocol_info", json!({}));
        assert_eq!(hello["id"], "hello-1");
        assert_eq!(hello["result"]["protocolVersion"], 1);
        let missing = ask("bad-1", "execute_push", json!({}));
        assert_eq!(missing["id"], "bad-1");
        assert!(missing["error"]["message"]
            .as_str()
            .unwrap()
            .contains("configuration"));
        let unknown = ask("bad-2", "open_codex", json!({}));
        assert!(unknown["error"].is_object());
        assert!(!temp.path().join("app/config.json").exists());
    }

    #[test]
    fn progress_stays_available_while_mutations_are_busy() {
        let temp = tempdir().unwrap();
        let server = Server::new(Arc::new(Engine::new(temp.path().join("app")).unwrap()));
        let _busy = server.mutation.lock().unwrap();
        let rejected = server.handle(Request {
            id: "save".into(),
            method: "save_config".into(),
            params: json!({}),
        });
        assert!(rejected["error"]["message"]
            .as_str()
            .unwrap()
            .contains("in progress"));
        let progress = server.handle(Request {
            id: "progress".into(),
            method: "get_operation_progress".into(),
            params: json!({"operationId":"missing"}),
        });
        assert_eq!(progress["id"], "progress");
        assert!(progress.get("result").is_some());
        assert_eq!(progress["result"], Value::Null);
    }
}
