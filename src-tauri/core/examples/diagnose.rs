// Read-only compatibility probe: no Engine construction, sync, or profile writes.
use spice_route_core::{codex, platform};
fn main() {
    let home = std::env::args()
        .nth(1)
        .expect("Pass the Codex data directory");
    let executable = platform::find_codex_executable();
    let version = platform::codex_version(executable.as_deref());
    let compatibility = codex::with_build_gate(
        codex::inspect(std::path::Path::new(&home)).expect("Read schema metadata"),
        version.as_deref(),
    );
    println!(
        "{}",
        serde_json::json!({"executable":executable,"version":version,"compatibility":compatibility})
    );
}
