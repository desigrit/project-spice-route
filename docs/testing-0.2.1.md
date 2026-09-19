# Spice Route 0.2.1: interface test build

This update uses the supplied ship reference, simplified into a navy and ivory icon. Light and dark themes share that palette. The canonical icon source is `app-icon.png`; native assets are generated with `npm run tauri icon app-icon.png -- --output src-tauri/icons`. The sidebar uses the generated 256px image.

What to sync now uses compact rows, rounded mode menus, and a folder name with an **…** menu instead of path forms. Advanced file exclusions are collapsed. Overview, Settings, and Recovery use open sections and separators instead of enclosing cards.

## Check when convenient

1. Install `artifacts/Spice-Route-0.2.1-x64-setup.exe`. No Node.js or development server is needed.
2. In What to sync, open a project's **…** menu and choose **Change folder…**. Confirm or cancel the native folder picker. Save choices to persist changes; **Use Codex location** resets an existing override. Repeat for a linked worktree if available.
3. Switch a project from **Full project** to **Chat history only** and then **Excluded**. The row estimate and total should update immediately; history mode hides working folders, and exclusion shows zero bytes.
4. Try chat exclusions and the archived-chat setting, then return to Projects to check the estimates. Values are estimates from discovered files/history; the Push preview applies the engine's file filters and accounts for transfer content.
5. Switch between light and dark in Settings. Check navigation, open menus, keyboard focus, search, and the minimum window size.

## Validation and limits

- Eleven frontend tests pass, covering folder mapping and cancellation, mode changes, history/exclusion estimates, onboarding, settings, and concurrent handoff controls.
- Offline headless renders cover all six screen/dialog fixtures in both themes at 1180px and 920px. They use fabricated data and check horizontal overflow.
- An isolated headless SelectionScreen fixture also checks live dropdown and folder-menu interactions in both themes, history estimates, and keyboard focus restoration. Run `node scripts/check-selection.mjs`; its native folder dialog is mocked and external network requests are blocked. This caught and guards against the Fluent portal inheriting a full-window background and intercepting clicks.
- Impeccable's manual detector reports no findings in the changed interface files.
- The desktop app was not launched, and live Codex data was not changed. Native picker appearance and installed-app interaction remain manual checks.
- This interface update does not broaden Codex compatibility or change the sync engine. Existing restoration gates and cloud-provider acceptance limitations remain in effect.
- The test installer is unsigned.
- TypeScript, the production frontend build, and the Windows MSVC/NSIS package build pass. Vite reports a non-blocking bundle-size advisory for the Fluent UI bundle.
