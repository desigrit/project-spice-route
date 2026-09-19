# Spice Route Windows design

Approved direction: **B, Workspace**, selected by the user on September 18, 2026.

Approved brand icon: **C, sweeping sail**, selected by the user on September 18, 2026. Use the simplified ship in flat navy and warm ivory across the app and installer.

The Windows interface uses actual WinUI 3 controls through Windows App SDK. The Rust sync engine remains reusable. The existing Tauri maintenance build is a separate release and must not be represented as WinUI.

## Composition

Use an expanded `NavigationView` with visible menu labels by default, a native command bar, and a clear workspace divided into the current computer and the latest visible cloud handoff. Show chat and project counts once. Keep navy and warm ivory from the supplied ship identity, with native light, dark, and high-contrast resources.

Review occupies a full page. Files, Attention, and Notes use separate tabs. A project filter and virtualized file list keep large transfers usable. Search and largest-first sorting make size contributors easy to find. Routine capture information belongs in Notes; only a decision or a concrete problem belongs in Attention.

## Explicit user decisions

- Remove Open Codex controls throughout the application.
- Label the review action **Push**, retaining its up-arrow icon. Codex closure remains an execution check; the button does not need to explain that implementation detail.
- Preserve the selected Workspace arrangement. Keep the native hamburger toggle at the top of the navigation rail and the ship icon in the title bar.
- Start with the navigation pane open and menu labels visible. Use native menu-item spacing, without extra per-item margins.
- Apply the selected **C, sweeping sail** icon in flat navy and ivory. Preserve the current Workspace arrangement and application behavior.
- Do not use em dashes in public content.

## Behavior

Show progress inline with a named operation phase and cancel control. Use real step totals when known. Do not show a fake percent during indeterminate preparation. Keep errors with their review and preserve conflict decisions. Group folder mappings and conflicts in Attention.

Use a readable date/time plus the unique suffix for handoffs. Keep copied identifiers unambiguous, and compare snapshot ancestry independently of display dates. A locally saved snapshot does not establish cloud completion.

Disable page navigation while a handoff operation is running. An unavailable navigation action must not leave a persistent error banner above the page. Review remains available for progress and cancellation.

Recovery provides a Diagnostics page for missing history after Pull. Show readable findings and an explicit Export log action. Reports contain versions, folder metadata, record counts, and bounded pull events, with no conversation text or credentials. Keep restoration success separate from proof that the external Codex UI displays the restored sessions.

When this installation has no saved baseline, Push can replace every visible cloud head after an explicit review. The review states that only the current selection will appear in the new handoff. Older immutable objects remain stored until the user chooses Reset cloud history, which is offered as a separate action when storage needs to be reclaimed.

Use native keyboard navigation, automation names, virtualization, DPI behavior, and system theme resources. Avoid blocking the UI thread with engine work or creating child console windows. Large model files remain visible and selected unless the user changes selection.

Primary actions use the native accent-button template, including hover, pressed, disabled, and keyboard focus states. Keep the accent fill on hover. All foreground and surface brushes must resolve through theme resources, including when appearance changes inside Settings.

What to sync uses visible scope tabs and aligned project rows: identity and selected size, local folder with an ellipsis action, and sync mode. Settings uses grouped icon-led rows with controls on the right, moving below the label at narrower widths. Advanced exclusions and cloud cleanup sit in expanders. Save feedback stays brief and local to its page.

Reference: `docs/design/native-options/b-board.png`. The reference contains illustrative data, not a live transfer.
