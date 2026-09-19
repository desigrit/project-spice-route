# Spice Route 1.5.1 verification

## Compatibility correction

Version 1.5.1 fixes a false compatibility block for Codex databases created on macOS. SQLite preserves the line endings used by migration SQL in `sqlite_master`. Equivalent schema 54 databases therefore produced different raw hashes on Windows and macOS.

The adapter now normalizes CRLF, LF, and CR line endings before calculating the schema fingerprint. It continues to compare the complete normalized layout, including tables, indexes, and triggers, and it still requires successful migrations and a tested Codex runtime pair. Existing Windows snapshot fingerprints from version 1.5.0 remain recognized.

The reported macOS fingerprint `8082e27f46c7a5691ae4dca5b004ce2103bacaafb3989f7be95607d34685c205` is the canonical schema 54 fingerprint.

## Automated verification

- Rust sync engine: 109 tests passed.
- Frontend: 24 tests passed.
- Frontend production build: passed.
- Rust Clippy with warnings denied: passed.
- Release version metadata validation: passed for 1.5.1.
- [Four-platform desktop build](https://github.com/desigrit/project-spice-route/actions/runs/35435536698): passed for Windows x64, Windows ARM64, macOS Apple Silicon, and macOS Intel.
- Published package checksums: verified after downloading every workflow artifact.

Regression coverage verifies all tested schema versions across Windows and macOS line endings. It also verifies legacy snapshot recognition, failed migration rejection, trigger drift rejection, index drift rejection, and unknown column rejection.

## Manual acceptance

Install version 1.5.1 on every participating computer. On the affected Mac, refresh Spice Route after installation and confirm that compatibility changes from blocked to supported. A different message about the Codex runtime means the schema has passed and the separately detected runtime is outside the tested pair.

No personal profile, Push, Pull, or interactive app launch was used during automated verification.
