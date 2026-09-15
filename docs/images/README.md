# README screenshots

These images show Spice Route 0.3.1 components rendered with fabricated projects, chats, device names, and locations. They illustrate the interface rather than a live cloud transfer. The static render does not include Windows title-bar decorations.

To regenerate the visual QA output, install Playwright separately from the app dependencies:

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

The curated images in this folder come from `ui-overview-light-1180.png`, `ui-selection-dark-1180.png`, and `ui-recovery-light-1180.png`. Inspect regenerated output before replacing them.
