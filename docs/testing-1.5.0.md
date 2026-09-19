# Spice Route 1.5.0 desktop verification

September 19, 2026. This release adds a native Windows ARM64 package, startup diagnostics for the WinUI app and sync engine, and macOS packaging for Apple Silicon and Intel. Windows and macOS use the same Rust engine and snapshot format.

## ARM startup finding

The previously published Windows package contained x64 PE executables only. The affected second computer reports ARM64 hardware. Windows 11 can emulate many x64 applications, so this architecture mismatch does not prove the exact old crash without its event or crash log. It is the strongest concrete packaging risk found in the released payload, and the old startup probe did not start the sync engine.

Version 1.5.0 removes that ambiguity:

- The x64 package contains x64 app and engine executables.
- The ARM64 package contains ARM64 app and engine executables.
- Each installer blocks installation on the wrong native processor architecture.
- Packaging checks the PE machine field for both executables.
- Engine startup has a 15-second response deadline and stops a stuck child process.
- Startup failures are recorded in `%LOCALAPPDATA%\com.spiceroute.codexsync\logs\startup.log`.

The log rotates at 256 KiB and records process, operating-system, and engine architecture, failure type, HRESULT, engine exit code, and bounded stderr. Control characters are removed. Build probes suppress persistent logging and use disposable data.

## macOS implementation

The macOS application uses Tauri, React, the system WebKit view, and the shared Rust engine. It includes:

- Apple Silicon and Intel build targets.
- `.app` and `.dmg` bundles for macOS 13 or newer.
- Native traffic-light placement and an overlay title bar.
- Mac-specific Workspace styling with the same navigation and sync concepts as Windows.
- iCloud Drive discovery through `~/Library/Mobile Documents/com~apple~CloudDocs`.
- Google Drive and OneDrive discovery through `~/Library/CloudStorage`.
- Codex runtime discovery through `CODEX_INSTALL_DIR`, common Codex.app resource paths, `~/.local/bin`, Homebrew paths, and `PATH`.
- A graceful Codex quit request through AppleScript, followed by the existing writer check.
- The same diagnostics export, compatibility checks, snapshot format, conflict rules, and recovery engine as Windows.

CI packages the Mac app with an ad-hoc signature. It is not notarized. An Apple Developer identity and notarization remain required for normal public distribution.

## Completed checks

- 107 release Rust engine tests passed on Windows, and 108 passed on macOS with the Unix path regression.
- Release Clippy passed with warnings denied.
- 24 frontend tests passed.
- The TypeScript and Vite production build passed.
- 13 headless WinUI engine-client contract checks passed against the packaged x64 engine.
- Native WinUI x64 and ARM64 builds completed with zero warnings.
- The x64 app and engine report PE machine `0x8664`.
- The ARM64 app and engine report PE machine `0xAA64`.
- Both unsigned per-user installers and SHA-256 files were produced without launching the installed app.
- Apple Silicon and Intel `.app`, `.dmg`, and SHA-256 files were produced with ad-hoc signatures.
- The [four-platform 1.5.0 workflow](https://github.com/desigrit/project-spice-route/actions/runs/35428204232) completed successfully from commit `0ae9151`.
- Rust formatting and `git diff --check` passed.

## Produced Windows packages

| Package | Size | SHA-256 |
| --- | ---: | --- |
| `Spice-Route-1.5.0-windows-x64-setup.exe` | 67,126,117 bytes | `8969d0d21040125cb38f2db588a8a60829681386b4f05dba4a41bee0f02375c4` |
| `Spice-Route-1.5.0-windows-arm64-setup.exe` | 62,106,822 bytes | `d8f8f3d65ab05ec9ff14442fd3e675d5a26b2a0d035e2d8a61925922602183c2` |

## Produced macOS packages

| Package | Size | SHA-256 |
| --- | ---: | --- |
| `Spice-Route-1.5.0-macos-apple-silicon.dmg` | 8,580,131 bytes | `d6196773ceea057bd47e0b48c8fd8cfd07e8488a340943f4817a253240c8a23f` |
| `Spice-Route-1.5.0-macos-apple-silicon.app.zip` | 7,178,348 bytes | `a5c37c99fd5ecac4a0421177f5fe896ebd920b573ea8d7084a20d2330569e41a` |
| `Spice-Route-1.5.0-macos-intel.dmg` | 9,066,958 bytes | `35fed925da8856fb75847f20ddea8959f73ce587c6a650dee440234802a557b6` |
| `Spice-Route-1.5.0-macos-intel.app.zip` | 7,642,804 bytes | `e318c76e49d9f9b0605c99f52367de64fafad1cbbd05cd807cfabac3f46ecee6` |

## Remaining device checks

Both Mac architectures now build, sign, package, and verify in CI. A hands-on Mac test must still confirm the installed Codex runtime path, accepted runtime and schema pair, File Provider hydration, Codex quit behavior, and a full Push and Pull with a disposable profile.

The ARM computer should install the ARM64 package, open Spice Route, and confirm that workspace inspection completes. If startup still fails, collect `startup.log`. That log should identify whether the remaining cause is the WinUI runtime, engine launch, engine response, or another exception.

No installed application, personal-data Push, personal-data Pull, or live Codex restoration was launched during this verification.
