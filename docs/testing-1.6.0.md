# Spice Route 1.6.0 verification

This release implements the approved B Side by side Overview and C Workbench What to sync page in Windows WinUI 3 and the React/Tauri Mac interface. The database adapters and transfer engine behavior are unchanged. Intel Mac builds are discontinued; 1.5.2 remains the final archived Intel package.

## Verified locally

- 27 frontend tests passed. Coverage includes transfer gating, independent project and worktree mappings, mode-dependent sizes, archive and chat exclusions, keyboard selection, empty search, and recalculation after a folder change.
- TypeScript and the Vite production build passed. Vite still reports the existing large-bundle advisory.
- WinUI Release compilation passed without warnings or errors.
- The native offscreen probe passed 59 checks using fabricated data. It covers light and dark themes, wide and narrow layouts, accent-button states, enabled controls, live selected sizes, preserved project selection, delayed and rejected size scans, multi-root folders, search, and expanded navigation. No sync engine process starts in this probe.
- The actual React components were rendered headlessly at 1180 × 820 and 920 × 620 in light and dark mode. No horizontal overflow was detected. These captures are interface checks on Windows, not native macOS execution.
- All 13 native engine-client contract checks passed against a disposable profile.
- The desktop Rust command layer passed cargo check. The only new command lists snapshot metadata for the Mac Overview; it does not read or hydrate content objects.
- The Impeccable detector reported no findings for the new workspace stylesheet.

The independent design review found the B/C compositions faithful to the approved references. Its one material finding, an inspector size that could remain stale during a failed scan, was corrected and scored resolved.

## Release package verification

The [1.6.0 release workflow](https://github.com/desigrit/project-spice-route/actions/runs/35719731943) builds source commit `f8b35318601d1e364c0803246a98424b093dba00`.

- Windows x64 and ARM64: 109 engine tests on each architecture, engine lint, frontend tests, production builds, payload architecture checks, packaged startup probes, installed startup probes, and 13 installed engine-client checks per architecture passed. The downloaded installers match their published SHA-256 checksums.
- macOS Apple Silicon: 106 engine tests, engine lint, frontend tests, production build, binary architecture, ad-hoc signature, and Apple Events entitlement checks passed. The downloaded DMG and application archive match their published SHA-256 checksums.

The Mac package is ad-hoc signed, not notarized. CI does not establish real-device cloud delivery or Codex continuation.

## Try the new interface

1. Confirm that Overview shows this device and the latest visible handoff in separate columns.
2. Select a project in What to sync. Change Full project to Chat history only and check the size in both the table and detail pane.
3. Select a project with multiple roots. Confirm each folder has its own change action.
4. Filter the project list, then use arrow keys to select another project. A search with no results should clear the detail pane.
5. Check the project and projectless chat tabs. Archived chats and excluded projects should follow the saved rules.
6. Save choices, return to Overview, and review Push or Pull. Existing compatibility, recovery and conflict checks still apply.

Local verification did not modify a personal profile, execute a real Push or Pull, or install the app. Physical-device testing on Windows ARM64 and macOS, including real cloud delivery and Codex continuation, remains a separate acceptance step.
