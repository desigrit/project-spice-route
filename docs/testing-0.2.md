# Spice Route 0.2 test handoff

## Build status

The native 0.2.0 Windows installer builds successfully with Visual Studio Build Tools 2022, MSVC, the Windows SDK, and Rust `stable-x86_64-pc-windows-msvc`. The build passed eight interface tests, 28 Rust engine tests, release Clippy with warnings denied, and Tauri's NSIS packaging. No app was launched and no live Codex data was changed during build validation.

The test installer is `artifacts/Spice-Route-0.2.0-x64-setup.exe`. Its SHA-256 is `BE76AA195923474929D3D33887CCAE4B407AD7B9A6856796CFB0306E835DF6F5`. It is not Authenticode-signed, so Windows SmartScreen or Smart App Control may require a signed build before it will run on a protected machine.

Source builds require Microsoft's **Build Tools for Visual Studio** with **Desktop development with C++**, including MSVC and a Windows SDK. Run these commands in this workspace:

```powershell
npm ci
npm test
npm run build
.\scripts\msvc-env.cmd cargo test --manifest-path src-tauri/core/Cargo.toml
.\scripts\msvc-env.cmd cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --release --target x86_64-pc-windows-msvc -- -D warnings
.\scripts\msvc-env.cmd npm run tauri build -- --target x86_64-pc-windows-msvc
```

Build output is a per-user Windows installer; end users need no Node.js or PowerShell scripts. Signing, native UI interaction, disposable-profile restoration, and physical cloud-client validation remain separate release gates.

## First UI test

1. Open the newly built **Spice Route 0.2.0** from the Start menu after installation.
2. Confirm task/history and projectless workspaces in setup. There is no required single Projects folder.
3. Complete device and cloud setup. Review each project under **What to sync**, including all roots for multi-root projects.
4. Choose one project on another drive, save, and reopen Settings / What to sync to confirm its folder is remembered. This choice does not move existing files or edit Codex directly.
5. Try **History only** and **Excluded**. Their code-folder inputs should disappear.
6. Test light, dark, and system themes, keyboard navigation, dropdowns, scrolling, and the window at its minimum size. Native Mica needs a supported Windows version and remains unverified until the app is opened manually.

## Sync test

Use disposable profiles and sample repositories for initial sync tests. Follow the full [acceptance matrix](validation.md); the 0.2 changes do not establish previously pending cross-device or cloud-provider acceptance.

- Before Push/Pull, save any active work. Sync deliberately waits until Codex writers are closed.
- First Pull asks for each incoming full project's destination, even if the incoming source path exists here. Map projects independently, including different drives, then re-review.
- After Pull, verify task identity, history, files, attachments, staged/unstaged changes, worktrees, and source-to-destination paths before continuing a task.
- Push from the receiving device and Pull back. Confirm that corrected local roots are used for capture and no duplicate project/task appears.
- Match the handoff ID after the provider finishes syncing. “Saved to sync folder” proves only local publication.
