import fs from 'node:fs/promises';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const base=import.meta.dirname;
const output=path.resolve(base,'../design/ux-2026-09');
await fs.mkdir(output,{recursive:true});
const {chromium}=await import(pathToFileURL(process.env.SPICE_PLAYWRIGHT_PATH||'C:/Users/rauna/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright/index.mjs').href);
const browser=await chromium.launch({headless:true,executablePath:'C:/Program Files/Google/Chrome/Application/chrome.exe'});
const page=await browser.newPage({viewport:{width:1280,height:850},deviceScaleFactor:1});
const errors=[],report=[];
page.on('pageerror',e=>errors.push(e.message));
const url=pathToFileURL(path.join(base,'preview.html')).href;
for(const direction of ['a','b','c']){
  for(const width of [1280,980]){
    for(const theme of ['light','dark']){
      for(const view of ['overview','selection','review','settings','recovery']){
        await page.setViewportSize({width,height:width===1280?850:760});
        await page.goto(url+'?capture&direction='+direction+'&page='+view+'&theme='+theme);
        await page.waitForSelector('.page-head');
        const scan=await page.evaluate(()=>{
          const main=document.querySelector('.main');
          const over=[...document.querySelectorAll('.main,.body,.review-body,.workbench,.toolbar,.pair-pane,.project-item,.page-footer,.settings-two,.inspector')]
            .filter(e=>e.getClientRects().length&&e.scrollWidth>e.clientWidth+2).map(e=>({class:e.className,scroll:e.scrollWidth,client:e.clientWidth}));
          const unnamed=[...document.querySelectorAll('button,input,select')].filter(e=>e.getClientRects().length&&!e.textContent.trim()&&!e.getAttribute('aria-label')&&!e.closest('label')).map(e=>e.outerHTML.slice(0,200));
          return {overflow:over,unnamed,mainWidth:main.clientWidth,bodyOverflow:document.documentElement.scrollWidth>innerWidth};
        });
        report.push({direction,width,theme,view,...scan});
        if(width===1280&&(theme==='light'||view==='overview'||view==='selection')||width===980&&theme==='light'&&['overview','selection'].includes(view))
          await page.screenshot({path:path.join(output,direction+'-'+view+'-'+theme+'-'+width+'.png')});
      }
    }
  }
}
await page.setViewportSize({width:1280,height:850});
for(const direction of ['a','b','c']){
 await page.goto(url+'?capture&direction='+direction+'&page=overview&platform=mac');
 await page.screenshot({path:path.join(output,direction+'-macos.png')});
}
await page.goto(url+'?capture&direction=b&page=review&state=blocked&transfer=pull');
await page.click('[data-tab=attention]');
await page.screenshot({path:path.join(output,'b-attention.png')});
await page.click('[data-choice=incoming]');
await page.click('[data-action=map-folder]');
await page.click('[data-picked-folder]');
report.push({interaction:'conflict and folder choices enable Pull',passed:await page.locator('[data-action=execute]').isEnabled()});
await page.goto(url+'?capture&direction=b&page=selection');
await page.click('[data-menu="mode:spice"]');
await page.screenshot({path:path.join(output,'b-mode-menu.png')});
await page.click('[data-mode=full]');
report.push({interaction:'Full project increases selected size',passed:await page.locator('.subtitle').textContent().then(s=>s.includes('995 MB'))});
await page.click('[data-menu="mode:spice"]');
await page.click('[data-mode=history]');
report.push({interaction:'Chat history only restores selected size',passed:await page.locator('.subtitle').textContent().then(s=>s.includes('207 MB'))});
await page.click('[data-action=save-choices]');
await page.click('[data-page=settings]');
report.push({interaction:'Save confirmation is scoped to its page',passed:await page.locator('.toast').count()===0});
await page.goto(url+'?capture&direction=b&page=review&state=progress');
await page.screenshot({path:path.join(output,'b-progress.png')});
await page.click('[data-action=complete]');
await page.screenshot({path:path.join(output,'b-complete.png')});
await page.goto(url+'?direction=b');
await page.screenshot({path:path.join(output,'gallery.png')});

await page.goto(url+'?capture&direction=b&page=review&transfer=pull');
await page.click('[data-action=execute]');
report.push({interaction:'Pull describes restoration, not publication',passed:await page.locator('.progress-section').textContent().then(t=>t.includes('Restoring your handoff')&&!t.includes('Publish completed'))});
await page.screenshot({path:path.join(output,'b-pull-progress.png')});
await page.click('[data-action=complete]');
report.push({interaction:'Pull completion describes the local restoration result',passed:await page.locator('.main').textContent().then(t=>t.includes('Received and verified')&&t.includes('Handoff restored')&&!t.includes('Ready for your drive app.'))});
await page.screenshot({path:path.join(output,'b-pull-complete.png')});
for(const direction of ['a','b','c']){
 await page.goto(url+'?capture&direction='+direction+'&state=incoming');
 report.push({interaction:direction+' incoming handoff is clearly not yet pulled',passed:await page.locator('.main').textContent().then(t=>/not.*pulled/i.test(t)&&t.includes('10:18 AM'))});
 await page.screenshot({path:path.join(output,direction+'-incoming.png')});
}
await page.goto(url+'?capture&direction=b&page=selection');
await page.click('[data-menu="mode:spice"]');
await page.click('[data-mode=full]');
await page.click('[data-page=overview]');
await page.click('[data-action=review-push]');
report.push({interaction:'Full project creates a project-files review row',passed:await page.locator('.file-scroll').textContent().then(t=>t.includes('Project files and Git history')&&t.includes('788 MB'))});
await page.click('[data-tab=notes]');
report.push({interaction:'Details describes included project files',passed:await page.locator('.notes-list').textContent().then(t=>t.includes('1 full project includes code and Git history')&&!t.includes('All projects use Chat history only'))});
await page.goto(url+'?capture&direction=c&page=selection');
await page.click('[data-menu="mode:spice"]');
await page.click('[data-mode=excluded]');
report.push({interaction:'Excluded-project inspector describes exclusion',passed:await page.locator('.inspector').textContent().then(t=>t.includes('excluded from future handoffs'))});
await page.click('[data-page=overview]');
await page.click('[data-action=review-push]');
report.push({interaction:'Excluded project does not leak into review changes',passed:await page.locator('.file-scroll').textContent().then(t=>!t.includes('Refine the settings page')&&!t.includes('Plan the next release')&&t.includes('Pack for Kyoto'))});
await page.goto(url+'?capture&direction=b');
const primary=page.locator('.primary').first();
await primary.hover();
await primary.evaluate(async el=>{await new Promise(requestAnimationFrame);await Promise.all(el.getAnimations().map(a=>a.finished));});
report.push({interaction:'Accent fill persists on hover',passed:await primary.evaluate(el=>{const s=getComputedStyle(el);return s.backgroundColor==='rgb(37, 79, 103)'&&s.color==='rgb(255, 255, 255)';})});

await fs.writeFile(path.join(output,'layout-report.json'),JSON.stringify({errors,report},null,2));
console.log(JSON.stringify({output,errors,layoutChecks:report.length,issues:report.filter(r=>r.overflow?.length||r.unnamed?.length||r.bodyOverflow||r.passed===false)},null,2));
await browser.close();