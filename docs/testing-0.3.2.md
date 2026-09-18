# Spice Route 0.3.2 verification plan

This maintenance build addresses excessive startup work, transfer overhead, flashing command windows, and a false settings-change error. It retains the Tauri interface. The approved WinUI Workspace interface is packaged separately as [version 1.4.1](testing-1.4.1.md), using the same engine.

The Windows installer is `artifacts/Spice-Route-0.3.2-x64-setup.exe`, with its SHA-256 in the adjacent checksum file. No installed application, live Push, live Pull, or personal-profile restoration was launched as part of these changes.

## Release checks

| Check | Result |
| --- | --- |
| Complete Rust test suite | 88 passed |
| Complete frontend test suite | 22 passed |
| Focused `App.test.tsx` suite | 20 tests passed, including lazy loading, stale-result protection, and preserved selection drafts |
| TypeScript checking | Passed |
| Rust formatting and release Clippy | Passed, all targets and warnings denied |
| Production web bundle | Passed |
| Windows MSVC/NSIS installer | Passed |
| Installer SHA-256 | Recorded in the adjacent .sha256 file |
| Live Windows A to B to A round trip | Not run |
| Actual OneDrive, Google Drive, and iCloud delivery | Not validated in this patch |

These entries deliberately distinguish focused checks from the complete release suite. A passing unit suite does not demonstrate a complete cloud-client handoff.

## What changed

- Overview loads a quick catalog. Opening What to sync starts detailed workspace size estimates. Opening Recovery starts recovery-folder enumeration.
- Estimates show a pending state while loading. Errors appear on the relevant page, and late responses from an older refresh cannot replace newer results. Size estimates arriving in the background do not reset unsaved selection choices.
- Manifest reads use buffering. Actual content verification remains part of Pull; a visible manifest is not represented as verified content.
- Process monitoring avoids repeating a full process inventory for each restored file. Internal Windows commands launch without console windows.
- Object capture uses buffered I/O, lighter compression, and existing-object reuse where possible.
- Configuration and transfer-contract fingerprints are stable across equivalent map ordering. Actual settings changes still require a fresh review.
- Generated `packaging/out` and `packaging/staging` content follows the build-output setting. Models remain selected unless explicitly excluded.
- Handoff display labels include a timestamp and a unique suffix. Existing snapshot IDs and ancestry remain unchanged.

The [performance audit](performance-audit.md) contains measurements and their limits.

## Existing installations

Install the same maintenance version on both computers. Keep the existing cloud folder, AppData configuration, device identity, and recovery data. There is no need to reset cloud history to test this patch.

New configurations include project configuration and secrets by default. Existing configurations preserve their saved selection. If an older configuration should carry those files, open What to sync and enable **Include project configuration and secrets**, then save the choices. Global Codex sign-in and machine settings stay local. Custom exclusion patterns continue to apply.

A large model already stored in an immutable snapshot remains in cloud storage. Excluding it from a later selection stops future transfer; it does not remove retained objects from older snapshots.

## Suggested acceptance run

Use disposable project data and test sessions first. Record the source device, readable handoff label, selected content size, and cloud client state for each transfer.

1. Open Overview. Confirm it becomes usable without starting a full project-size calculation or scanning the rollback tree. Navigate to What to sync and verify estimates appear progressively. Change a selection while estimates are loading and confirm the draft remains intact.
2. Open Recovery. Confirm its loading state is local to that page. Return to Overview and check that recovery read errors, if deliberately induced, do not follow the user across pages.
3. Prepare a Push containing a small project, a Git repository, an untracked file, and a model-sized test file. Confirm generated packaging folders follow the build-output setting and model files remain included. Confirm internal command windows do not flash.
4. Complete the Push with Codex closed. Record the local publication time separately from the drive client's upload completion. Run a later Push with mostly unchanged content to exercise object reuse; do not compare only an initial upload against an unchanged handoff.
5. Wait for the receiving drive client to deliver the snapshot. On the second PC, match its timestamped label and source device. Map project folders to that PC's chosen locations, then review the incoming content.
6. Complete Pull without modifying settings between review and execution. The earlier false **Settings changed after this preview** failure must not recur. Verify history, workspace files, staged changes, untracked files, and session continuation.
7. Make a small change on the second PC, Push, and Pull it on the first. Confirm the same sessions continue without duplicate entries and the handoff ancestry remains connected.

## Recovery and failure cases

Run these against disposable profiles and cloud folders:

- Change a project mapping or sync selection after preview. Execution must request a fresh review because the change is real.
- Publish a new cloud head after preview. Execution must stop for another review rather than silently applying an outdated decision.
- Reopen a Codex writer during transfer. The operation must detect it, stop safely, and preserve recoverable state if restoration had started.
- Interrupt restoration, restart the app, and recover through the durable journal. A pending recovery must block a new conflicting operation.
- Deliver the manifest before an object, leave an object as an unavailable placeholder, or corrupt stored bytes. Pull must fail clearly without declaring the snapshot received and verified.
- Cancel during capture or verification. Confirm cancellation reaches a safe checkpoint and does not publish an incomplete snapshot.
- Repeat an already reviewed Pull. Confirm it does not duplicate sessions or needlessly replace unchanged files.
- Reject access to a selected folder, exhaust free space, and use an unsupported Codex schema. Confirm the failure identifies the affected step and preserves the previous usable state.

## Still outside this build's evidence

The local measurements do not prove a specific multi-gigabyte transfer duration, universal cloud-client behavior, native WinUI rendering, or lossless round-trip behavior on the user's two live machines. Those claims require the corresponding acceptance runs. The release remains a preview until the live recovery and continuation checks have been demonstrated.
