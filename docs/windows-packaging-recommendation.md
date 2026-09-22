# Windows packaging recommendation

September 22, 2026. Recommendation for the next integration milestone, not an implemented packaging change.

Spice Route currently uses a self-contained WinUI 3 app and an NSIS installer. Its project declares WindowsPackageType=None. A sparse package, now documented as a package with external location, can add Windows package identity while keeping that installer and the existing binary location. [Microsoft overview](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/grant-identity-to-nonpackaged-apps-overview)

## Recommendation

Use sparse packaging if the release adds an integration that benefits from identity. The strongest initial candidate is an optional File Explorer command, Add to Spice Route, that opens the app to review a selected project folder. This command should never start Push or Pull by itself. Microsoft's modern Explorer extension guidance supports this approach for unpackaged apps given identity through a sparse package. [Explorer integration](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/integrate-packaged-app-with-file-explorer)

Local transfer-completion notifications are useful, but they are not a reason to require sparse packaging: the Windows App SDK AppNotificationManager supports packaged and unpackaged apps. Use that API for a short notification that opens the relevant review or recovery page. [Notification API comparison](https://learn.microsoft.com/en-us/windows/apps/develop/notifications/)

Sparse packaging does not make the interface more native, speed up hashing or transfer, confirm cloud delivery, or fix database compatibility. Those remain application responsibilities.

## Deployment choices

| Option | Fit for Spice Route |
| --- | --- |
| Current NSIS installer | Lowest deployment change; keep for the UX evaluation. Local notifications remain possible. |
| NSIS plus sparse identity package | Add identity-based shell integration while retaining the current file layout and installer. |
| Full MSIX | Reconsider when Store distribution and managed updates are priorities. This changes the deployment model more substantially. |

A sparse identity package does not provide full MSIX's installation ownership or update system. Our installer still owns application payload replacement, updates, and cleanup. [Microsoft distribution guidance](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/choose-distribution-path)

## Requirements before shipping

- Choose a stable package name and publisher. Keep them consistent across x64 and ARM64.
- Sign the identity package with a certificate trusted on the destination PC. For public distribution, use production signing. A self-signed development certificate requires separate trust setup.
- Add matching identity metadata to the app executable, then register the identity package against the per-user installation directory.
- Account for the Windows 10 build 19041 minimum for external-location identity.
- Keep architecture-specific application payloads even if the identity package is architecture-neutral.

These requirements come from Microsoft's manual identity-package instructions. [Manifest, signing, and registration](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/grant-identity-to-nonpackaged-apps)

The following are Spice Route-specific acceptance checks, not guarantees supplied by packaging:

1. Clean install, upgrade, repair, and uninstall on x64 and ARM64.
2. No duplicate Start entries, taskbar identities, or stale registration after an upgrade.
3. Explorer activation opens the existing app instance, validates the path, and goes to a reviewable selection.
4. Registration or shell-extension failure never deletes user configuration, history, or recovery data.
5. No silent certificate installation, visible console flashes, or mandatory background sync.
6. Validate startup of the installed, signed package, then verify it with the actual sync engine.

The decision needed before implementation is which integration to ship and which publisher/signing identity to use. The UX redesign can proceed independently.