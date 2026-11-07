// 真实产品 host 验收：必须从外部 run-long-validation 启动并共享单一期限。
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile, readdir, mkdir, writeFile, realpath } from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { performance } from 'node:perf_hooks';
import { runBoundedCommand } from '../../../scripts/run-with-deadline.mjs';
import { captureWasmBuildSourceIdentity, wasmBuildSourceSha256, readWasmPackageMetadata } from '../../../scripts/wasm-package-capability.mjs';
import { assertHostBusinessParity } from './assert-business-trace.mjs';

const root = await realpath(process.cwd());
const [artifactArg, outputArg] = process.argv.slice(2);
assert.equal(process.argv.length, 4, '必须提供 WASM package 和新的证据目录');
const artifact = await realpath(artifactArg), output = path.resolve(outputArg);
const start = performance.now(), end = start + 298000;
const left = () => { const remaining = Math.floor(end - performance.now()); assert(remaining > 1000, '共享验收期限不足'); return remaining; };
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const digest = async file => { const bytes = await readFile(file); return { bytes: bytes.length, sha256: hash(bytes) }; };
const write = (name, data) => writeFile(path.join(output, name), JSON.stringify(data, null, 2), { flag: 'wx' });
async function sourceRows() {
  const paths = ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', '.cargo/config.toml', 'packages/engine/examples/calendar_business_fixture.rs', 'packages/engine/examples/calendar_business_native.rs', 'packages/engine/tests/common/mixed_calendar_c2c_fixture.rs'];
  async function walk(dir) { for (const entry of await readdir(path.join(root, dir), { withFileTypes: true })) { const name = `${dir}/${entry.name}`; if (entry.isDirectory()) await walk(name); else if (entry.isFile()) paths.push(name); else throw new Error(`源码目录含非普通文件 ${name}`); } }
  for (const dir of ['packages/engine/src', 'apps/web-wasm/src', 'apps/web/src', 'agents/worktree-migration-20261106/calendar-v11-draft']) await walk(dir);
  return Promise.all(paths.sort().map(async name => ({ name, ...await digest(path.join(root, name)) })));
}
await mkdir(output);
const before = await sourceRows();
const rust = await captureWasmBuildSourceIdentity(root);
const metadata = await readWasmPackageMetadata(artifact);
assert.equal(wasmBuildSourceSha256(rust), metadata.provenance.sourceSha256, 'WASM 来源与当前源码不同');
for (const [name, expected] of [['web_wasm_bg.wasm', metadata.provenance.wasmSha256], ['web_wasm.js', metadata.provenance.glueSha256]]) assert.equal((await digest(path.join(artifact, name))).sha256, expected);
assert.equal(await realpath(path.join(root, 'apps/web/wasm-pkg')), artifact, '产品 WASM 路径必须绑定同一 package');
const cargo = process.env.CARGO || 'cargo';
await runBoundedCommand({ command: cargo, args: ['build', '-p', 'engine', '--example', 'calendar_business_fixture', '--example', 'calendar_business_native', '-j', '8', '--offline'], cwd: root, timeoutMs: left() });
const fixtureExe = path.join(root, 'target/debug/examples/calendar_business_fixture');
const nativeExe = path.join(root, 'target/debug/examples/calendar_business_native');
const executable = { fixture: await digest(fixtureExe), native: await digest(nativeExe) };
const input = path.join(output, 'input');
await runBoundedCommand({ command: fixtureExe, args: [input], cwd: root, timeoutMs: Math.min(10000, left()) });
const fixture = await readFile(path.join(input, 'fixture.json')), script = await readFile(path.join(input, 'script.json'));
const nativePath = path.join(output, 'native.json');
await runBoundedCommand({ command: nativeExe, args: [path.join(input, 'fixture.json'), path.join(input, 'script.json'), nativePath], cwd: root, timeoutMs: Math.min(10000, left()) });
const { createServer } = await import(pathToFileURL(path.join(root, 'apps/web/node_modules/vite/dist/node/index.js')));
const { chromium } = await import(pathToFileURL(path.join(root, 'apps/web/node_modules/@playwright/test/index.mjs')));
const served = [], servedAssets = [];
const wasmUrl = `/@fs${path.join(artifact, 'web_wasm_bg.wasm')}`;
const server = await createServer({ root, configFile: false, mode: 'e2e', logLevel: 'warn', resolve: { alias: [{ find: /^env$/, replacement: path.join(root, 'apps/web/src/host/engine-clock-env.ts') }] }, server: { host: '127.0.0.1', port: 0, fs: { allow: [root, artifact] } }, plugins: [{ name: 'calendar-business', configureServer(vite) { vite.middlewares.use(async (req, res, next) => {
  res.setHeader('Cross-Origin-Opener-Policy', 'same-origin'); res.setHeader('Cross-Origin-Embedder-Policy', 'require-corp'); res.setHeader('Cache-Control', 'no-store');
  try {
    const url = new URL(req.url, 'http://127.0.0.1').pathname;
    if (url === wasmUrl) {
      const bytes = await readFile(path.join(artifact, 'web_wasm_bg.wasm'));
      assert.equal(hash(bytes), metadata.provenance.wasmSha256);
      const row = { id: `asset-${servedAssets.length}`, url, sha256: hash(bytes), bytes: bytes.length, finished: false };
      servedAssets.push(row); res.setHeader('x-calendar-asset', row.id); res.setHeader('Content-Type', 'application/wasm');
      res.on('finish', () => { row.finished = true; }); res.end(bytes); return;
    }
    if (url.endsWith('/web_wasm.js') || url.startsWith('/apps/web/src/') || url.startsWith('/agents/worktree-migration-20261106/calendar-v11-draft/')) {
      let source;
      if (url.endsWith('/web_wasm.js')) source = await digest(path.join(artifact, 'web_wasm.js'));
      else {
        const relative = decodeURIComponent(url.slice(1));
        const frozen = before.find(item => item.name === relative);
        assert(frozen, `实际 TS 响应缺少冻结源码 ${relative}`);
        source = await digest(path.join(root, relative));
        assert.equal(source.sha256, frozen.sha256, `实际 TS 源码已漂移 ${relative}`);
      }
      if (url.endsWith('/web_wasm.js')) assert.equal(source.sha256, metadata.provenance.glueSha256);
      const row = { id: `asset-${servedAssets.length}`, url, source, finished: false, chunks: [] };
      servedAssets.push(row); res.setHeader('x-calendar-asset', row.id);
      const originalWrite = res.write, originalEnd = res.end;
      res.write = function(chunk, ...args) { if (chunk) row.chunks.push(Buffer.from(chunk)); return originalWrite.call(this, chunk, ...args); };
      res.end = function(chunk, ...args) { if (chunk) row.chunks.push(Buffer.from(chunk)); const bytes = Buffer.concat(row.chunks); row.bytes = bytes.length; row.sha256 = hash(bytes); delete row.chunks; return originalEnd.call(this, chunk, ...args); };
      res.on('finish', () => { row.finished = true; });
    }
    if (req.url === '/__probe') { res.setHeader('Content-Type', 'text/html'); res.end('<script type="module" src="/agents/worktree-migration-20261106/calendar-v11-draft/browser-host-probe.ts"></script>'); return; }
    let bytes;
    if (req.url === '/__c2c_fixture.json') bytes = fixture;
    else if (req.url === '/__c2c_script.json') bytes = script;
    else if (req.url?.startsWith('/wasm-metadata/')) { const name = req.url.slice('/wasm-metadata/'.length); assert(['wasm-build-provenance.json', 'verified-restore-capability.json'].includes(name)); bytes = await readFile(path.join(artifact, name)); }
    if (bytes) { served.push({ url: req.url, bytes: bytes.length, sha256: hash(bytes) }); res.setHeader('Content-Type', 'application/json'); res.end(bytes); return; }
    next();
  } catch (error) { res.statusCode = 500; res.end(String(error)); }
}); } }] });
let browser, primaryError;
const logs = [], responses = [], responseTasks = [];
try {
  await server.listen(); const address = server.httpServer.address(); assert(address && typeof address !== 'string');
  browser = await chromium.launch({ headless: true, args: ['--disable-dev-shm-usage'] });
  const page = await browser.newPage();
  page.on('pageerror', error => logs.push(String(error))); page.on('requestfailed', request => logs.push(`${request.url()}: ${request.failure()?.errorText}`));
  page.on('response', response => {
    const id = response.headers()['x-calendar-asset'];
    if (!id) return;
    responseTasks.push((async () => {
      const failure = await response.finished();
      responses.push({ id, url: response.url(), status: response.status(), finishedWithoutError: failure === null });
    })().then(() => undefined, error => { responses.push({ id, error: String(error) }); }));
  });
  await page.goto(`http://127.0.0.1:${address.port}/__probe`);
  await page.waitForFunction(() => window.c2cProbe !== undefined, undefined, { timeout: left() });
  const result = await page.evaluate(() => window.c2cProbe);
  await Promise.all(responseTasks);
  await write('browser-raw.json', { result, logs, responses, served, servedAssets, evidenceMode: 'server-response-buffer-and-browser-metadata', directBrowserBodyHashProven: false });
  for (const asset of servedAssets) {
    const matches = responses.filter(row => row.id === asset.id);
    assert.equal(matches.length, 1, `浏览器响应缺失或重复 ${asset.url}`);
    assert(asset.finished && matches[0].status === 200 && matches[0].finishedWithoutError, `响应未完成 ${asset.url}`);
  }
  assert(servedAssets.some(row => row.url === wasmUrl && row.sha256 === metadata.provenance.wasmSha256));
  assert(servedAssets.some(row => row.url.endsWith('/web_wasm.js')));
  assert(servedAssets.some(row => row.url.endsWith('/browser-host-probe.ts')));
  assert(servedAssets.some(row => row.url.endsWith('/worker-host.ts')));
  assert.equal(result.ok, true, '真实产品 host 执行失败，原件已保存');
  assert.equal(result.cleanupError, undefined);
  assert(served.some(row => row.url === '/__c2c_fixture.json' && row.sha256 === hash(fixture)));
  assert(served.some(row => row.url === '/__c2c_script.json' && row.sha256 === hash(script)));
  assertHostBusinessParity(JSON.parse(await readFile(nativePath, 'utf8')), result);
} catch (error) { primaryError = error; }
finally {
  const cleanup = await Promise.allSettled([browser ? browser.close() : Promise.resolve(), server.close()]);
  const failures = cleanup.filter(row => row.status === 'rejected').map(row => row.reason);
  if (primaryError || failures.length) throw new AggregateError([...(primaryError ? [primaryError] : []), ...failures], '浏览器验收或清理失败', { cause: primaryError });
}
assert.deepEqual(await sourceRows(), before, '验收期间源码发生变化');
for (const [name, expected] of [['web_wasm_bg.wasm', metadata.provenance.wasmSha256], ['web_wasm.js', metadata.provenance.glueSha256]]) assert.equal((await digest(path.join(artifact, name))).sha256, expected);
left();
await write('manifest.json', { source: before, executable, artifact, metadata, fixture: { bytes: fixture.length, sha256: hash(fixture) }, script: { bytes: script.length, sha256: hash(script) }, native: await digest(nativePath), browser: await digest(path.join(output, 'browser-raw.json')), jobs: 8, calendar: 'synthetic-test-c2c-mixed-calendar' });
console.log('真实 native 与产品 browser 业务 oracle 和公开投影通过');
