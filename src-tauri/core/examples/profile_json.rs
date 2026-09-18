// Synthetic I/O benchmark. No Codex profile, app process, or cloud data is used.
use serde_json::Value;
use std::fs::File;
use std::io::BufReader;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let temporary = tempfile::tempdir()?;
    let path = temporary.path().join("manifest.json");
    let payload = serde_json::json!({"records": (0..25_000)
        .map(|id| serde_json::json!({"id":id,"text":"sample content ".repeat(12)}))
        .collect::<Vec<_>>()});
    std::fs::write(&path, serde_json::to_vec(&payload)?)?;
    let started = Instant::now();
    let before: Value = serde_json::from_reader(File::open(&path)?)?;
    let unbuffered = started.elapsed();
    let started = Instant::now();
    let after: Value =
        serde_json::from_reader(BufReader::with_capacity(256 * 1024, File::open(&path)?))?;
    let buffered = started.elapsed();
    assert_eq!(before, after);
    println!(
        "{} bytes: unbuffered {:?}, buffered {:?}, {:.1}x faster",
        std::fs::metadata(&path)?.len(),
        unbuffered,
        buffered,
        unbuffered.as_secs_f64() / buffered.as_secs_f64()
    );
    Ok(())
}
