// Headless interaction fixture. Mounts SelectionScreen only, with fabricated data.
// No app launch, native dialogs, profile access, server, or external network requests.
import fs from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { build } from 'vite';
import react from '@vitejs/plugin-react';

const root = path.resolve(import.meta.dirname, '..');
const output = path.join(root, 'docs/design');
const source = root.replaceAll('\\', '/');
const entry = `
import React from 'react';
import { createRoot } from 'react-dom/client';
import { SelectionScreen } from '${source}/src/App.tsx';
import { AppTheme } from '${source}/src/fluent-theme.tsx';
const config = { schemaVersion:1, deviceId:'fixture', deviceName:'Fixture', codexHome:'',
  projectlessRoot:'', projectsRoot:'', cloudRoot:'', cloudProvider:'oneDrive', theme:window.QA_THEME,
  onboardingComplete:true, destinationRoots:{}, sourceRoots:{},
  selection:{revision:'fixture',defaultProjectMode:'full',projectModes:{},excludedThreadIds:[],
  includeArchived:true,includeBuildOutputs:false,includeSensitiveFiles:false,extraExcludePatterns:[]} };
const catalog = { warnings:[],totalEstimatedBytes:10485760,
  projects:[{id:'sample',name:'Sample project',roots:['D:/Code/Sample'],localRoots:['D:/Code/Sample'],
  threadCount:1,estimatedBytes:10485760,gitRepository:true,linkedWorktree:false}],
  threads:[{id:'chat',title:'Sample conversation',preview:'',cwd:'D:/Code/Sample',projectId:'sample',
  archived:false,updatedAtMs:0,estimatedBytes:65536,projectless:false}] };
createRoot(document.getElementById('root')).render(React.createElement(AppTheme,{mode:window.QA_THEME},
  React.createElement('main',{style:{padding:'32px',height:'100vh'}},
  React.createElement('h1',null,'What to sync'),
  React.createElement(SelectionScreen,{config,catalog,onSave:()=>{}}))));`;
const built = await build({ configFile:false, root, logLevel:'error', plugins:[react(),{
  name:'isolated-selection-fixture',
  resolveId(id) {
    if (id.replaceAll('\\', '/').endsWith('/spice-qa-entry') || id === 'spice-qa-entry') return '\0spice-qa-entry';
    if (id === '@tauri-apps/plugin-dialog') return '\0' + id;
  },
  load(id) {
    if (id === '\0spice-qa-entry') return entry;
    if (id === '\0@tauri-apps/plugin-dialog') return 'export async function open(){return null;}';
  },
}], define:{'process.env.NODE_ENV':'"production"'}, build:{write:false,minify:true,lib:{entry:'spice-qa-entry',name:'SpiceQA',formats:['iife']}} });
const script = (Array.isArray(built) ? built[0] : built).output.find(item => item.type === 'chunk').code;
const css = await fs.readFile(path.join(root,'src/styles.css'),'utf8') + '\n' + await fs.readFile(path.join(root,'src/interface.css'),'utf8');
const { chromium } = await import(process.env.SPICE_PLAYWRIGHT_PATH ? pathToFileURL(process.env.SPICE_PLAYWRIGHT_PATH).href : 'playwright');
const browser = await chromium.launch({executablePath:process.env.SPICE_CHROME_PATH,headless:true});
const report = [];
try {
  for (const theme of ['light','dark']) {
    const page = await browser.newPage({viewport:{width:920,height:620}});
    page.setDefaultTimeout(8000);
    const errors = [];
    page.on('pageerror', error => { errors.push(error.message); console.error(error.message); });
    await page.route('**/*', route => route.request().url() === 'https://spice-route.test/'
      ? route.fulfill({contentType:'text/html',body:`<!doctype html><html lang="en" data-theme="${theme}"><head><meta charset="utf-8"><style>${css}</style></head><body><div id="root"></div></body></html>`})
      : route.abort());
    await page.goto('https://spice-route.test/');
    await page.evaluate(value => { window.QA_THEME = value; }, theme);
    await page.addScriptTag({content:script});
    const mode = page.getByRole('combobox',{name:'Sync mode for Sample project'});
    await mode.focus();
    await mode.press('ArrowDown');
    await page.getByRole('option',{name:'Chat history only'}).waitFor();
    await page.screenshot({path:path.join(output,`ui-mode-menu-${theme}.png`),animations:'disabled'});
    await page.getByRole('option',{name:'Chat history only'}).click();
    const estimate = page.getByLabel('Estimated sync size for Sample project');
    if (!(await estimate.innerText()).includes('64 KB')) throw new Error('History estimate did not update.');
    await mode.click();
    await page.getByRole('option',{name:'Full project',exact:true}).click();
    const folder = page.getByRole('button',{name:'Folder options: Local folder for Sample project'});
    await folder.focus();
    await folder.press('Enter');
    await page.getByRole('menuitem',{name:'Change folder…'}).waitFor();
    await page.screenshot({path:path.join(output,`ui-folder-menu-${theme}.png`),animations:'disabled'});
    await page.keyboard.press('Escape');
    if (!(await folder.evaluate(node => node === document.activeElement))) throw new Error('Folder menu did not return keyboard focus.');
    if (errors.length) throw new Error(errors.join('\n'));
    report.push({theme,keyboardMenus:true,historyBytes:65536,escapeRestoresFocus:true,pageErrors:errors});
    await page.close();
  }
  await fs.writeFile(path.join(output,'ui-interaction-report.json'),JSON.stringify(report,null,2));
  console.log(JSON.stringify(report));
} finally { await browser.close(); }
