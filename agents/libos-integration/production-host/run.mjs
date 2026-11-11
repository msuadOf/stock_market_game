import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {realpath, symlink, unlink} from 'node:fs/promises';
const require = createRequire(new URL('../../../apps/web/package.json', import.meta.url));
const {createServer} = await import(require.resolve('vite'));
const {chromium} = require('@playwright/test');
const pkg = '/tmp/stock-libos-production-host-pkg';
const link = 'agents/libos-integration/production-host/pkg';
let server, browser, result, failure;
await symlink(pkg, link, 'dir');
try {
  server = await createServer({root:'apps/web', configFile:'apps/web/vite.config.ts', server:{host:'127.0.0.1',port:4199,strictPort:true,fs:{allow:[process.cwd(),await realpath(pkg)]}}, plugins:[{name:'production-libos-probe',configureServer(instance){instance.middlewares.use('/production-libos-probe',(_req,res)=>{res.setHeader('Content-Type','text/html');res.end('<!doctype html><script type="module" src="/@fs/Users/asuad/workplace/stock_market_game/agents/libos-integration/production-host/main.ts"></script>');});}}]});
  await server.listen();
  browser = await chromium.launch({headless:true,executablePath:'/Users/asuad/Library/Caches/ms-playwright/chromium-1243/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing'});
  const page = await browser.newPage();
  const errors=[];page.on('pageerror',error=>errors.push(String(error)));
  await page.goto('http://127.0.0.1:4199/production-libos-probe');
  await page.waitForFunction(() => window.productionHostProbe?.ready || window.productionHostProbe?.failure, null, {timeout:20000});
  result = await page.evaluate(() => window.productionHostProbe);
  assert.deepEqual(errors, []); assert.equal(result.failure, undefined, JSON.stringify(result.failure));
  assert.equal(result.ready.hostReady, 1); assert.ok(result.ready.slots > 0);
  assert.equal(result.ready.globalState & 2, 2); assert.equal(result.ready.domainAborted, 1);
  assert.equal(result.ready.abortedSlots, result.ready.slots);
} catch(error) { failure=error; }
finally {
  for (const cleanup of [()=>browser?.close(),()=>server?.close(),()=>unlink(link)]) {
    try { await cleanup(); } catch(error) { failure=failure ? new AggregateError([failure,error], '探针及清理失败') : error; }
  }
}
if (failure) throw failure;
console.log(JSON.stringify(result));
