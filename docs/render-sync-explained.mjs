import fs from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const docs = import.meta.dirname;
const { chromium } = process.env.SPICE_PLAYWRIGHT_PATH
  ? await import(pathToFileURL(path.resolve(process.env.SPICE_PLAYWRIGHT_PATH)).href)
  : await import('playwright');
const browser = await chromium.launch({
  headless: true,
  ...(process.env.SPICE_CHROME_PATH ? { executablePath: process.env.SPICE_CHROME_PATH } : {}),
});
const page = await browser.newPage({ viewport: { width: 1440, height: 930 }, deviceScaleFactor: 1 });
const errors = [];
page.on('pageerror', error => errors.push(error.message));
const source = pathToFileURL(path.join(docs, 'spice-route-sync-explained.html')).href;
const output = path.join(docs, 'images');
await fs.mkdir(output, { recursive: true });

for (const view of ['route', 'schema', 'choices']) {
  await page.goto(`${source}?view=${view}&theme=light`);
  await page.screenshot({ path: path.join(output, `spice-route-${view}-explained.png`), fullPage: true });
}

for (const width of [1440, 1024, 760, 390]) {
  await page.setViewportSize({ width, height: 930 });
  for (const view of ['route', 'schema', 'choices']) {
    await page.goto(`${source}?view=${view}&theme=light`);
    const result = await page.evaluate(() => ({
      horizontalOverflow: document.documentElement.scrollWidth > window.innerWidth + 1,
      selectedTab: document.querySelector('[role="tab"][aria-selected="true"]')?.id,
      visiblePanel: [...document.querySelectorAll('[role="tabpanel"]')].filter(panel => !panel.hidden).length,
    }));
    if (result.horizontalOverflow || result.selectedTab !== `tab-${view}` || result.visiblePanel !== 1) {
      errors.push(`${width}px ${view}: ${JSON.stringify(result)}`);
    }
  }
}
await page.setViewportSize({ width: 1440, height: 930 });
await page.goto(`${source}?view=route&theme=light`);
await page.keyboard.press('Tab');
await page.locator('#tab-route').focus();
await page.keyboard.press('ArrowRight');
if (await page.locator('#tab-schema').getAttribute('aria-selected') !== 'true') {
  errors.push('Keyboard tab navigation failed');
}
await browser.close();
if (errors.length) throw new Error(errors.join('\n'));
console.log('Rendered all three views and checked responsive layout and keyboard navigation.');
