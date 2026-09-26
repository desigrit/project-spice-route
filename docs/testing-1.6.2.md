# Spice Route 1.6.2 verification

This release adds the Codex state/history 57/7 storage profile observed with Codex desktop 26.924.2738 on Windows. The schema fixture contains table, index, and trigger definitions only. It contains no personal records or paths.

The adapter now carries `threads.creator_user_id`, `threads.creator_account_id`, `thread_items.started_at_ms`, and `thread_items.completed_at_ms` through selected snapshots. It treats null extension fields as absent when comparing unchanged chats across supported profiles. A destination whose database lacks a field rejects a non-null value before any staged database mutation.

Local verification completed:

- Rust core: 112 disposable-data tests passed, including 57/7 round trips, older and newer profile transfers, repeated imports, attachment paths, exclusions, and refusal of lossy downgrades.
- Rust formatting and Clippy: passed.
- Frontend: 28 tests passed, and the production bundle built.
- Windows x64 and ARM64: the native app and Rust engine compiled, and per-user installers were packaged. Startup probes were skipped, so neither app nor engine was launched on the build PC.
- macOS Apple Silicon: the CI test, lint, app, and DMG jobs passed. The ad-hoc signed DMG and app archive were downloaded and matched their SHA-256 checksums.

Still to verify on real devices: installer startup, a 57/7 Push followed by Pull, Codex history display and continuation, and recovery after interruption. Those checks should use disposable Codex profiles first.
