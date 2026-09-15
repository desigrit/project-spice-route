# Windows preview installer

[Download Spice Route 0.3.1 for Windows x64](https://github.com/desigrit/project-spice-route/raw/refs/heads/main/artifacts/Spice-Route-0.3.1-x64-setup.exe).

The adjacent `.sha256` file records the installer's SHA-256 checksum. Compare it with the output of:

```powershell
Get-FileHash .\Spice-Route-0.3.1-x64-setup.exe -Algorithm SHA256
```

This is an unsigned preview installer. It installs for the current Windows user and does not require Node.js, Rust, or the development scripts. WebView2 is required by the desktop shell. Git is needed when syncing Git repositories.

Only the current installer is kept here. Earlier testing notes describe historical builds that are not included in this initial repository upload.

See the [main README](../README.md) for setup and the [0.3.1 testing guide](../docs/testing-0.3.1.md) for known limits.
