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

Platform build results and package checksums will be added after verification. Real-device handoff through a cloud client remains a separate acceptance check.
