import { chromium } from '../../../apps/web/node_modules/@playwright/test/index.mjs';
import { spawn } from 'node:child_process';
import assert from 'node:assert/strict';
import { symlink, unlink } from 'node:fs/promises';
import path from 'node:path';
const pkg = process.env.LIBOS_CONTROLLED_PKG;
if (!pkg || !path.isAbsolute(pkg)) throw Error('LIBOS_CONTROLLED_PKG 必须为构建目录绝对路径');
const link = 'agents/libos-integration/controlled-panic/pkg';
await symlink(pkg, link, 'dir');
const server = spawn(process.execPath, ['apps/web/node_modules/vite/bin/vite.js','--config','agents/wasm-tombstone-followup/browser-fixture/vite.config.mjs','--port','4188','--strictPort'], {stdio:'inherit'});
const serverExit = new Promise(resolve => server.once('exit', resolve));
let browser;
try {
  const deadline = Date.now() + 5000;
  while (true) {
    try { const response = await fetch('http://127.0.0.1:4188/agents/libos-integration/controlled-panic/'); if (response.ok) break; }
    catch (error) { if (Date.now() >= deadline) throw error; }
    if (Date.now() >= deadline) throw Error('Vite 启动超时');
    await new Promise(resolve => setTimeout(resolve, 50));
  }
  browser = await chromium.launch({headless:true,executablePath:'/Users/asuad/Library/Caches/ms-playwright/chromium-1243/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing'});
  const page = await browser.newPage();
  page.on('pageerror', error => console.error('页面错误',error));
  await page.goto('http://127.0.0.1:4188/agents/libos-integration/controlled-panic/');
  try { await page.waitForFunction(() => window.results?.failure !== undefined && ((window.results.events.some(event => event.type === 'fault-woken') && window.results.events.some(event => event.type === 'join-failed')) || window.results.events.some(event => event.type === 'fixture-error')), null, {timeout:8000}); }
  catch (error) { console.error('当前状态',JSON.stringify(await page.evaluate(() => window.results))); throw error; }
  const result = await page.evaluate(() => window.results);
  console.log(JSON.stringify(result));
  assert.equal(result.events.filter(event => event.type === 'fixture-error').length, 0);
  assert.ok(result.events.some(event => event.type === 'spawned' && event.duplicateMessage.includes('验证 child 句柄已存在')));
  assert.equal(result.events.filter(event => event.type === 'child-created').length, 1);
  assert.ok(result.events.some(event => event.type === 'waiting-registered'));
  assert.ok(result.events.some(event => event.type === 'gate-released'));
  assert.ok(result.events.some(event => event.type === 'fault-woken'));
  assert.ok(result.events.some(event => event.type === 'join-failed' && event.message.includes('FACADE_DEAD_PANIC')));
  const record = result.failure.context.records.find(entry => entry.identity.workerId === 1);
  assert.ok(record, '必须保留 child 身份');
  assert.equal(record.tombstone.message, 'LIBOS_FAULT_WAKE_PROBE');
  assert.equal(record.tombstone.kind, 'panic');
  await page.close();
  const hostPage = await browser.newPage();
  hostPage.on('pageerror', error => console.error('宿主页错误',error));
  await hostPage.goto('http://127.0.0.1:4188/agents/libos-integration/controlled-panic/?mode=host');
  try { await hostPage.waitForFunction(() => window.results?.failure !== undefined && ((window.results.events.some(event => event.type === 'fault-woken') && window.results.events.some(event => event.type === 'join-failed')) || window.results.events.some(event => event.type === 'fixture-error')), null, {timeout:8000}); }
  catch (error) { console.error('宿主场景当前状态',JSON.stringify(await hostPage.evaluate(() => window.results))); throw error; }
  const hostResult = await hostPage.evaluate(() => window.results);
  console.log(JSON.stringify({host:hostResult}));
  assert.equal(hostResult.events.filter(event => event.type === 'fixture-error').length, 0);
  assert.equal(hostResult.events.filter(event => event.type === 'child-created').length, 1);
  assert.ok(hostResult.events.some(event => event.type === 'spawned' && event.duplicateMessage.includes('验证 child 句柄已存在')));
  assert.ok(hostResult.events.some(event => event.type === 'waiting-registered'));
  assert.ok(hostResult.events.some(event => event.type === 'gate-released'));
  assert.ok(hostResult.events.some(event => event.type === 'failure-pending' && event.abortValue === 1));
  assert.ok(hostResult.events.some(event => event.type === 'fault-woken'));
  assert.ok(hostResult.events.some(event => event.type === 'join-failed' && event.message.includes('FACADE_DEAD_HARDTRAP')));
  const hostRecord = hostResult.failure.context.records.find(entry => entry.identity.workerId === 1);
  assert.deepEqual(hostRecord.tombstone, {state:'absent'});
  await hostPage.close();
} finally {
  try { if (browser) await browser.close(); }
  finally { server.kill('SIGTERM'); try { await serverExit; } finally { await unlink(link); } }
}
