fn main() {
    // The portable core includes these files from the parent crate with #[path].
    // Cargo does not always discover changes outside the package directory, so
    // declare them explicitly to prevent packaging a stale sync engine.
    for source in [
        "../src/codex.rs",
        "../src/compatibility.rs",
        "../src/diagnostics.rs",
        "../src/engine.rs",
        "../src/error.rs",
        "../src/models.rs",
        "../src/platform.rs",
        "../src/recovery.rs",
        "../src/settings.rs",
        "../src/snapshot.rs",
        "../src/util.rs",
    ] {
        println!("cargo:rerun-if-changed={source}");
    }
}
