# Spice Route 1.4.2 Windows verification

This update refines the existing WinUI 3 Workspace interface. It does not change snapshot storage, Codex compatibility, or the shared Rust transfer engine.

## Interface changes

- The native hamburger opens and closes the navigation pane. The ship remains in the title bar.
- Primary actions use the native accent template. Navy stays visible on hover and press in Light mode, with the corresponding ivory treatment in Dark mode. Native disabled and keyboard focus states remain available.
- What to sync has visible Projects, Project chats, and Projectless chats tabs, aligned project rows, selected-size estimates, compact mode menus, and folder ellipsis controls. Narrow windows move folders beneath their project.
- Settings groups related options into icon-led rows. Controls move beneath their labels in narrow windows. Additional exclusions and cloud history use expanders.
- Save feedback stays on its page and disappears after four seconds. Changing another setting immediately returns to the unsaved state.
- Text, icons, dividers, status badges, and panel surfaces use theme resources so appearance changes update the existing page.

The installed icon is unchanged while the user chooses among the proposed simpler ship designs.

## Verification

The native engine-client contract suite passed all 11 checks using a disposable profile. The shared engine's previous regression results and remaining transfer acceptance scenarios are documented in [1.4.1 verification](testing-1.4.1.md).

| Check | Result |
| --- | --- |
| Release compilation, self-contained publish, and installer packaging | Passed |
| Hidden startup probe, including all changed pages | Passed |
| Native engine-client contract checks | 11 passed |
| Offscreen visual and control-state assertions | 16 passed |
| Light, dark, wide, and narrow page captures | Inspected |
| Impeccable detector | No findings; native visual checks provide the relevant evidence |

The rendered Push samples are `#123B59` normally, `#0D3049` on hover, and `#09283D` when pressed in Light mode. The Dark equivalents are `#F1E5C3`, `#FFF1CC`, and `#D8CCAD`. The probe also confirms that project controls are enabled, the hamburger is present, and changing a full project to chat history updates its size and enables Save. The first visual pass caught system-accent hover colors; the final template resolves its state brushes directly from the app's theme tokens.

Current sample captures: [Overview](images/native-overview-light.png), [What to sync](images/native-selection-dark.png), [Settings](images/native-settings-light.png).

The visual probe uses fabricated device, project, chat, and handoff data. Its in-memory engine responder rejects unexpected operations. The window is placed offscreen before being shown without activation, and its actual WinUI visual tree is captured with RenderTargetBitmap. It does not start the Rust engine, access a Codex profile, run a transfer, or activate a foreground window.

To reproduce after building:

```powershell
./scripts/build-native-windows.ps1
$probe = Start-Process -FilePath ./artifacts/native-win-x64/SpiceRoute.exe -ArgumentList '--visual-probe', 'D:\SpiceRouteVisualChecks' -WindowStyle Hidden -PassThru
$probe.WaitForExit()
$probe.ExitCode
```

The installer is `artifacts/Spice-Route-1.4.2-windows-x64-setup.exe`. It upgrades the existing per-user WinUI installation and preserves the local profile. The installer is not launched by the build script.

## Remaining interactive checks

Check keyboard focus and screen reader announcements, the folder picker, Windows high contrast, high-DPI displays, and narrow windows on the target computer. Offscreen sample captures do not establish successful cloud delivery or live Codex continuation. Windows A to B to A transfers and the cloud-provider matrix remain separate acceptance work.
