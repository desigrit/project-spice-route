# Spice Route 1.4.4 Windows verification

This update opens the navigation pane with menu labels by default, refines the Review file browser, and adds diagnostics for missing history after Pull.

## Review and navigation

The file browser aligns search, project filtering, column headers, and file rows. Native hover, selection, keyboard focus, and narrow-window behavior are covered by the offscreen sample-data probe. Page navigation is disabled while an operation is running, so a premature navigation click cannot leave a stale error banner above the review.

## Diagnosing missing chats

On the computer where history is missing, open **Recovery**, choose **Diagnose missing chats**, then select **Export log**. Attach the JSON file to the issue report. The checks can run while Codex is open and do not restore or change Codex data.

The report compares configured and discovered Codex folders, database and project metadata, the local sync baseline, and available pull records. New pulls record diagnostic events automatically. Earlier pulls can still be investigated using the current profile and retained snapshot/recovery metadata, but they do not have the new detailed event history.

The log excludes conversation text, database row contents, credentials, and complete configuration files. It includes folder metadata needed to distinguish profiles. A profile discovered from environment variables is a candidate, not proof that the running Codex process uses it. The report calls out this limitation.

## Verification

The core engine suite passes all 95 tests, including disposable profile restoration, missing references, visibility metadata, failed-pull logging, and exclusion of private content from reports. Release Clippy passes with warnings denied, and Rust formatting checks pass.

All 13 native engine-client contract checks pass, including the new diagnostic operation and its behavior for a missing database. Writes use a disposable profile. Diagnostics may inspect detected Codex folders read-only.

The actual WinUI interface was rendered offscreen in a non-activating window with in-memory sample data. Review, navigation, Overview, What to sync, Settings, and Diagnostics were inspected in light and dark themes. Wide and narrow Review layouts preserve the expanded menu and switch to a compact project picker when space is limited. The probe is isolated from the sync engine, folder pickers, export writes, and personal records. The Impeccable detector returned no findings; native rendering supplies the visual evidence because C# and XAML are not comprehensively covered by that detector.

All 51 native probe checks pass against the packaged build, including expanded navigation, aligned columns, filter behavior, genuine rendered selection and hover colors, busy navigation, diagnostic counts, and diagnostic requests limited to the read-only operation. It produces 18 base page captures plus control-state and filter-state images. The hidden startup probe also passes.

Existing engine coverage and the remaining live-device transfer tests are described in [1.4.1 verification](testing-1.4.1.md). This release adds evidence for diagnosing missing history. It does not claim to have reproduced or fixed the reported other-PC visibility failure.

The installer is `artifacts/Spice-Route-1.4.4-windows-x64-setup.exe`. It upgrades the existing per-user installation and preserves local configuration and recovery data. The installer is not launched during build verification.
