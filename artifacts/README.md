# Windows installers

[Spice Route 1.4.5 for Windows x64](Spice-Route-1.4.5-windows-x64-setup.exe) is the current Workspace interface, built with WinUI 3. It adds support for Codex schema 55, with tested attachment conversion across supported database formats. Install it on both computers before exchanging snapshots from the newer Codex runtime. The [0.3.2 Tauri maintenance installer](Spice-Route-0.3.2-x64-setup.exe) remains available for comparison.

The adjacent `.sha256` file records the installer's SHA-256 checksum. Compare it with the output of:

```powershell
Get-FileHash .\Spice-Route-1.4.5-windows-x64-setup.exe -Algorithm SHA256
```

These are unsigned testing installers. They install for the current Windows user and do not require Node.js, Rust, or development scripts. Version 1.4.5 bundles .NET, WinUI, and the VC runtime. Only the earlier Tauri interface requires WebView2. Git is needed when syncing Git repositories.

Version 1.4.5 is a clean installer that uses `%LOCALAPPDATA%\Programs\Spice Route`. Uninstall an earlier version first. The installer does not migrate the old program folder. Local configuration and recovery data live separately from the installed application. Neither installer has been launched as part of the current verification.

See the [main README](../README.md) for setup and the [Windows verification guide](../docs/testing-1.4.5.md) for checks and remaining acceptance work. The [maintenance verification guide](../docs/testing-0.3.2.md) covers the shared engine fixes.
