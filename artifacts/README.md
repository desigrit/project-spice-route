# Desktop packages

Spice Route 1.5.2 is the current Workspace interface, built with WinUI 3 and available for both Windows processor families:

- [Windows x64](Spice-Route-1.5.2-windows-x64-setup.exe), for Intel and AMD computers
- [Windows ARM64](Spice-Route-1.5.2-windows-arm64-setup.exe), for Windows on Arm computers
- [macOS Apple Silicon](macos-apple-silicon/Spice-Route-1.5.2-macos-apple-silicon.dmg), for M-series Macs
- [macOS Intel](macos-intel/Spice-Route-1.5.2-macos-intel.dmg), for Intel Macs

The installers reject the wrong native architecture instead of starting an incompatible app and failing later. Version 1.5.2 replaces the complete installed payload during an upgrade, validates the packaged C++ runtime architecture, and keeps a startup error page open with the bounded diagnostic log path. It also enables Push and Pull across Codex patch-runtime differences when the database still matches a complete tested storage profile.

The adjacent `.sha256` file records the installer's SHA-256 checksum. Compare it with the output of:

```powershell
Get-FileHash .\Spice-Route-1.5.2-windows-x64-setup.exe -Algorithm SHA256
Get-FileHash .\Spice-Route-1.5.2-windows-arm64-setup.exe -Algorithm SHA256
```

These are unsigned testing installers. They install for the current Windows user and do not require Node.js, Rust, or development scripts. Version 1.5.2 bundles .NET, WinUI, and the matching VC runtime. Git is needed when syncing Git repositories.

The Mac folders include a DMG, a zipped application, and SHA-256 checksums. These first Mac packages are ad-hoc signed and are not notarized. They are intended for hands-on testing on macOS 13 or newer.

Version 1.5.2 uses `%LOCALAPPDATA%\Programs\Spice Route`. Close Spice Route before installing it. Local configuration and recovery data live separately from the installed application. The release workflow installs each Windows package, runs an offscreen startup probe with disposable data, verifies the installed engine, and uninstalls it. No personal Codex profile or live transfer is used.

See the [main README](../README.md) for setup and the [1.5.2 verification guide](../docs/testing-1.5.2.md) for checks and remaining acceptance work.
