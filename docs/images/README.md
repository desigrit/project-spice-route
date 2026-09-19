# README screenshots

The `native-*` images show the actual Spice Route 1.4.4 WinUI interface with fabricated projects, chats, device names, and locations. They come from the offscreen visual probe documented in [Windows verification](../testing-1.4.4.md). They illustrate the interface rather than a live cloud transfer. Native operating-system caption buttons are outside the captured XAML tree.

The README includes all five current screens:

| README image | Offscreen probe capture |
| --- | --- |
| `native-overview-light.png` | `light-wide-overview.png` |
| `native-selection-dark.png` | `dark-wide-selection.png` |
| `native-settings-light.png` | `light-wide-settings.png` |
| `native-review-light.png` | `light-wide-review.png` |
| `native-diagnostics-light.png` | `light-wide-diagnostics.png` |

The older images without the `native-` prefix show the earlier Tauri interface. To regenerate those legacy images, install Playwright separately from the app dependencies:

```powershell
npm install --no-save --package-lock=false playwright
npx playwright install chromium
node scripts/render-previews.mjs
node scripts/check-selection.mjs
```

Both scripts run headlessly. They do not open the installed app, access a personal Codex profile, or perform a Push or Pull. Output is written to the ignored `docs/design` folder.

Optional environment variables:

- `SPICE_PLAYWRIGHT_PATH`: absolute path to an existing Playwright module entry point.
- `SPICE_CHROME_PATH`: absolute path to an existing Chromium or Chrome executable.
- `SPICE_PREVIEW_PAGES`: comma-separated pages to render, such as `overview,selection,recovery`.

The older Tauri images come from `ui-overview-light-1180.png`, `ui-selection-dark-1180.png`, and `ui-recovery-light-1180.png`. Inspect regenerated output before replacing them.
