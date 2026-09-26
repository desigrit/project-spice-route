# README screenshots

The `spice-route-*-explained.png` images show the current Push and Pull route, the selected Codex database tables, and the project transfer modes. They are renders of [the three-view visual guide](../spice-route-sync-explained.html), based on the Rust engine. They contain no personal data and do not represent a live transfer.

The `native-*` images show the actual Spice Route 1.6.0 WinUI interface with fabricated projects, chats, device names, and locations. They come from the offscreen visual probe documented in [Windows verification](../testing-1.6.0.md). They illustrate the interface rather than a live cloud transfer. Native operating-system caption buttons are outside the captured XAML tree.

The README includes all five current screens:

| README image | Offscreen probe capture |
| --- | --- |
| `native-overview-light.png` | `light-wide-overview.png` |
| `native-selection-dark.png` | `dark-wide-selection.png` |
| `native-settings-light.png` | `light-wide-settings.png` |
| `native-review-light.png` | `light-wide-review.png` |
| `native-diagnostics-light.png` | `light-wide-diagnostics.png` |

The images without the `native-` prefix show the React/Tauri interface. To render the current Mac pages headlessly, install Playwright separately from the app dependencies:

```powershell
npm install --no-save --package-lock=false playwright
npx playwright install chromium
node scripts/render-previews.mjs
node scripts/check-selection.mjs
node docs/render-sync-explained.mjs
```

These scripts run headlessly. They do not open the installed app, access a personal Codex profile, or perform a Push or Pull. The preview scripts write to the ignored `docs/design` folder; the sync guide renderer updates the three explanatory images in this directory.

Optional environment variables:

- `SPICE_PLAYWRIGHT_PATH`: absolute path to an existing Playwright module entry point.
- `SPICE_CHROME_PATH`: absolute path to an existing Chromium or Chrome executable.
- `SPICE_PREVIEW_PLATFORM`: use `macos` for the Mac treatment.
- `SPICE_PREVIEW_PAGES`: comma-separated pages to render, such as `overview,selection,recovery`.

The older Tauri images come from `ui-overview-light-1180.png`, `ui-selection-dark-1180.png`, and `ui-recovery-light-1180.png`. Inspect regenerated output before replacing them.
