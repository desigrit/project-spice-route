# Spice Route 1.6.7 verification

This release supports the Codex 59/8 databases inspected on October 13, 2026, with desktop build 26.1007.2314. It also supports exact combinations 59/7, 59/6, and 57/8. The stored fixture contains schema definitions, with no personal records.

History migration 8 adds `thread_turns.root_turn_id`. Transfers to older supported history databases preserve populated values in local Spice Route data. That file is journaled with the databases and restored during rollback. A later Push includes the retained values; missing or replaced turns block publication. The database schema on the receiving computer stays native to its installed Codex version.

The regression suite covers:

- Export, import, and repeated Pull with each supported profile, including Windows and Mac line endings.
- History roots and timing values returning intact from every supported profile to 59/8.
- Continued chats, individual exclusions, unrelated local chats, attachments, and unchanged-content fingerprints.
- Rejection of unprotected imports, unexpected triggers, missing indexes, failed migrations, invalid metadata, and changed turn identities.
- Recovery of databases and retained root metadata after interrupted replacement.
- Reading legacy retained-history format 1 and rejecting unknown formats.

Local settings format 3 prevents older Spice Route builds from reopening a profile after this upgrade. Install 1.6.7 on the computers used together.

Local validation passed: 123 engine tests, strict Rust Clippy, 29 frontend tests, and the production frontend build. Tests use disposable profiles; no personal Codex records were imported or changed.

The [final desktop build](https://github.com/desigrit/project-spice-route/actions/runs/38491067713) passed on all three platforms:

- Windows x64 and ARM64: 123 engine tests each, 29 frontend tests each, strict Clippy, packaged startup probes, and installation checks. Each installed engine passed 13 native client contract checks.
- Apple Silicon: 120 engine tests, 29 frontend tests, strict Clippy, and an ad-hoc signed app and DMG with verified architecture and entitlements.
- WinUI: all 59 offscreen visual checks passed, covering wide and narrow layouts in light and dark themes.

The Windows startup probe now loads the engine's current default settings instead of hardcoding a settings version. All startup and installation checks ran on disposable CI profiles.

The downloaded packages match their CI SHA-256 values:

| Package | SHA-256 |
| --- | --- |
| Windows x64 installer | `82ac37fbc1c8e4622be43cefc692c18ee65b8b7394cfe0ebf845ae0a5f336ffd` |
| Windows ARM64 installer | `17239a7a4bad6bfaddc3ab27f166307179ef9d6b36875c5dbd2719b609200e0f` |
| Apple Silicon DMG | `7e57f8cce297f29120f866daa17cc11b458807b2261aca5eb467a964c1d2126f` |
| Apple Silicon app ZIP | `6e2aea08509c3051d0e56e555e35060269b6b53fb2b5abc822113bc235975838` |

Real-device handoff through a cloud client remains a separate acceptance check. No personal database was restored, and the installed app on the working computer was not launched.
