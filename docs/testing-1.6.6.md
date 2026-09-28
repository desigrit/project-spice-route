# Spice Route 1.6.6 verification

Version 1.6.6 refines Overview on Windows and macOS. The device name leads the local pane, the latest visible handoff reports its own chat and project counts, and the delivery note sits below recent handoffs. The short handoff ID, local date, and selected size appear together. The counts come from the completed snapshot manifest, so they do not require scanning project folders when Overview opens.

The sample Windows Overview capture comes from the WinUI visual probe in the desktop build workflow. That probe uses in-memory data, keeps the window offscreen, and does not connect to a personal Codex profile or start a sync operation. The Mac interface can be rendered separately with the headless preview script described in [the screenshot guide](images/README.md).

The release gate includes frontend tests and build, Rust engine tests and strict Clippy, Windows x64 and ARM64 installer checks, installed startup and engine protocol checks, the sample WinUI visual probe, and an Apple Silicon DMG build. Checksums beside each package should match the downloaded files. Real-device Push and Pull across the selected cloud provider remain a separate acceptance check.
