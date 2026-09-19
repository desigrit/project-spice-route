# Validation and release gate

Spice Route 1.5.2 has native Windows x64 and ARM64 packages plus macOS Apple Silicon and Intel packages. Current checks cover the shared compatibility engine, selective history transfer, snapshot capture, conflicts, project reconstruction, diagnostics, recovery, and target-architecture validation for the Windows runtime payload. See the [1.5.2 desktop verification record](testing-1.5.2.md) for package evidence and remaining physical-device checks.

Version 0.3.1 passes 47 Rust tests, 19 frontend tests, release Clippy with warnings denied, TypeScript, and the Windows MSVC/NSIS build. It removes compression from previews and cloud hydration from status refresh, repairs conflict feedback and missing-baseline acknowledgment, and extends recovery protection. See [the 0.3.1 testing guide](testing-0.3.1.md). No live transfer or installed-app launch was performed for this patch.

The 0.3.0 compatibility engine passes 36 Rust tests and release Clippy with warnings denied. Both schema fixtures now use the actual reported database definitions, including triggers. The 52/54 matrix tests and preserved format-1 snapshot support are detailed in [the 0.3.0 handoff](testing-0.3.0.md). Non-null newer fields block a downgrade transfer; unsupported future versions remain blocked. Real-device history display and session continuation are still a manual gate.

The 0.2.1 interface update passes 11 frontend tests, 24 offline layout combinations, and live headless menu/size/focus checks in both themes. TypeScript, the production frontend, and the MSVC/NSIS installer build pass. Impeccable reports no findings in the changed UI files. The desktop app was not launched. See [the 0.2.1 testing guide](testing-0.2.1.md); the engine coverage and remaining manual release gates below are unchanged.

## Automated coverage

The Rust core suite covers:

- Stable snapshot hashes and rejection of unsafe relative paths.
- Junction-aware and case-insensitive path overlap behavior on Windows.
- Exact storage-profile gating, advisory runtime reporting, and fail-closed migration behavior.
- Selective task and project export, archived and projectless handling, individual exclusions, database-backed history, sidebar state, and continuation without duplicate task rows.
- Local-image attachment capture and destination path rewriting without changing historical text.
- Preservation of destination-only sessions, settings, queues, task permission fields, and agent paths.
- New imported tasks using destination-local permission defaults.
- Full, history-only, and excluded project behavior.
- Project-secret inclusion by default for new configurations, explicit exclusions, and build-output filtering.
- File capture mutation detection and safe cancellation.
- SHA-256 and zstd object round trips, deduplication, corrupt cloud-object repair, and manifest-last publication.
- Multi-head ancestry and merge convergence.
- Source deletion rules and selection changes that do not delete destination files.
- Durable rollback creation, integrity validation, cancellation, restoration, and cleanup of newly created targets.
- Git history, split-index normalization, staged-only objects, staged and unstaged changes to the same file, untracked files, and working-tree deletions.
- Explicit cloud cleanup confirmation while preserving the shared selection policy.

The 0.2.0 Rust suite passes all 28 tests with the MSVC toolchain. The frontend passes eight interaction tests: multi-head review/merge gating, independent project and worktree paths, resetting a source override, history-only/excluded folders, project-mode dropdowns, onboarding without a global project root, and the optional restore default. Release Clippy passes with warnings denied, the production frontend compiles, and Tauri successfully produces the NSIS installer. Native runtime interaction remains intentionally untested because the app was not launched; see [the 0.2 test handoff](testing-0.2.md).

Static visual previews use the actual React components, fabricated data, server-side rendering, and isolated headless Chrome. Overview, project selection, Settings, and onboarding were checked in light/dark at 1180×820 and 920×620 with no horizontal overflow; onboarding remains within the minimum viewport. These screenshots test layout only, without running Tauri, invoking Codex, or proving native Mica behavior. Previews and layout reports are under `docs/design`; regenerate with `scripts/render-previews.mjs` using a configured Playwright module and Chrome executable.

Recovery and the incoming two-root mapping dialog also pass the same light/dark and size checks. All 24 layout combinations have no horizontal overflow; the mapping dialog scrolls at the minimum size with its actions accessible. A Push-only source-folder guard rejects missing Full-project roots before capture and publication, while Pull still allows new/missing destinations. Its regression passes in the 28-test Rust suite.

## Manual provider matrix

These checks require real provider clients and two disposable Windows devices or virtual machines. They remain the release gate before using Spice Route as the only copy of important work.

| Scenario | Google Drive | OneDrive | iCloud Drive |
|---|---:|---:|---:|
| Snapshot objects arrive before manifest | Pending | Pending | Pending |
| Manifest arrives before one or more objects | Pending | Pending | Pending |
| Placeholder-only files hydrate and verify | Pending | Pending | Pending |
| Offline Push reports local publication accurately | Pending | Pending | Pending |
| Provider status wording is accurate | Pending | Pending | Pending |
| Corrupt or partial object is rejected | Pending | Pending | Pending |
| Windows A to B to A session continuation | Pending | Pending | Pending |

## Disposable Codex acceptance

Run each case on disposable profiles before adding another Codex build to the allowlist:

1. Create projectless, archived, full-project, history-only, and excluded tasks.
2. Include database-backed history, sidebar sections, image attachments, task exclusions, and destination-only local tasks.
3. Push from Windows A, wait for provider delivery, and Pull on Windows B with different usernames and drive letters.
4. Confirm every included task displays and can continue under the same identity without a duplicate. Confirm excluded records are absent from the snapshot.
5. Exercise a normal repository and linked worktrees with commits, branches, staged, unstaged, untracked, and deleted files.
6. Continue on B, Push, Pull on A, and repeat the identity and file-state checks.
7. Create divergent task and project edits and verify explicit version choice plus retained recovery data.
8. Interrupt Pull during staging and live application, exhaust destination space, reopen Codex during application, corrupt an object, and remove an object.
9. Confirm each case either stops before live mutation or exposes a verified rollback path.
10. Install on a clean Windows account with no separately installed Node.js or PowerShell scripts.

The release gate is complete only when session restoration is lossless, project transfer is recoverable, and all three provider rows above pass on physical clients.
