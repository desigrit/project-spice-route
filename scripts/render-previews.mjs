// Offline visual QA only: SSR renders component markup with fabricated sample data.
// No Tauri calls, hydration, app server, personal profile, or live Codex data.
import fs from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { createDOMRenderer, RendererProvider, renderToStyleElements } from '@griffel/react';
import { Button } from '@fluentui/react-components';
import { Route, FolderCode, ArchiveRestore, Settings, Laptop, RefreshCw } from 'lucide-react';
import { createServer } from 'vite';
import react from '@vitejs/plugin-react';

const root = path.resolve(import.meta.dirname, '..');
const output = path.join(root, 'docs/design');
const playwrightPath = process.env.SPICE_PLAYWRIGHT_PATH;
const chromePath = process.env.SPICE_CHROME_PATH;
const platform = process.env.SPICE_PREVIEW_PLATFORM === 'macos' ? 'macos' : 'windows';
const { chromium } = await import(playwrightPath ? pathToFileURL(playwrightPath).href : 'playwright');
const h = React.createElement;
const noop = () => {};
const availablePages = ['overview', 'selection', 'settings', 'recovery', 'onboarding', 'mapping'];
const pages = process.env.SPICE_PREVIEW_PAGES?.split(',').map((page) => page.trim()).filter(Boolean) || availablePages;
if (pages.some((page) => !availablePages.includes(page))) throw new Error(`Unknown preview page. Choose from ${availablePages.join(', ')}.`);

const config = {
  schemaVersion: 1, deviceId: 'preview-device', deviceName: 'Surface Studio',
  codexHome: 'D:\\Codex\\.codex', projectlessRoot: 'D:\\Codex\\sessions', projectsRoot: '',
  cloudRoot: 'C:\\Users\\Alex\\OneDrive\\Spice Route', cloudProvider: 'oneDrive',
  theme: 'light', onboardingComplete: true, destinationRoots: {}, sourceRoots: {},
  selection: { revision: 'preview', defaultProjectMode: 'full', projectModes: { notes: 'historyOnly' },
    excludedThreadIds: [], includeArchived: true, includeBuildOutputs: false,
    includeSensitiveFiles: false, extraExcludePatterns: [] },
};
const environment = {
  codexHome: config.codexHome, codexHomeResolved: config.codexHome,
  codexExecutable: null, codexVersion: '0.153.4', codexRunning: false,
  cloudCandidates: [{ provider: 'oneDrive', path: 'C:\\Users\\Alex\\OneDrive', label: 'OneDrive' }],
  compatibility: { supported: true, adapter: 'Codex 0.153.4', stateMigration: 52,
    historyMigration: 6, schemaFingerprint: 'preview-only',
    explanation: 'This Codex format is supported by the current adapter.' }, warnings: [],
};
const projects = [
  { id: 'spice', name: 'Spice Route', roots: ['D:\\Code\\Spice Route'], localRoots: ['D:\\Code\\Spice Route'],
    threadCount: 8, estimatedBytes: 46137344, gitRepository: true, linkedWorktree: false },
  { id: 'atlas', name: 'Atlas', roots: ['E:\\Projects\\Atlas', 'E:\\Worktrees\\Atlas-design'],
    localRoots: ['E:\\Projects\\Atlas', 'E:\\Worktrees\\Atlas-design'], threadCount: 12,
    estimatedBytes: 188743680, gitRepository: true, linkedWorktree: true },
  { id: 'notes', name: 'Travel notes', roots: ['C:\\Users\\Alex\\Documents\\Travel'],
    localRoots: ['C:\\Users\\Alex\\Documents\\Travel'], threadCount: 4,
    estimatedBytes: 3145728, gitRepository: false, linkedWorktree: false },
];
const threads = Array.from({ length: 28 }, (_, index) => ({
  id: `preview-${index}`, title: index < 8 ? 'Refine desktop handoff' : 'Plan the next update',
  preview: 'A sample conversation for visual review.', cwd: config.projectlessRoot,
  projectId: index < 8 ? 'spice' : index < 20 ? 'atlas' : index < 24 ? 'notes' : null,
  archived: index === 27, updatedAtMs: 1789160400000, estimatedBytes: 65536, projectless: index >= 24,
}));
const catalog = { projects, threads, totalEstimatedBytes: 241172480, warnings: [] };
const snapshot = {
  id: 'preview-snapshot', shortId: 'A7C9E24B', deviceId: 'other-device', deviceName: 'Travel laptop',
  createdAt: '2026-09-11T13:40:00Z', parentId: null, logicalBytes: 241172480,
  storedBytes: 114294784, objectCount: 1248, verified: false, clientSyncState: 'unknown',
};
const status = { latestSnapshot: snapshot, visibleHeads: [snapshot], lastAppliedSnapshotId: snapshot.id,
  lastPushedSnapshotId: null, cloudBytes: 467664896, incomingAvailable: false, mergeReady: false,
  pendingRecovery: false, state: 'ready', message: 'Your next device is one handoff away.' };
const recoveries = [{ id: 'recovery-preview', createdAt: '2026-09-11T13:38:00Z',
  reason: 'Before pulling from Travel laptop', sourceSnapshotId: snapshot.id,
  status: 'available', sizeBytes: 49283072 }];
const mappingPreview = {
  operationId: 'preview-mapping', direction: 'pull', snapshotId: snapshot.id,
  changes: [{ key: 'project:atlas', kind: 'project', action: 'add', label: 'Atlas',
    detail: 'Restore project history and its linked workspace.', bytes: 188743680 }],
  warnings: [], blockedReasons: [], estimatedBytes: 188743680, requiresCodexClose: true,
  requiredMappings: [
    { projectId: 'atlas', rootIndex: 0, projectName: 'Atlas · repository',
      sourcePath: 'C:\\Users\\Alex\\Projects\\Atlas', suggestedPath: 'E:\\Projects\\Atlas' },
    { projectId: 'atlas', rootIndex: 1, projectName: 'Atlas · design worktree',
      sourcePath: 'C:\\Users\\Alex\\Worktrees\\Atlas-design', suggestedPath: 'E:\\Worktrees\\Atlas-design' },
  ],
};
const nav = [ ['overview', 'Overview', Route], ['selection', 'What to sync', FolderCode],
  ['recovery', 'Recovery', ArchiveRestore], ['settings', 'Settings', Settings] ];

const css = await fs.readFile(path.join(root, 'src/styles.css'), 'utf8') + '\n'
  + await fs.readFile(path.join(root, 'src/interface.css'), 'utf8') + '\n'
  + await fs.readFile(path.join(root, 'src/macos.css'), 'utf8');
const boat = `data:image/png;base64,${(await fs.readFile(path.join(root, 'src/assets/boat-mark.png'))).toString('base64')}`;
await fs.mkdir(output, { recursive: true });
const vite = await createServer({ configFile: false, root, plugins: [react()],
  server: { middlewareMode: true, hmr: false, watch: null }, appType: 'custom', logLevel: 'error' });
let browser;
try {
  const { Overview, SelectionScreen, SettingsScreen, RecoveryScreen, Onboarding, PreviewDialog } = await vite.ssrLoadModule('/src/App.tsx');
  const { AppTheme } = await vite.ssrLoadModule('/src/fluent-theme.tsx');
  // Effects never run with renderToStaticMarkup. Only the theme state initializer needs this.
  globalThis.window = { matchMedia: () => ({ matches: false }) };
  const shell = (page, content, overlay) => h('div', { className: 'app-shell' },
    h('aside', { className: 'sidebar', 'aria-label': 'Main navigation' },
      h('div', { className: 'brand' }, h('img', { className: 'brand-logo', src: boat, width: 38, height: 38, alt: '' }),
        h('span', null, h('strong', null, 'Spice Route'), h('small', null, 'Codex handoff'))),
      h('nav', null, nav.map(([id, label, Icon]) => h(Button, { key: id, className: page === id ? 'nav-item active' : 'nav-item',
        'aria-current': page === id ? 'page' : undefined }, h(Icon, { size: 18 }), h('span', null, label)))),
      h('div', { className: 'sidebar-footer' }, h('div', { className: 'device-chip' }, h(Laptop, { size: 16 }),
        h('span', null, h('small', null, 'This device'), config.deviceName)),
        h('div', { className: 'sidebar-version' }, 'Spice Route 1.5.1'))),
    h('main', { className: 'main-content' }, h('header', { className: 'topbar' },
      h('div', null, h('p', { className: 'eyebrow' }, page === 'selection' ? 'Sync policy' : page),
        h('h1', null, nav.find(([id]) => id === page)?.[1] || 'Overview')),
      h(Button, { className: 'icon-button', 'aria-label': 'Refresh' }, h(RefreshCw, { size: 18 }))),
      page === 'settings' ? h('div', { className: 'save-confirmation', role: 'status' }, 'Settings saved.') : null, content), overlay);

  browser = await chromium.launch({ executablePath: chromePath, headless: true });
  const report = [];
  for (const theme of ['light', 'dark']) {
    for (const page of pages) {
      const renderer = createDOMRenderer();
      const themedConfig = { ...config, theme };
      const overview = h(Overview, { config: themedConfig, environment, catalog, status, onPush: noop, onPull: noop, onOpenCodex: noop });
      const content = page === 'selection' ? h(SelectionScreen, { config: themedConfig, catalog, onSave: noop })
        : page === 'settings' ? h(SettingsScreen, { config: themedConfig, environment, onSave: noop, onResetCloudHistory: noop })
        : page === 'recovery' ? h(RecoveryScreen, { recoveries, onRestore: noop, onDiagnose: noop }) : overview;
      const overlay = page === 'onboarding' ? h(Onboarding, { config: { ...themedConfig, onboardingComplete: false }, environment, onComplete: noop })
        : page === 'mapping' ? h(PreviewDialog, { preview: mappingPreview, onCancel: noop, onExecute: noop, onSaveMappings: noop }) : null;
      const markup = renderToStaticMarkup(h(RendererProvider, { renderer }, h(AppTheme, { mode: theme }, shell(overlay ? 'overview' : page, content, overlay))));
      const styles = renderToStaticMarkup(h(React.Fragment, null, ...renderToStyleElements(renderer)));
      const html = `<!doctype html><html lang="en" data-theme="${theme}" data-platform="${platform}"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Spice Route | static ${page} preview</title>${styles}<style>${css}</style></head><body><div id="root">${markup.replaceAll('/src/assets/boat-mark.png', boat)}</div></body></html>`;
      await fs.writeFile(path.join(output, `ui-${page}-${theme}.html`), html);
      for (const viewport of [{ width: 1180, height: 820 }, { width: 920, height: 620 }]) {
        const context = await browser.newContext({ viewport, deviceScaleFactor: 1 });
        const tab = await context.newPage();
        await tab.route('**/*', (route) => route.abort());
        await tab.setContent(html, { waitUntil: 'load' });
        await tab.evaluate(() => document.fonts.ready);
        const stem = platform === 'macos' ? `ui-macos-${page}-${theme}-${viewport.width}` : `ui-${page}-${theme}-${viewport.width}`;
        await tab.screenshot({ path: path.join(output, `${stem}.png`) });
        const overflow = await tab.evaluate(() => ({
          viewport: { width: innerWidth, height: innerHeight },
          documentOverflow: document.documentElement.scrollWidth > innerWidth,
          containers: [...document.querySelectorAll('.main-content, .panel, .onboarding-card, .preview-dialog, .mapping-list, .selection-toolbar')]
            .filter((element) => element.scrollWidth > element.clientWidth + 2)
            .map((element) => ({ className: element.className, clientWidth: element.clientWidth, scrollWidth: element.scrollWidth })),
          dialogBounds: (() => { const dialog = document.querySelector('.onboarding-card, .preview-dialog'); if (!dialog) return null;
            const rect = dialog.getBoundingClientRect(); return { top: rect.top, bottom: rect.bottom, height: rect.height,
              clientHeight: dialog.clientHeight, scrollHeight: dialog.scrollHeight }; })(),
        }));
        report.push({ platform, page, theme, width: viewport.width, ...overflow });
        if (page === 'selection' || page === 'settings') {
          await tab.evaluate(() => { const main = document.querySelector('.main-content'); main.scrollTop = main.scrollHeight; });
          await tab.screenshot({ path: path.join(output, `${stem}-bottom.png`) });
        }
        if (page === 'mapping') {
          await tab.evaluate(() => { const dialog = document.querySelector('.preview-dialog'); dialog.scrollTop = dialog.scrollHeight; });
          await tab.screenshot({ path: path.join(output, `${stem}-bottom.png`) });
        }
        await context.close();
      }
    }
  }
  const previous = await fs.readFile(path.join(output, 'ui-layout-report.json'), 'utf8').then(JSON.parse).catch(() => []);
  const retained = previous.filter((item) => !report.some((next) => (item.platform || 'windows') === next.platform && item.page === next.page && item.theme === next.theme && item.width === next.width));
  await fs.writeFile(path.join(output, 'ui-layout-report.json'), JSON.stringify([...retained, ...report], null, 2));
  console.log(JSON.stringify(report, null, 2));
} finally {
  if (browser) await browser.close();
  await vite.close();
  delete globalThis.window;
}
