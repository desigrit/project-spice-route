use spice_route_core::engine::Engine;
use spice_route_core::error::Result;
use spice_route_core::models::{
    AppConfig, CloudCleanupPreview, CloudCleanupResult, ConflictResolution, ContentCatalog,
    EnvironmentDiscovery, OperationPreview, OperationProgress, OperationResult, RecoverySummary,
    SyncStatus,
};
use spice_route_core::{platform, recovery};
use std::sync::Arc;
use tauri::{Manager, State};

async fn run_blocking<T, F>(operation: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|error| {
            spice_route_core::error::SpiceError::User(format!(
                "Background operation failed: {error}"
            ))
        })?
}

#[tauri::command]
async fn discover_environment(engine: State<'_, Arc<Engine>>) -> Result<EnvironmentDiscovery> {
    let engine = Arc::clone(engine.inner());
    run_blocking(move || engine.discover_environment()).await
}

#[tauri::command]
fn load_config(engine: State<'_, Arc<Engine>>) -> Result<AppConfig> {
    engine.load_config()
}

#[tauri::command]
async fn save_config(engine: State<'_, Arc<Engine>>, config: AppConfig) -> Result<AppConfig> {
    let engine = Arc::clone(engine.inner());
    run_blocking(move || engine.save_config(config)).await
}

#[tauri::command]
async fn list_content(engine: State<'_, Arc<Engine>>, config: AppConfig) -> Result<ContentCatalog> {
    let engine = Arc::clone(engine.inner());
    run_blocking(move || engine.list_content(&config)).await
}

#[tauri::command]
async fn list_content_quick(engine: State<'_, Arc<Engine>>, config: AppConfig) -> Result<ContentCatalog> {
    let engine = Arc::clone(engine.inner());
    run_blocking(move || engine.list_content_quick(&config)).await
}

#[tauri::command]
async fn get_sync_status(engine: State<'_, Arc<Engine>>, config: AppConfig) -> Result<SyncStatus> {
    let engine = Arc::clone(engine.inner());
    run_blocking(move || engine.sync_status(&config)).await
}

#[tauri::command]
async fn preview_push(
    engine: State<'_, Arc<Engine>>,
    config: AppConfig,
) -> Result<OperationPreview> {
    let engine = Arc::clone(engine.inner());
    run_blocking(move || engine.preview_push(&config)).await
}

#[tauri::command]
async fn preview_pull(
    engine: State<'_, Arc<Engine>>,
    config: AppConfig,
    snapshot_id: Option<String>,
) -> Result<OperationPreview> {
    let engine = Arc::clone(engine.inner());
    run_blocking(move || engine.preview_pull(&config, snapshot_id.as_deref())).await
}

#[tauri::command]
async fn execute_push(
    engine: State<'_, Arc<Engine>>,
    config: AppConfig,
    operation_id: String,
) -> Result<OperationResult> {
    let engine = Arc::clone(engine.inner());
    run_blocking(move || engine.execute_push(&config, &operation_id)).await
}

#[tauri::command]
async fn execute_pull(
    engine: State<'_, Arc<Engine>>,
    config: AppConfig,
    operation_id: String,
    resolutions: Vec<ConflictResolution>,
) -> Result<OperationResult> {
    let engine = Arc::clone(engine.inner());
    run_blocking(move || engine.execute_pull(&config, &operation_id, &resolutions)).await
}

#[tauri::command]
fn cancel_operation(engine: State<'_, Arc<Engine>>, operation_id: String) -> Result<()> {
    engine.cancel(&operation_id)
}

#[tauri::command]
fn get_operation_progress(
    engine: State<'_, Arc<Engine>>,
    operation_id: String,
) -> Result<Option<OperationProgress>> {
    engine.operation_progress(&operation_id)
}

#[tauri::command]
async fn preview_cloud_cleanup(
    engine: State<'_, Arc<Engine>>,
    config: AppConfig,
) -> Result<CloudCleanupPreview> {
    let engine = Arc::clone(engine.inner());
    run_blocking(move || engine.preview_cloud_cleanup(&config)).await
}

#[tauri::command]
async fn execute_cloud_cleanup(
    engine: State<'_, Arc<Engine>>,
    config: AppConfig,
    operation_id: String,
    confirmation: String,
) -> Result<CloudCleanupResult> {
    let engine = Arc::clone(engine.inner());
    run_blocking(move || engine.execute_cloud_cleanup(&config, &operation_id, &confirmation)).await
}

#[tauri::command]
async fn request_codex_close() -> Result<bool> {
    run_blocking(platform::request_codex_close).await
}

#[tauri::command]
fn open_codex() -> Result<()> {
    platform::open_codex()
}

#[tauri::command]
async fn list_recoveries(engine: State<'_, Arc<Engine>>) -> Result<Vec<RecoverySummary>> {
    let engine = Arc::clone(engine.inner());
    run_blocking(move || recovery::list(&engine.data_dir)).await
}

#[tauri::command]
async fn restore_recovery(engine: State<'_, Arc<Engine>>, recovery_id: String) -> Result<()> {
    let engine = Arc::clone(engine.inner());
    run_blocking(move || recovery::restore(&engine.data_dir, &recovery_id)).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().app_local_data_dir()?;
            let engine = Engine::new(data_dir)
                .map_err(|error| Box::<dyn std::error::Error>::from(error.to_string()))?;
            app.manage(Arc::new(engine));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            discover_environment,
            load_config,
            save_config,
            list_content,
            list_content_quick,
            get_sync_status,
            preview_push,
            preview_pull,
            execute_push,
            execute_pull,
            cancel_operation,
            get_operation_progress,
            preview_cloud_cleanup,
            execute_cloud_cleanup,
            request_codex_close,
            open_codex,
            list_recoveries,
            restore_recovery,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Spice Route");
}
