import assert from 'node:assert/strict';
import path from 'node:path';
import { symlink, unlink } from 'node:fs/promises';
import { chromium } from '../../../apps/web/node_modules/@playwright/test/index.mjs';
import { createServer } from '../../../apps/web/node_modules/vite/dist/node/index.js';

const root = process.cwd();
const pkg = process.env.WASM_PKG;
if (!pkg || !path.isAbsolute(pkg)) throw new Error('WASM_PKG 必须为绝对路径');
const link = path.join(root, 'agents/wasm-tombstone-followup/browser-fixture/pkg');
await symlink(pkg, link, 'dir');
const server = await createServer({ plugins: [{ name: 'full-host-fixture', enforce: 'pre', transform(code, id) { if (id.split('?')[0].endsWith('/src/host/engine-control-worker.ts')) return `import '/@fs${root}/agents/wasm-tombstone-followup/browser-fixture/full-host-control.ts'`; } }], root: path.join(root, 'apps/web'), server: { host: '127.0.0.1', port: 0, fs: { allow: [root, pkg] } } });
let browser;
try {
  await server.listen();
  browser = await chromium.launch({ headless: true });
  const page = await browser.newPage();
  await page.addInitScript(() => { window.fixtureEvents = []; const channel = new BroadcastChannel('wasm-full-host-fixture'); channel.onmessage = event => window.fixtureEvents.push(event.data); });
  await page.goto(server.resolvedUrls.local[0]);
  assert.equal(await page.evaluate(() => crossOriginIsolated), true);
  await page.getByRole('button', { name: '启动游戏' }).click();
  try { await page.getByText('错误码：WASM_WORKER_DEATH', { exact: false }).first().waitFor({ timeout: 4000 }); } catch (error) { console.log(JSON.stringify({ body: await page.locator('body').innerText(), events: await page.evaluate(() => window.fixtureEvents) })); throw error; }
  await page.waitForFunction(() => window.fixtureEvents.some(event => event.type === 'terminal'), null, { timeout: 4000 });
  const events = await page.evaluate(() => window.fixtureEvents);
  assert.ok(events.some(event => event.type === 'nativeError' && event.message.includes('unreachable')));
  assert.ok(events.some(event => event.type === 'termination' && event.name.includes(':engine')));
  const failure = events.find(event => event.type === 'failure');
  assert.equal(failure?.failure.context.records.find(record => record.identity.workerId === 0)?.tombstone.message, 'WASM_PRIMARY_TOMBSTONE_PROBE');
  assert.ok(events.some(event => event.type === 'terminal' && event.release.kind === 'unknown'));
  assert.match(await page.locator('body').innerText(), /释放失败.*release unknown/);
  console.log(JSON.stringify({ fullHostFailure: true, events }));
} finally {
  if (browser) await browser.close();
  await server.close();
  await unlink(link);
}
