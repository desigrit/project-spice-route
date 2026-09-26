# Spice Route 1.6.3 verification

This update adds the Codex state/history 57/6 storage pair reported on macOS. The state and history layouts come from existing sanitized 57 and 6 schema fixtures. Spice Route accepts the pair only if the full combined fingerprint is `dac383732eddc28f1d2faca9a5b107ac23877db8dff8a6903bfd79b918a386c4`, required tables and triggers match, and both migration ledgers are complete. The Mac's actual fingerprint has not yet been supplied, so the reported migration numbers alone do not confirm that this build will unlock it.

Local checks completed without launching the app or accessing personal Codex data:

- Rust engine: 114 disposable-data tests passed.
- New 57/6 tests: Windows and macOS SQL line endings produce the same fingerprint; selected chat export, exclusion, repeated restore, and 57/6 to 57/7 to 57/6 transfer preserve history and unrelated local chats.
- A 57/6 destination refuses non-null history migration 7 lifecycle values before changing either database.
- A changed index under the 57/6 migration pair still blocks Push and Pull.
- Rust Clippy passed with warnings denied.
- Frontend: 28 tests passed and the production bundle built.

[Desktop build run 36271266284](https://github.com/desigrit/project-spice-route/actions/runs/36271266284) completed successfully for Windows x64, Windows ARM64, and macOS Apple Silicon. The Windows jobs ran their installer and startup probes on CI machines. The macOS job checked the Apple Silicon bundle, ad-hoc signature, and DMG. The published installers and DMG were checked against their CI SHA-256 files before being added to the repository.

| Package | SHA-256 |
|---|---|
| Windows x64 installer | `13555194780ecb6a1ef5ebec4fad1078454cb0eeb5b8541c08fe7eed5817e4bc` |
| Windows ARM64 installer | `794935b90a8ec6f4ee4132f9796dbd0623f89f880edfd7680d1f3aa9b6879247` |
| macOS Apple Silicon DMG | `078ddc983c0de640e3ba348bf6c774085befe4d2834e3409c57f69cd62577bde` |

Real-device acceptance remains: confirm the Mac fingerprint, Push from a disposable 57/6 Codex profile, Pull on 57/7, open and continue the chats, then repeat in the other direction. If the Mac fingerprint differs, export its compatibility report for a separate adapter update. Do not infer compatibility from the two migration numbers alone.
