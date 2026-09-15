# Spice Route 0.2.2

Save confirmations are compact, disappear after four seconds, and belong only to the originating page. Navigation clears them permanently, and a late save completion cannot show feedback on another page. Important handoff and recovery messages retain their existing persistence.

Codex discovery now finds its bundled runtime even when the desktop app inherits a PATH without Codex. Version probes run without console windows. Overview displays the actual compatibility explanation instead of only a generic setup warning. Compatibility gates were not relaxed.

Read-only inspection on this PC confirmed Codex CLI 0.153.4, database migrations 52/6, and the exact supported schema fingerprint. The updated engine's diagnostic example reports `supported: true`. No live session content was changed or pushed.

Validation: 13 frontend tests and 29 Rust core tests pass. These include confirmation expiry/navigation/late completion and bundled-runtime identification. The Windows test installer is unsigned.

## First real handoff

1. Install `artifacts/Spice-Route-0.2.2-x64-setup.exe` and refresh Overview. If blocked, the displayed compatibility explanation identifies the remaining problem.
2. Check the cloud folder and selections. Save them before pushing.
3. Finish active Codex work and fully close Codex, including CLI writers. Spice Route also checks for them; a Push cannot run during this Codex conversation.
4. Choose Push, review its file estimate and any warnings, and publish. Save the handoff ID and let the cloud client finish syncing.
5. On the other device, install Spice Route, select the same cloud folder, match the visible handoff ID, and Pull with Codex closed. Review destination mappings and conflicts first.

Real cloud delivery, installed-app restoration, and opening/continuing the same session across devices remain manual acceptance checks. A successful source Push alone does not prove restoration. Existing automated disposable-profile restoration and rollback tests pass; this update does not claim physical-device validation.
