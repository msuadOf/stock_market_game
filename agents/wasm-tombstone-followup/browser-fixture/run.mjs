import { chromium } from '../../../apps/web/node_modules/@playwright/test/index.mjs';
import { spawn } from 'node:child_process';
import assert from 'node:assert/strict';
import { symlink, unlink } from 'node:fs/promises';
import path from 'node:path';
const pkg = process.env.WASM_PKG;
if (!pkg || !path.isAbsolute(pkg)) throw new Error('WASM_PKG 必须指向绝对路径的验证构建目录');
const link = 'agents/wasm-tombstone-followup/browser-fixture/pkg';
await symlink(pkg, link, 'dir');
const server = spawn(process.execPath, ['apps/web/node_modules/vite/bin/vite.js','--config','agents/wasm-tombstone-followup/browser-fixture/vite.config.mjs','--port','4179','--strictPort'], {stdio:'inherit'});
const serverExit = new Promise(resolve => server.once('exit', resolve));
let browser;
try {
  const deadline = Date.now() + 5000;
  while (true) {
    try { const response = await fetch('http://127.0.0.1:4179/agents/wasm-tombstone-followup/browser-fixture/'); if (response.ok) break; }
    catch (error) { if (Date.now() >= deadline) throw error; }
    if (Date.now() >= deadline) throw new Error('Vite 启动超时');
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  browser = await chromium.launch({headless:true});
  for (const [mode,pending] of [[0,false],[1,false],[2,false],[0,true],[1,true],[2,true]]) {
    const page = await browser.newPage();
    page.on('pageerror', error => console.error('页面错误',error));
    await page.goto(`http://127.0.0.1:4179/agents/wasm-tombstone-followup/browser-fixture/?mode=${mode}&pending=${Number(pending)}`);
    await page.waitForFunction(() => window.results?.failure !== undefined, null, {timeout:8000});
    if (!pending && mode !== 2) await page.waitForFunction(() => window.results.events.some(event => event.type === 'join-failed') && window.results.events.some(event => event.type === 'death-wake'), null, {timeout:8000});
    const result = await page.evaluate(() => window.results);
    console.log(JSON.stringify({mode,pending,...result}));
    assert.ok(result.events.some(event => event.type === 'ready'), '必须真实初始化共享 WASM');
    if (pending) {
      assert.ok(result.events.some(event => event.type === 'failure-pending'), '必须先通知 failurePending');
      assert.equal(result.failure.context.incomplete, true);
      const record = result.failure.context.records.find(entry => entry.identity.workerId === 1);
      assert.ok(record, '最终报告必须保留 child 身份');
      const tombstone = record.tombstone;
      if (mode === 2) assert.deepEqual(tombstone, {state:'absent'});
      else assert.deepEqual(tombstone, {state:'live',kind:'panic',phase:0,message: mode === 0 ? '真实 Worker panic 消息' : 'panic 消息不可用',truncated:false});
    } else {
      assert.equal(result.failure.code, 'FACADE_BOOTSTRAP_FAILURE');
      const tombstone = result.failure.context.tombstone;
      if (mode === 2) {
        assert.deepEqual(tombstone, {state:'absent'});
        assert.ok(!result.events.some(event => event.type === 'death-wake'), 'hard trap 不得伪造 panic 唤醒');
      } else {
        assert.deepEqual(tombstone, {state:'live',kind:'panic',phase:0,message: mode === 0 ? '真实 Worker panic 消息' : 'panic 消息不可用',truncated:false});
        assert.ok(result.events.some(event => event.type === 'death-wake' && event.value === 'ok' && event.word === 1), 'panic 必须唤醒已入队的 death waiter');
        assert.ok(result.events.some(event => event.type === 'join-failed' && event.message.includes('FACADE_DEAD_PANIC')), 'panic 必须唤醒 join 并报告死亡');
      }
    }
    await page.close();
  }
  for (const role of ['engine','ingress']) {
    const page = await browser.newPage();
    page.on('pageerror', error => console.error('主 Worker 页面错误', error));
    page.on('console', message => console.error('主 Worker console', message.text()));
    await page.goto(`http://127.0.0.1:4179/agents/wasm-tombstone-followup/browser-fixture/primary.html?role=${role}`);
    try { await page.waitForFunction(() => window.result?.tombstone !== undefined && window.result.events.some(event => event.type === 'wake'), null, {timeout:8000}); }
    catch (error) { console.error('primary 当前状态', JSON.stringify(await page.evaluate(() => window.result))); throw error; }
    const result = await page.evaluate(() => window.result);
    console.log(JSON.stringify({role,...result}));
    assert.deepEqual(result.tombstone,{state:'live',kind:'panic',phase:0,message:'WASM_PRIMARY_TOMBSTONE_PROBE',truncated:false});
    assert.ok(result.events.some(event => event.type === 'identity-rejected'));
    assert.ok(result.events.some(event => event.type === 'wake' && event.value === 'ok' && event.word === 1));
    if (role === 'ingress') assert.ok(result.events.some(event => event.type === 'bound' && event.invalidIdentity && event.invalidAddress && event.duplicate));
    await page.close();
  }
  {
    const page = await browser.newPage();
    page.on('pageerror', error => console.error('owner 页面错误',error));
    await page.goto('http://127.0.0.1:4179/agents/wasm-tombstone-followup/browser-fixture/owner.html');
    try { await page.waitForFunction(() => window.result?.events.some(event => event.type === 'fixtureOwnerTerminated'),null,{timeout:8000}); }
    catch (error) { console.error('owner 当前状态',JSON.stringify(await page.evaluate(() => window.result))); throw error; }
    const result = await page.evaluate(() => window.result);
    console.log(JSON.stringify({owner:result}));
    assert.ok(result.events.some(event => event.type === 'fixtureNativeError' && event.message.includes('unreachable')), '必须观察真实 Worker error');
    assert.ok(result.events.some(event => event.type === 'failurePending'));
    assert.ok(result.events.some(event => event.type === 'fixtureTermination' && event.name.includes(':engine')), 'owner 必须终止 engine Worker');
    const failure = result.events.find(event => event.type === 'failure');
    assert.ok(failure, 'owner 必须向 host 交付最终故障');
    const record = failure.failure.context.records.find(entry => entry.identity.workerId === 0);
    assert.equal(record.tombstone.message,'WASM_PRIMARY_TOMBSTONE_PROBE');
    assert.ok(result.events.some(event => event.type === 'terminal' && event.release.kind === 'unknown'), '未证明静止时必须报告 unknown');
    await page.close();
  }
} finally { if (browser) await browser.close(); server.kill('SIGTERM'); await serverExit; await unlink(link); }
