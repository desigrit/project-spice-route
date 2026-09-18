# Spice Route 1.4.3 Windows verification

This release applies the user-selected **C, sweeping sail** icon in flat navy and warm ivory. The Workspace layout, runtime behavior, snapshot storage, Codex compatibility, and shared Rust transfer engine are unchanged from 1.4.2.

## Verification

| Check | Result |
| --- | --- |
| Release compilation and self-contained publish | Passed |
| Hidden startup probe | Passed |
| App and installer icon sources | Selected C artwork, exported at all required sizes |
| NSIS installer packaging | Passed |
| Offscreen WinUI visual checks | 16 passed |

The canonical source is `app-icon.png`, copied unchanged from the selected C preview. Tauri's icon exporter creates the platform assets. Windows uses `Assets/Boat.png` in the title bar and `Assets/SpiceRoute.ico` for the executable and window icon. The installer uses the matching `src-tauri/icons/icon.ico`. The ICO contains 16, 24, 32, 48, 64, and 256 pixel variants.

The native, packaged, and installer icon files have matching hashes. The updated title-bar icon was inspected in the actual WinUI rendering with sample data. README screenshots were refreshed from the offscreen probe. The installed app and installer were not opened, and no personal data was accessed.

The earlier interface checks and native engine-client contract results are recorded in [1.4.2 verification](testing-1.4.2.md). Shared engine regression results and remaining transfer acceptance scenarios are recorded in [1.4.1 verification](testing-1.4.1.md). These are prior results, not a new engine test run for this icon update.

The installer is `artifacts/Spice-Route-1.4.3-windows-x64-setup.exe`, with an adjacent SHA-256 checksum. It updates the existing per-user WinUI installation and preserves the local profile. The build script does not launch the installer.

Keyboard and screen reader checks, high contrast and high-DPI checks, live Codex continuation, and Windows A to B to A transfers across the supported cloud providers remain separate acceptance work.
