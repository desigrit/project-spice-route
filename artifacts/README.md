# Desktop packages

Spice Route 1.5.0 is the current Workspace interface, built with WinUI 3 and available for both Windows processor families:

- [Windows x64](Spice-Route-1.5.0-windows-x64-setup.exe), for Intel and AMD computers
- [Windows ARM64](Spice-Route-1.5.0-windows-arm64-setup.exe), for Windows on Arm computers
- [macOS Apple Silicon](macos-apple-silicon/Spice-Route-1.5.0-macos-apple-silicon.dmg), for M-series Macs
- [macOS Intel](macos-intel/Spice-Route-1.5.0-macos-intel.dmg), for Intel Macs

The installers reject the wrong native architecture instead of starting an incompatible app and failing later. This release also writes bounded startup diagnostics to `%LOCALAPPDATA%\com.spiceroute.codexsync\logs\startup.log` if the app or sync engine cannot start. It retains support for Codex schema 55 and tested attachment conversion across supported database formats.

The adjacent `.sha256` file records the installer's SHA-256 checksum. Compare it with the output of:

```powershell
Get-FileHash .\Spice-Route-1.5.0-windows-x64-setup.exe -Algorithm SHA256
Get-FileHash .\Spice-Route-1.5.0-windows-arm64-setup.exe -Algorithm SHA256
```

These are unsigned testing installers. They install for the current Windows user and do not require Node.js, Rust, or development scripts. Version 1.5.0 bundles .NET, WinUI, and the matching VC runtime. Git is needed when syncing Git repositories.

The Mac folders include a DMG, a zipped application, and SHA-256 checksums. These first Mac packages are ad-hoc signed and are not notarized. They are intended for hands-on testing on macOS 13 or newer.

Version 1.5.0 is a clean installer that uses `%LOCALAPPDATA%\Programs\Spice Route`. Uninstall an earlier version first. The installer does not migrate the old program folder. Local configuration and recovery data live separately from the installed application. Neither installer was launched during packaging.

See the [main README](../README.md) for setup and the [1.5.0 verification guide](../docs/testing-1.5.0.md) for checks and remaining acceptance work.
