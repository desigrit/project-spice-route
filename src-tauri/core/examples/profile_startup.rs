// Read-only discovery benchmark. The supplied config is read without saving it.
// Runtime files are confined to a disposable directory. No transfer or UI runs.
use spice_route_core::{engine::Engine, models::AppConfig, util::read_json};
use std::{path::PathBuf, time::Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("Pass a config.json path to profile discovery")?;
    let config: AppConfig = read_json(&PathBuf::from(path))?;
    let temporary = tempfile::tempdir()?;
    let engine = Engine::new(temporary.path().join("app"))?;
    let started = Instant::now();
    let catalog = engine.list_content_quick(&config)?;
    println!(
        "Quick catalog: {:?}; {} chats, {} projects",
        started.elapsed(),
        catalog.threads.len(),
        catalog.projects.len()
    );
    let started = Instant::now();
    let status = engine.sync_status(&config)?;
    println!(
        "Visible cloud status: {:?}; {} visible heads, {} stored bytes; no content verification",
        started.elapsed(),
        status.visible_heads.len(),
        status.cloud_bytes
    );
    Ok(())
}
