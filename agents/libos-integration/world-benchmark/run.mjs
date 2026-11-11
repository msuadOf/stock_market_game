import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {createRequire} from 'node:module';
import {readFile, realpath, symlink, unlink, writeFile} from 'node:fs/promises';
import path from 'node:path';
import {failureDetail} from './failure-detail.mjs';
import {parseFineWaitUs} from './wait-config.mjs';
import {validateShutdown} from './shutdown-check.mjs';
import {validateStepProfile} from './step-profile.mjs';

const pkg = process.env.LIBOS_DIAGNOSTIC_PKG;
if (!pkg || !path.isAbsolute(pkg)) throw Error('LIBOS_DIAGNOSTIC_PKG 必须为绝对路径');
const mode = process.env.LIBOS_DIAGNOSTIC_MODE;
if (mode !== 'std' && mode !== 'libos') throw Error('LIBOS_DIAGNOSTIC_MODE 必须为 std 或 libos');
const workerMode = process.env.LIBOS_DIAGNOSTIC_WORKER ?? 'std';
const mathMode = process.env.LIBOS_DIAGNOSTIC_MATH ?? 'std';
const profileMode = process.env.LIBOS_DIAGNOSTIC_PROFILE ?? '0';
if (profileMode !== '0' && profileMode !== '1') throw Error('画像参数必须为 0 或 1');
if (mathMode !== 'std' && mathMode !== 'libm') throw Error('观察概率数学模式必须为 std 或 libm');
const checkShutdown = process.env.LIBOS_DIAGNOSTIC_SHUTDOWN === '1';
const checkChildren = process.env.LIBOS_DIAGNOSTIC_CHILDREN === '1';
if (checkChildren && !checkShutdown) throw Error('检查 child exit 必须同时检查正常 shutdown');
if (workerMode !== 'std' && workerMode !== 'libos') throw Error('LIBOS_DIAGNOSTIC_WORKER 必须为 std 或 libos');
if (workerMode === 'libos' && mode !== 'libos') throw Error('实验性 LibOS 下行要求 LibOS 上行');
const faultMode = process.env.LIBOS_DIAGNOSTIC_FAULT === '1';
const fineWaitUs = parseFineWaitUs(process.env.LIBOS_DIAGNOSTIC_FINE_US);
if (faultMode && mode !== 'libos') throw Error('故障场景必须启用 libos 上行');
const output = process.env.LIBOS_DIAGNOSTIC_OUTPUT || `/tmp/stock-libos-world-benchmark-${mode}.json`;
if (!path.isAbsolute(output)) throw Error('LIBOS_DIAGNOSTIC_OUTPUT 必须为绝对路径');
const wasmHash = createHash('sha256').update(await readFile(path.join(pkg, 'web_wasm_diagnostic_bg.wasm'))).digest('hex');
const glueHash = createHash('sha256').update(await readFile(path.join(pkg, 'web_wasm_diagnostic.js'))).digest('hex');
const link = 'agents/libos-integration/world-benchmark/pkg';
const require = createRequire(new URL('../../../apps/web/package.json', import.meta.url));
const {createServer} = await import(require.resolve('vite'));
const {chromium} = require('@playwright/test');
let server, browser, page, result, failure;
const errors = [];
await symlink(pkg, link, 'dir');
try {
  server = await createServer({root:'apps/web', configFile:'apps/web/vite.config.ts', server:{host:'127.0.0.1', port:4198, strictPort:true, fs:{allow:[process.cwd(), await realpath(pkg)]}}, plugins:[{name:'libos-world-benchmark', configureServer(instance) { instance.middlewares.use('/libos-world-benchmark', (_request, response) => { response.setHeader('Content-Type','text/html'); response.end('<!doctype html><html><script type="module" src="/@fs/Users/asuad/workplace/stock_market_game/agents/libos-integration/world-benchmark/main.ts"></script></html>'); }); }}]});
  await server.listen();
  browser = await chromium.launch({headless:true, executablePath:'/Users/asuad/Library/Caches/ms-playwright/chromium-1243/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing'});
  page = await browser.newPage();
  page.on('pageerror', error => errors.push(String(error)));
  page.on('console', message => {
    if (message.text() === 'LIBOS_CPU_WINDOW:cpu-sample-start' || message.text() === 'LIBOS_CPU_WINDOW:cpu-sample-end') process.stderr.write(`${message.text()}\n`);
  });
  await page.goto(`http://127.0.0.1:4198/libos-world-benchmark?upstreamMode=${mode}&workerMode=${workerMode}&mathMode=${mathMode}&profile=${profileMode}&shutdown=${checkShutdown ? '1' : '0'}&children=${checkChildren ? '1' : '0'}&fault=${faultMode ? '1' : '0'}&fineWaitUs=${fineWaitUs}`);
  await page.waitForFunction(({fault, children}) => window.results?.failure !== undefined || (window.results?.result && (!fault || (window.results?.finalFailure && window.results?.shutdown && window.results?.children)) && (!children || window.results?.children)), {fault:faultMode, children:checkChildren}, {timeout:faultMode ? 8000 : 90000});
  result = await page.evaluate(() => window.results);
  assert.equal(errors.length, 0, JSON.stringify(errors));
  assert.equal(result.failure, undefined, JSON.stringify(result.failure));
  if (faultMode) {
    assert.ok(result.events.includes('failure-pending:LIBOS_HOST_ABORT_PROBE'));
    assert.equal(result.finalFailure.code, 'LIBOS_HOST_ABORT_PROBE');
    assert.equal(result.finalSnapshot.disposed, true, 'supervisor 必须进入已处置状态');
    assert.ok(result.shutdown, '必须取得真实 GameSession shutdown 的结果');
    assert.equal(result.shutdown.ok, false, '已失败的 Session shutdown 不得伪报成功');
    {
      assert.ok(result.shutdown.failure?.primary?.stage, '失败 shutdown 必须保留主错误阶段');
      assert.ok(result.shutdown.failure?.primary?.detail, '失败 shutdown 必须保留主错误详情');
      assert.equal(result.shutdown.failure.primary.stage, 'session_poison');
      assert.match(result.shutdown.failure.primary.detail, /HostAborted/, '主错误必须保留宿主故障');
      assert.ok(Array.isArray(result.shutdown.failure.secondary), '失败 shutdown 必须保留次生错误列表');
      const completion = result.shutdown.failure.completion;
      assert.ok(completion, '必须取得真实 worker join 报告');
      assert.ok(completion.owned_workers > 0, 'fixture 必须实际拥有 worker');
      assert.equal(completion.joined_workers, completion.owned_workers, '必须收获全部 owned worker 的 join 结果');
      assert.equal(completion.worker_results.length, completion.joined_workers, '每次 join 必须保留独立结果');
      assert.equal(new Set(completion.worker_results.map(worker => worker.worker)).size, completion.joined_workers, 'join 的 worker 身份不得重复');
    }
    assert.equal(typeof result.children.drained, 'boolean', '必须记录 child drain 的结果');
    assert.equal(result.children.drained, true, `child drain 必须完成：${result.children.error ?? '无附加诊断'}`);
    assert.ok(result.result.stepError, '宿主故障后推进必须失败');
    assert.match(result.result.stepError, /HostAborted \{ phase: (TickConverge|Seal|Drain)/, '推进错误必须携带宿主故障及相位');
    assert.equal(result.result.probe.retryOk, false, '终态会话不得重试');
    assert.equal(result.result.probe.saveOk, false, '终态会话不得生成存档');
    assert.ok(result.result.probe.retryError, '重试拒绝必须保留原因');
    assert.ok(result.result.probe.saveError, '存档拒绝必须保留原因');
    assert.equal(result.result.probe.retryError, result.result.stepError, '重试必须保留首次失败');
    assert.equal(result.result.probe.saveError, result.result.stepError, '存档拒绝必须保留首次失败');
    assert.equal(result.result.probe.tickAfter, result.result.probe.tickBefore);
  } else {
    if (checkShutdown) {
      validateShutdown(result.result.shutdown);
    }
    if (checkChildren) assert.equal(result.children?.drained, true, `child exit 未完成：${result.children?.error ?? '缺少结果'}`);
    assert.equal(result.result.fineWaitUs, fineWaitUs, '实际等待配置与请求不符');
    assert.equal(result.result.mathMode, mathMode, '实际数学模式与请求不符');
    if (profileMode === '1') validateStepProfile(result.result.stepProfile, 3000);
    assert.equal(result.result.warm.from, 0);
    assert.equal(result.result.warm.to, 1000);
    assert.equal(result.result.sample.from, 1000);
    assert.equal(result.result.sample.to, 4000);
    assert.ok(result.result.elapsedMs > 0);
  }
} catch (error) {
  failure = error;
  result = page ? await page.evaluate(() => window.results).catch(snapshotError => ({snapshotError:String(snapshotError)})) : undefined;
} finally {
  for (const cleanup of [() => browser?.close(), () => server?.close(), () => unlink(link)]) {
    try { await cleanup(); }
    catch (error) { failure = failure ? new AggregateError([failure, error], '基准和清理均失败') : error; }
  }
}
if (failure) {
  try { await writeFile(`${output}.failure`, JSON.stringify({mode,workerMode,mathMode,profileMode,fineWaitUs,wasmHash,glueHash,result,errors,error:failureDetail(failure)}, null, 2), {flag:'wx'}); }
  catch (error) { console.error('写入失败证据失败', error); }
  throw failure;
}
await writeFile(output, JSON.stringify({mode,workerMode,mathMode,profileMode,fineWaitUs,wasmHash,glueHash,node:process.version,result,errors}, null, 2), {flag:'wx'});
console.log(JSON.stringify(result));
