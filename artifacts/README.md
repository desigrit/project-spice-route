# Windows installers

[Spice Route 1.4.3 for Windows x64](Spice-Route-1.4.3-windows-x64-setup.exe) is the current Workspace interface, built with WinUI 3. It applies the selected sweeping-sail icon without changing runtime or transfer behavior. The [0.3.2 Tauri maintenance installer](Spice-Route-0.3.2-x64-setup.exe) remains available for comparison.

The adjacent `.sha256` file records the installer's SHA-256 checksum. Compare it with the output of:

```powershell
Get-FileHash .\Spice-Route-1.4.3-windows-x64-setup.exe -Algorithm SHA256
```

These are unsigned testing installers. They install for the current Windows user and do not require Node.js, Rust, or development scripts. Version 1.4.3 bundles .NET, WinUI, and the VC runtime. Only the earlier Tauri interface requires WebView2. Git is needed when syncing Git repositories.

Version 1.4.3 updates earlier WinUI installations in place and shares their local configuration and recovery profile. The earlier Tauri app can remain installed, but close it before opening the current app. Neither installer has been launched as part of the current verification.

See the [main README](../README.md) for setup and the [Windows verification guide](../docs/testing-1.4.3.md) for checks and remaining acceptance work. The [maintenance verification guide](../docs/testing-0.3.2.md) covers the shared engine fixes.
