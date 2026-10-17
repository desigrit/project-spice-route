# Desktop packages

Spice Route 1.6.9 supports Codex database layout 61/8 on Windows x64, Windows ARM64, and Apple Silicon. Already on 1.6.8 or later? Open **Settings > Updates**, check for updates, and choose **Install and restart**.

- [Windows x64](https://github.com/desigrit/project-spice-route/releases/download/v1.6.9/Spice-Route-1.6.9-windows-x64-setup.exe), for Intel and AMD computers
- [Windows ARM64](https://github.com/desigrit/project-spice-route/releases/download/v1.6.9/Spice-Route-1.6.9-windows-arm64-setup.exe), for Windows on Arm computers
- [macOS Apple Silicon](https://github.com/desigrit/project-spice-route/releases/download/v1.6.9/Spice-Route-1.6.9-macos-apple-silicon.dmg), for M-series Macs
- [Archived macOS Intel 1.5.2](macos-intel/Spice-Route-1.5.2-macos-intel.dmg), the final Intel Mac release

The installers reject the wrong native architecture instead of starting an incompatible app and failing later. The installer replaces the complete installed payload during an upgrade, validates the packaged C++ runtime architecture, and keeps a startup error page open with the bounded diagnostic log path. It also enables Push and Pull across Codex patch-runtime differences when the database still matches a complete tested storage profile.

GitHub release assets include `.sha256` files recording each Windows installer's SHA-256 checksum. Compare them with the output of:

```powershell
Get-FileHash .\Spice-Route-1.6.9-windows-x64-setup.exe -Algorithm SHA256
Get-FileHash .\Spice-Route-1.6.9-windows-arm64-setup.exe -Algorithm SHA256
```

These are unsigned testing installers. They install for the current Windows user and do not require Node.js, Rust, or development scripts. Each Windows package bundles .NET, WinUI, and the matching VC runtime. Git is needed when syncing Git repositories.

Future Mac builds target Apple Silicon only. The Mac folders include a DMG, a zipped application, and SHA-256 checksums. These first Mac packages are ad-hoc signed and are not notarized. They are intended for hands-on testing on macOS 13 or newer.

Windows uses `%LOCALAPPDATA%\Programs\Spice Route`. Close Spice Route before installing it. Local configuration and recovery data live separately from the installed application. The release workflow installs each Windows package, runs an offscreen startup probe with disposable data, verifies the installed engine, and uninstalls it. No personal Codex profile or live transfer is used.

See the [main README](../README.md) for setup and the [1.6.7 verification guide](../docs/testing-1.6.7.md) for checks and remaining acceptance work.
