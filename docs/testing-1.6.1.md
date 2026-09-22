# Spice Route 1.6.1 verification

This update refines the Overview, What to sync, Recovery, and Settings pages. It also lets one Codex project include more than one local code folder.

## Verified in source and disposable profiles

- 111 shared Rust engine tests pass. A new fixture gives one Kaptus project an Android root and an added Kaptus-iOS root, then confirms that both codebases enter one snapshot while the source Codex database remains untouched. Another regression check confirms that removing an added root does not delete its existing destination files.
- 28 React interaction tests pass, including a check that changing the default mode preserves an existing project's mode.
- The React production build, WinUI x64 Release build, Rust formatting, and Rust lint pass.
- The Windows x64 and ARM64 installers passed CI startup probes and installed-package checks on disposable profiles. Neither app was launched on the development PC. The Apple Silicon package passed its CI build, source tests, and checksum verification.
- Mode-aware size estimation now walks working files only for Full projects. Switching a project to Full requests a fresh estimate.

## Before a personal handoff

1. In Settings, choose the folder that contains project codebases as **Project discovery and restores**. On RAUNAK-PC, this is `D:\Code`.
2. Open What to sync and select its refresh icon. Search for `Kaptus-iOS`; the Kaptus project should appear.
3. Select Kaptus and choose **Add Kaptus-iOS**. Its details should show both `D:\Code\AndroidStudioProjects\Kaptus` and `D:\Code\Kaptus-iOS`. Choose **Full project** if the code should travel.
4. Save choices, review a Push, and confirm that both roots and the revised selected size appear before publishing.
5. On the receiving device, map each folder to the correct local location during Pull. Check both codebases and the Kaptus project listing in Codex afterward.

Also check the narrow-window navigation, the green or amber device status, the consistent refresh icons, and the always-visible Settings controls. Changing **New projects** in Settings should leave every currently listed project's mode unchanged.

Real cloud delivery, Codex continuation after Pull, and the Windows ARM64 and macOS UI still need hands-on device verification. No personal Push or Pull was run during this build.
