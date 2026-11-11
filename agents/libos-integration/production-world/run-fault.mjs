import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {realpath, symlink, unlink} from 'node:fs/promises';
const require = createRequire(new URL('../../../apps/web/package.json', import.meta.url));
const {createServer} = await import(require.resolve('vite'));
const {chromium} = require('@playwright/test');
const pkg = '/tmp/stock-libos-real-upstream-product';
const link = 'agents/libos-integration/production-world/pkg';
let server, browser, result, failure;
await symlink(pkg, link, 'dir');
try {
  server = await createServer({root:'apps/web', configFile:'apps/web/vite.config.ts', server:{host:'127.0.0.1',port:4199,strictPort:true,fs:{allow:[process.cwd(),await realpath(pkg)]}}, plugins:[{name:'production-world-probe',configureServer(instance){instance.middlewares.use('/production-world-probe',(_req,res)=>{res.setHeader('Content-Type','text/html');res.end('<!doctype html><script type="module" src="/@fs/Users/asuad/workplace/stock_market_game/agents/libos-integration/production-world/main-fault.ts"></script>');});}}]});
  await server.listen();
  browser = await chromium.launch({headless:true,executablePath:'/Users/asuad/Library/Caches/ms-playwright/chromium-1243/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing'});
  const page = await browser.newPage();
  const errors=[];page.on('pageerror',error=>errors.push(String(error)));
  await page.goto('http://127.0.0.1:4199/production-world-probe');
  await page.waitForFunction(() => window.productionWorldProbe?.children || window.productionWorldProbe?.failure, null, {timeout:20000});
  result = await page.evaluate(() => window.productionWorldProbe);
  assert.deepEqual(errors, []); assert.equal(result.failure, undefined, JSON.stringify(result.failure));
  assert.ok(result.result.stepError, '故障后推进必须显式失败');
  assert.equal(result.result.stepError.code, 'STEP_FATAL');
  assert.match(result.result.stepError.message, /HostAborted/);
  assert.equal(result.children.drained, true);
  assert.equal(result.result.shutdown.ok, false, '故障后 shutdown 不能伪报成功');
  const shutdown = result.result.shutdown.error.result;
  assert.equal(shutdown.primary.stage, 'session_poison');
  assert.match(shutdown.primary.detail, /HostAborted/);
  assert.equal(shutdown.completion.owned_workers, 6);
  assert.equal(shutdown.completion.joined_workers, 6);
  assert.equal(shutdown.completion.worker_results.length, 6);
  assert.equal(new Set(shutdown.completion.worker_results.map(worker => worker.worker)).size, 6);
  assert.ok(shutdown.completion.worker_results.every(worker => Object.hasOwn(worker.result, 'Err')));

} catch(error) { if (result) console.error(JSON.stringify(result)); failure=error; }
finally {
  for (const cleanup of [()=>browser?.close(),()=>server?.close(),()=>unlink(link)]) {
    try { await cleanup(); } catch(error) { failure=failure ? new AggregateError([failure,error], '探针及清理失败') : error; }
  }
}
if (failure) throw failure;
console.log(JSON.stringify(result));
