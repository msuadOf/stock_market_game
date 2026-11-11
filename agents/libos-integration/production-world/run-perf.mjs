import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {createRequire} from 'node:module';
import {readFile, realpath, symlink, unlink} from 'node:fs/promises';
import path from 'node:path';
const require = createRequire(new URL('../../../apps/web/package.json', import.meta.url));
const {createServer} = await import(require.resolve('vite'));
const {chromium} = require('@playwright/test');
const pkg = process.env.LIBOS_PRODUCT_PKG;
if (!pkg || !pkg.startsWith('/')) throw Error('必须提供绝对产物路径');
const digest = bytes => createHash('sha256').update(bytes).digest('hex');
const artifactHash = async () => ({wasm:digest(await readFile(path.join(pkg,'web_wasm_bg.wasm'))),glue:digest(await readFile(path.join(pkg,'web_wasm.js')))});
const artifactBefore = await artifactHash();
const link = 'agents/libos-integration/production-world/pkg';
let server, browser, result, failure;
await symlink(pkg, link, 'dir');
try {
  server = await createServer({root:'apps/web', configFile:'apps/web/vite.config.ts', server:{host:'127.0.0.1',port:4199,strictPort:true,fs:{allow:[process.cwd(),await realpath(pkg)]}}, plugins:[{name:'production-world-probe',configureServer(instance){instance.middlewares.use('/production-world-probe',(_req,res)=>{res.setHeader('Content-Type','text/html');res.end('<!doctype html><script type="module" src="/@fs/Users/asuad/workplace/stock_market_game/agents/libos-integration/production-world/main-perf.ts"></script>');});}}]});
  await server.listen();
  browser = await chromium.launch({headless:true,executablePath:'/Users/asuad/Library/Caches/ms-playwright/chromium-1243/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing'});
  const page = await browser.newPage();
  const errors=[];page.on('pageerror',error=>errors.push(String(error)));
  await page.goto('http://127.0.0.1:4199/production-world-probe');
  await page.waitForFunction(() => window.productionWorldProbe?.children || window.productionWorldProbe?.failure, null, {timeout:20000});
  result = await page.evaluate(() => window.productionWorldProbe);
  assert.deepEqual(errors, []); assert.equal(result.failure, undefined, JSON.stringify(result.failure));
  assert.equal(result.result.warm, 1000); assert.equal(result.result.ticks, 3000);
  assert.ok(Number.isFinite(result.result.elapsedMs) && result.result.elapsedMs > 0);
  assert.match(result.result.snapshotDigest, /^[a-f0-9]{64}$/);
  assert.match(result.result.setupDigest, /^[a-f0-9]{64}$/); assert.equal(result.result.seed, '42'); assert.equal(result.children.drained, true);
  assert.ok(result.result.shutdown, '缺少真实 Session shutdown 结果');
  const report = result.result.shutdown.result.runtime;
  assert.equal(report.owned_workers, 6); assert.equal(report.joined_workers, 6);
  assert.equal(report.group_finals, 1); assert.equal(report.desk_finals, 5);
  assert.equal(report.worker_results.length, 6);
  assert.equal(new Set(report.worker_results.map(worker => worker.worker)).size, 6);
  assert.ok(report.worker_results.every(worker => Object.hasOwn(worker.result, 'Ok')), 'worker join 存在失败');
  assert.deepEqual(await artifactHash(), artifactBefore, '采样期间产物字节发生变化');
  result.artifacts = artifactBefore;
  result.joinOkCount = report.worker_results.filter(worker => Object.hasOwn(worker.result, 'Ok')).length;

} catch(error) { failure=error; }
finally {
  for (const cleanup of [()=>browser?.close(),()=>server?.close(),()=>unlink(link)]) {
    try { await cleanup(); } catch(error) { failure=failure ? new AggregateError([failure,error], '探针及清理失败') : error; }
  }
}
if (failure) throw failure;
console.log(JSON.stringify(result));
