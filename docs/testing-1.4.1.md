# Spice Route 1.4.1 Windows verification

This build implements the approved Workspace design using actual WinUI 3 controls. It replaces the Windows web-rendered interface with a C# desktop shell and reuses the Rust engine through a hidden local JSON-line process.

The primary actions are **Push** and **Pull**. Push retains its up arrow. There is no Open Codex control. A transfer still requests a graceful Codex exit when necessary and blocks while known writers remain open.

## What is included

- Native navigation, light and dark themes, compact menus, and folder pickers.
- A Workspace Overview with consistent actions, timestamped handoff labels, and one count for each selection metric.
- A full-page Review with separate Files, Attention, and Notes tabs. Files use a virtualized list, project filters, search, and size sorting. Large files are surfaced before transfer.
- Explicit conflict choices that must be resolved before continuing. Choices and mappings cannot change while execution is running.
- Project selection, history-only size estimates, per-project folders, Settings, onboarding, and Recovery.
- Small, transient save feedback that stays on the relevant page.
- An engine connection that preserves request identities, forwards progress and errors, and rejects another process using the same app profile. Closing the connection cancels outstanding work before releasing its lock.
- The [engine performance and reliability changes](performance-audit.md) shared with the 0.3.2 maintenance build.

## Automated verification

| Check | Result |
| --- | --- |
| Complete Rust release suite | 92 passed |
| Rust formatting and release Clippy, all targets, warnings denied | Passed |
| Headless native engine-client contract checks | 11 passed |
| WinUI 3 Release compilation | Passed, zero warnings and errors |
| Self-contained Windows publish and NSIS packaging | Passed |
| Runtime payload inspection | .NET, WinUI, VC runtime, Rust sidecar, PRI, and compiled XAML included |
| Hidden packaged startup probe | Passed, MainWindow loaded with exit code 0 |
| Existing Tauri frontend suite | 24 passed |
| Existing Tauri TypeScript and production bundle | Passed |
| Interactive native layout, keyboard, folder picker, and high-DPI checks | Not run |
| Installation and startup on a clean Windows account | Not run |
| Live Windows A to B to A handoff | Not run |
| Actual OneDrive, Google Drive, and iCloud delivery | Not validated by these changes |

The packaged app ran only in startup-probe mode. It loaded the real application resources and constructed MainWindow without showing a window, starting the sync engine, or reading the Codex profile. The installer was not launched, no real Push or Pull was executed, and no personal selection settings were changed. This check does not prove interactive behavior on every Windows configuration.

Version 0.4.0 failed before engine startup for two independent XAML reasons. The publish output omitted the app resource index and compiled XAML files, and Recovery requested a `History` symbol that is not defined by the installed WinUI version. Version 1.4.1 copies the generated PRI and XBF files into every publish, checks that they exist before packaging, uses the supported `Clock` symbol, and runs the hidden startup probe before creating an installer.

The contract checks use a disposable profile and the actual frontend engine client. They cover concurrent request correlation, explicit and invalid conflict choices, actionable errors, configuration round trips, preserved device identity, progress lookup, profile locking, EOF shutdown, and retry after another instance releases the lock.

## Build and contract tests

The native project targets .NET 9 and Windows x64. The installer bundles the required application runtimes; users do not need to install development tools. Git remains an external dependency for Git repository transfers.

```powershell
./scripts/build-native-windows.ps1
dotnet run --project native/tests/EngineContract/EngineContract.csproj --configuration Release -- ./artifacts/native-win-x64/SpiceRoute.Engine.exe
```

Build prerequisites are listed in the [README](../README.md#build-from-source). The installer is `artifacts/Spice-Route-1.4.1-windows-x64-setup.exe`; its adjacent `.sha256` file records the exact artifact checksum.

## Installation and existing data

Spice Route installs per user and uses the existing `%LOCALAPPDATA%\com.spiceroute.codexsync` profile. To upgrade 0.4.0 safely, version 1.4.1 retains its legacy installation identity internally. Windows displays the product and Start menu shortcut as **Spice Route**, and the installer replaces the 0.4.0 entry instead of creating a duplicate. The earlier Tauri app can remain installed, but must be closed before starting Spice Route. Uninstalling Spice Route preserves the profile and sync data.

New profiles include project secrets and configuration by default. Existing saved choices remain unchanged. To include those files in an existing profile, enable **Include project secrets and configuration** in Settings and save. Additional exclusion patterns still apply. Codex account credentials and machine settings remain local.

Use version 1.4.1 on both computers for the first test. Keep the current cloud folder and local app profile; resetting cloud history is not required. Previously stored large objects remain referenced by their immutable snapshots.

If a fresh 1.4.1 installation has no saved local baseline, Push can explicitly replace every visible cloud head with the current selection. Review labels this as a cloud replacement and remains available even when the selected content has no ordinary diff rows. A second head appearing after preview invalidates the operation. The new handoff excludes projects and files omitted by the current selection. Older object blobs remain stored for ancestry safety; choose **Reset cloud history first** in the review if reclaiming that storage is part of the test, then prepare a new Push.

## Interactive acceptance

1. Open Spice Route with the earlier Tauri app closed. Confirm the existing device identity and folders appear. Check light and dark themes, keyboard navigation, 100% and 150% scaling, and folder picker behavior.
2. Confirm Overview becomes usable without recursively measuring every project. Open What to sync, change a project mode while estimates load, and confirm both the draft and the displayed size remain correct.
3. Prepare a small Push. Review Files, Attention, and Notes independently. Filter by project, search, and sort by size. Confirm the file list remains usable with thousands of entries and that no command windows appear.
4. Resolve a disposable conflict explicitly. Verify unselected choices block execution. Start the handoff and confirm inputs cannot change while cancellation remains available.
5. Test saving settings, recovery, and cloud cleanup success followed by a simulated refresh failure. The completed action must remain successful while stale status is reported separately.
6. Complete a disposable Windows A to B to A handoff, including Git staged changes, untracked files, different destination paths, history-only projects, and session continuation. Match the readable handoff label on both devices.
7. Exercise the [failure and recovery scenarios](testing-0.3.2.md#recovery-and-failure-cases), including a writer reopening during restoration and an interrupted rollback. Pending recovery must remain visible and recoverable.
8. On a disposable profile with no saved baseline and at least one visible cloud handoff, prepare Push with every project set to Chat history only or Excluded. Confirm Review identifies a cloud replacement, lists the current selection, enables Push, and rejects execution if another cloud head appears after preview. After Push, confirm the new handoff contains no project working files. If storage reclamation is required, use Reset cloud history first and repeat Push.

The live continuation, clean-machine installation, and provider matrix remain release gates. This is a testable preview, not a claim that those scenarios have already passed.
