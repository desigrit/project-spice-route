# Spice Route 1.4.5 Windows verification

This release adds the Codex runtime `0.155.0-alpha.9.2` and database migrations `55/6` to the supported profiles. Install Spice Route 1.4.5 on both computers before exchanging snapshots produced from that profile. Existing settings, selections, recovery data, and snapshots remain in place.

## What changed

Codex migration 55 renames `thread_artifacts` to `thread_attachments`, renames its type column, and replaces the corresponding index. The history database remains at migration 6. The adapter translates the names when reading and restoring records, retaining the established snapshot representation and chat fingerprints. It does not add old tables to a live Codex database or edit migration ledgers.

Supported transfers preserve the destination's own database format. Versions 52, 54, and 55 can exchange records when their fields are representable at the destination. A schema-52 destination still rejects non-null `originator` or `daybreak_enabled` values before mutation. Unknown runtime/schema pairs, altered layouts, incomplete migrations, and malformed attachment records remain blocked.

## Automated checks

The release engine suite passes all **103 tests**. Eight added tests cover:

- 52 to 55, 54 to 55, 55 to 54, 55 to 55, and representable 55 to 52 transfers, including return transfers.
- Attachment types, opaque payloads, image objects, operational path mapping, and unchanged historical text.
- Selection exclusions, destination-only record preservation, replacement of stale attachments, deletion, and repeated imports without duplicates.
- Database-backed history, destination permissions and credentials, schema preservation, and unchanged migration ledgers.
- Equal fingerprints for equivalent schema-54 and schema-55 chats, with changed types or payloads still detected.
- Downgrade refusal and malformed-row preflight before either destination database changes.
- Exact runtime/schema validation and clearer messages for unknown migrations or missing native tables.

The six-test transfer module was rerun after strengthening its stale-attachment replacement and post-import compatibility assertions. Release Clippy passes with warnings denied, and Rust formatting checks pass.

## Package verification

The Windows installer includes the updated shared engine and the existing WinUI interface. Verification uses the hidden startup probe and 13 headless native engine-client contract checks. Writes in those checks use a disposable Spice Route profile; diagnostics may inspect detected Codex directories read-only. The live Codex compatibility probe reads schema metadata and queries the bundled runtime version without constructing the sync engine or performing a transfer. It confirmed that the current local profile passes both the exact schema-55 check and the `0.155.0-alpha.9.2` runtime gate.

The installer is `artifacts/Spice-Route-1.4.5-windows-x64-setup.exe`, with an adjacent SHA-256 checksum. This is a clean installation in `%LOCALAPPDATA%\Programs\Spice Route`; uninstall an earlier version first. No legacy-folder migration or cleanup is performed. The folder, Start shortcut, and displayed product name use Spice Route, and the new internal ownership and uninstall identifiers no longer use the preview label. Configuration and recovery data remain separate from the installed application.

The installer compiles, but clean installation and uninstall still need an interactive check on a disposable Windows installation. Neither the installer nor an interactive app window is launched during verification. No personal-data Push or Pull is performed.

The interface is unchanged from 1.4.4. The README retains that release's actual sample-data screenshots and their version caption. The earlier [native visual verification](testing-1.4.4.md) documents those captures.

## Remaining device acceptance

These checks establish the tested database transfer contract. They do not prove that Codex displays and continues imported history on the other computer. The previously reported missing-history issue still requires a real cross-device Pull and inspection of the affected desktop profile, including its catalog and host mappings. This release fixes the schema-55 compatibility block; it does not claim that separate visibility issue is resolved.

After updating both computers, verify a small selected handoff in both directions. Confirm that the intended tasks appear in Codex, attachments open, continuation stays on the same task, and excluded or unrelated local tasks remain intact. A failed visibility check should be investigated using **Recovery > Diagnose missing chats > Export log** on the receiving computer.

See the [compatibility design](compatibility-plan.md) for the supported profiles and transfer boundaries.
