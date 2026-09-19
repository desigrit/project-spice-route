# Spice Route 1.5.2 verification

## Compatibility and Windows startup corrections

Version 1.5.2 separates two independent checks. The Codex runtime string is diagnostic when the database matches a complete tested profile. Tables, indexes, triggers, required columns, and migration completion remain authoritative, and an unknown or altered profile still blocks Push and Pull.

The Windows installer now replaces the complete app payload during an upgrade. The build copies target-specific C++ runtime files without preferring a host file by version, omits the unused x64-only helper shipped inside the current ARM64 redistributable, and rejects packaged CRT files for the wrong processor architecture. Startup logging records each workspace and engine phase, request duration, engine exit, and bounded engine diagnostics. A handled WinUI startup exception leaves an error page open with the log location.

## Automated verification

- Rust sync engine: 109 tests passed, including runtime variation over exact schema profiles, unknown-profile refusal, trigger and index drift, migration failure, snapshot-source validation, and cross-schema transfer rules.
- Frontend: 24 tests passed, and the production TypeScript build completed.
- Native engine client: 13 contract checks passed with disposable data.
- Native WinUI Release build: passed with zero warnings.
- Rust Clippy with warnings denied: passed.
- Windows packaging validates the app, engine, and required CRT architecture for x64 and ARM64. Each native runner then installs the final NSIS package, activates the app offscreen with a disposable profile, completes workspace initialization, checks every page constructor, verifies the installed engine contract, and uninstalls the package.
- The final release workflow builds Windows x64, Windows ARM64, macOS Apple Silicon, and macOS Intel packages from the same tagged source.
- The [four-platform 1.5.2 release workflow](https://github.com/desigrit/project-spice-route/actions/runs/35457842725) completed successfully. The published installers, disk images, and application archives were downloaded again and matched all six native-runner SHA-256 checksums.

## Physical-device check

Install 1.5.2 on the Windows ARM64 computer and the Mac. On Windows, confirm that workspace loading reaches Overview or leaves the in-app startup error visible. If an environment-specific failure remains, attach `%LOCALAPPDATA%\com.spiceroute.codexsync\logs\startup.log`; it now identifies the stage and engine request that failed. On macOS, refresh Overview and confirm that the validated 54/6 or 55/6 database profile enables Push and Pull even when the Codex runtime differs from its recorded reference build.

No personal profile, Push, Pull, or interactive app launch is part of the local automated verification.
