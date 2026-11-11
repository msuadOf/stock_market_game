// 独立诊断：保留产品 WorkerHost，省略 React 与自动存档。
import { createRequire } from 'node:module';
import { writeFile } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
const require = createRequire(new URL('../../apps/web/package.json', import.meta.url));
const { createServer } = await import(require.resolve('vite'));
const { chromium } = require('@playwright/test');
const out = 'agents/web-architecture-diagnosis/bare-host-result.json';
let server, browser;
const result = { classification: 'Vite 开发服务器隔离宿主样本', seed: '42', logicalCpus: 10, simultaneousMeasurements: 1, durationTargetMs: 25000, samples: [], errors: [] };
try {
  server = await createServer({root:'apps/web', configFile:'apps/web/vite.config.ts', server:{host:'127.0.0.1',port:4197,strictPort:true}, plugins:[{name:'diagnostic-bare',configureServer(s){s.middlewares.use('/diagnostic-bare', (_req,res)=>{res.statusCode=200;res.setHeader('Content-Type','text/html');res.end('<!doctype html><html><body>diagnostic host</body></html>');});}}]});
  await server.listen();
  browser = await chromium.launch({headless:true, executablePath:'/Users/asuad/Library/Caches/ms-playwright/chromium-1243/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing'});
  const page = await browser.newPage();
  page.on('pageerror', error => result.errors.push(String(error)));
  await page.goto('http://127.0.0.1:4197/diagnostic-bare');
  await page.evaluate(async () => {
    const {createWorkerHost} = await import('/src/host/worker-host.ts');
    const {DEFAULT_SETUP} = await import('/src/config/defaults.ts');
    window.failures = []; window.tickObserved = 0;
    window.host = await createWorkerHost(DEFAULT_SETUP, 42n, {onSpawnFailure: f => window.failures.push(f)});
    await window.host.setSpeed(Infinity);
    await window.host.start(update => { if (update.type === 'baseline') window.tickObserved = update.snapshot.tick; else if (update.update.TickBatch) window.tickObserved = update.update.TickBatch.frames.at(-1).tick; return true; }, f => window.failures.push(f));
  });
  const started = Date.now();
  let first;
  while (Date.now()-started < 80000) {
    await new Promise(resolve=>setTimeout(resolve,1000));
    const item = await page.evaluate(() => ({at:performance.now(), tick:window.tickObserved, failures:window.failures}));
    result.samples.push(item);
    if (item.failures.length) throw new Error(JSON.stringify(item.failures));
    if (!first && item.tick >= 1200) first = item;
    if (first && item.at - first.at >= 25000) break;
  }
  const last = result.samples.at(-1);
  if (!first || last.at - first.at < 25000) throw new Error('测量窗口未完成');
  result.window = {first,last,elapsedMs:last.at-first.at,ticksPerSecond:(last.tick-first.tick)*1000/(last.at-first.at)};
  result.processes = execFileSync('ps',['-axo','pid=,ppid=,%cpu=,rss=,comm='],{encoding:'utf8'}).split('\n').filter(line=>line.includes('Google Chrome for Testing'));
  await page.evaluate(async () => {await window.host.stop(); await window.host.dispose();});
  result.completed = true;
} catch (error) {result.errors.push(String(error.stack || error)); process.exitCode = 1;}
finally {await browser?.close(); await server?.close(); await writeFile(out,JSON.stringify(result,null,2)); console.log(JSON.stringify(result.window ?? result.errors));}
