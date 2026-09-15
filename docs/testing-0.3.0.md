# Spice Route 0.3.0 — tested cross-version storage

Install `artifacts/Spice-Route-0.3.0-x64-setup.exe` on both PCs. This is an unsigned Windows test build; it does not require Node.js or developer tools.

## Supported combinations

| Source → destination | Support |
|---|---|
| Runtime 0.153.4 / schema 52 → same profile | Supported |
| Runtime 0.153.4 / schema 52 → runtime 0.154.0-alpha.6.2 / schema 54 | Supported; destination schema is preserved |
| Runtime 0.154.0-alpha.6.2 / schema 54 → same profile | Supported, including newer fields |
| Schema 54 → schema 52 | Supported when `originator` and `daybreak_enabled` are null; otherwise blocked before applying |
| Unknown runtimes, schemas, altered triggers, or mismatched runtime/schema pairs | Blocked with a diagnostic explanation |

When importing an older chat onto schema 54, existing destination values in newer columns remain intact. A newer source's explicit null can clear such a field on schema 54. Unsupported values are never silently discarded. Older Codex cannot be made to understand newer feature semantics by copying data.

## Use the existing snapshot

1. Install this build on the destination PC and refresh. The reported 0.154.0-alpha.6.2 / 54/6 profile should pass compatibility checks.
2. Confirm the source device and snapshot identifier match your published snapshot, and review incoming folder mappings.
3. Finish active Codex work and close Codex fully. Preview Pull and inspect conflicts/warnings before applying.
4. Open Codex manually afterward, confirm the restored history, and continue a selected session. That is the remaining real-device acceptance check.
5. Update Spice Route on the source PC before the next Push or Pull.

Existing format-1 snapshots are readable without re-uploading them. New snapshots use format 2 to prevent old Spice Route builds from applying records using their old silent-column-dropping behavior. Do not reset cloud history to resolve a version warning. Future runtime changes may require another tested compatibility profile; a matching migration number alone is insufficient.

## Evidence

- Sanitized schema definitions from both PCs are embedded as fixtures. Tests construct disposable databases from the exact tables, indexes, and triggers; the production compatibility check has no test bypass.
- 36 Rust core tests pass, including all four schema pairs, repeated imports, an additional history item and return transfer, exclusions, schema/ledger preservation, realtime history, rejection of non-null newer fields, unknown-column and trigger rejection, source-profile revalidation, mixed format-1/2 ancestry, and byte-for-byte database rollback on schema 54.
- Release Clippy passes with warnings denied; all 13 frontend tests and the TypeScript/production frontend build pass.
- The Windows MSVC release and NSIS installer build pass. The installer and SHA-256 file are in `artifacts`.
- No live Push, Pull, database migration, Codex restart, or installed-app launch was performed while preparing this release. Test results establish the storage contract, not a guarantee about every future Codex payload or feature.

See [the compatibility design](compatibility-plan.md) for the version policy and release boundaries.
