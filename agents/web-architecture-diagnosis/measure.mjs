// 只用于架构诊断：观测独立浏览器，既有产品源码与消息内容均不修改。
import { MarketUiReportRun, CdpClient, enterGame } from '../../scripts/performance/market-ui-report.mjs';
import { writeFile, readFile } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
const output = 'agents/web-architecture-diagnosis';
const run = new MarketUiReportRun({ url: 'http://127.0.0.1:4192/', browser: '/Users/asuad/Library/Caches/ms-playwright/chromium-1243/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing' });
const samples = [], targets = [], failures = [];
let client, root, workerSession;
function installProbe() {
  const probe = globalThis.__archProbe = { origin: performance.timeOrigin, events: [], longTasks: [] };
  const seen = new WeakSet(), listeners = new WeakMap(), watched = new WeakSet();
  const record = (direction, data) => {
    if (!data || !['protocol', 'protocolBatch', 'uiFrame', 'save', 'saved', 'failure', 'speedMetrics', 'created', 'start', 'stop'].includes(data.type)) return;
    const event = { at: performance.now(), direction, type: data.type };
    for (const key of ['deliveryId', 'requestId', 'generation']) if (data[key] !== undefined) event[key] = data[key];
    if (data.type === 'protocol' || data.type === 'protocolBatch') {
      const updates = data.type === 'protocol' ? [data.update] : data.updates;
      event.updates = updates.length; event.tick = null; event.civil = [];
      for (const update of updates) {
        if (update.TickBatch?.frames?.length) event.tick = update.TickBatch.frames.at(-1).tick;
        if (update.CivilUpdate) event.civil.push({ seq: update.CivilUpdate.seq_to, boundary: update.CivilUpdate.boundary });
      }
    }
    if (data.type === 'speedMetrics') event.metrics = data.metrics;
    if (data.type === 'failure') event.error = data.message;
    probe.events.push(event);
  };
  const proto = MessagePort.prototype, add = proto.addEventListener, remove = proto.removeEventListener, post = proto.postMessage;
  const observe = event => { if (!seen.has(event)) { seen.add(event); record('receive', event.data); } };
  proto.addEventListener = function(type, listener, options) {
    if (type !== 'message' || typeof listener !== 'function') return add.call(this, type, listener, options);
    let wrapped = listeners.get(listener);
    if (!wrapped) { wrapped = function(event) { observe(event); return listener.call(this, event); }; listeners.set(listener, wrapped); }
    return add.call(this, type, wrapped, options);
  };
  proto.removeEventListener = function(type, listener, options) { return remove.call(this, type, listeners.get(listener) || listener, options); };
  proto.postMessage = function(data, ...args) {
    if (!watched.has(this)) { watched.add(this); add.call(this, 'message', observe); }
    const started = performance.now(); record('send', data);
    const result = post.call(this, data, ...args);
    if (['protocol', 'protocolBatch', 'saved'].includes(data?.type)) probe.events.push({ at: performance.now(), direction: 'post-return', type: data.type, deliveryId: data.deliveryId, requestId: data.requestId, duration: performance.now() - started });
    return result;
  };
  if (typeof document !== 'undefined') new PerformanceObserver(list => {
    for (const item of list.getEntries()) probe.longTasks.push({ at: item.startTime, duration: item.duration });
  }).observe({ type: 'longtask' });
}
const expression = `(${installProbe.toString()})()`;
async function evaluate(connection, expression) {
  const result = await connection.send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
  if (result.exceptionDetails) throw new Error(JSON.stringify(result.exceptionDetails));
  return result.result.value;
}
function sendSession(sessionId, method, params = {}) {
  const id = root.nextId++;
  return new Promise((resolve, reject) => {
    root.pending.set(id, { resolve, reject, method });
    root.socket.send(JSON.stringify({ id, sessionId, method, params }));
  });
}
async function workerEval(expression) {
  const result = await sendSession(workerSession, 'Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
  if (result.exceptionDetails) throw new Error(JSON.stringify(result.exceptionDetails));
  return result.result.value;
}
const metrics = async () => Object.fromEntries((await client.send('Performance.getMetrics')).metrics.map(item => [item.name, item.value]));
const state = () => evaluate(client, `(()=>{const n=document.querySelector('.app-root'); return {at:performance.now(),day:Number(n?.dataset.gameDay),tick:Number(n?.dataset.gameTick),error:document.querySelector('.app-error')?.textContent||''}})()`);
try {
  console.log(JSON.stringify({ classification: '真实浏览器代表性性能诊断', logicalCpus: 10, simultaneousSamples: 1, runtime: '产品默认多 Worker', deadlineMs: 300000, reason: '性能样本顺序运行避免相互争用；单样本保留多核 engine' }));
  await run.startServerIfNeeded(); await run.launchBrowser(); client = await run.connectPage();
  const version = await (await fetch(`http://127.0.0.1:${new URL(client.socket.url).port}/json/version`)).json();
  root = new CdpClient(version.webSocketDebuggerUrl); await root.connect();
  await root.send('Target.setDiscoverTargets', { discover: true });
  await client.send('Runtime.enable'); await client.send('Page.enable'); await client.send('Performance.enable');
  await client.send('Page.addScriptToEvaluateOnNewDocument', { source: expression });
  await client.send('Emulation.setDeviceMetricsOverride', { width: 390, height: 1180, deviceScaleFactor: 1, mobile: true });
  await client.send('Page.navigate', { url: 'http://127.0.0.1:4192/' }); await enterGame(client);
  const readyDeadline = Date.now() + 30000;
  while (true) {
    const ready = await evaluate(client, `({created:__archProbe.events.some(e=>e.type==='created'),failure:__archProbe.events.find(e=>e.type==='failure')})`);
    if (ready.failure) throw new Error(JSON.stringify(ready.failure));
    if (ready.created) break;
    if (Date.now() > readyDeadline) throw new Error('市场会话未在30秒内创建');
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  // 当前浏览器的嵌套 Worker inspector 未回应 evaluate；保留目标数量，不冒充取得 Worker 分段耗时。
  targets.push(...(await root.send('Target.getTargets')).targetInfos.filter(item => item.type === 'worker'));
  await evaluate(client, `document.querySelector('.mobile-market-row')?.click()`);
  await new Promise(resolve => setTimeout(resolve, 100));
  await evaluate(client, `(()=>{const s=document.querySelector('select[aria-label="模拟速度"]');if(!s)throw Error('缺少速度控件');s.value='Infinity';s.dispatchEvent(new Event('change',{bubbles:true}));document.querySelector('[aria-label="继续模拟"]')?.click();})()`);
  await new Promise(resolve => setTimeout(resolve, 1000));
  const before = await metrics(), initial = await state(), started = Date.now();
  await client.send('Profiler.enable'); await client.send('Profiler.setSamplingInterval', { interval: 2000 }); await client.send('Profiler.start');
  let profiling = true, finishAt = Infinity;
  console.log(JSON.stringify({ initial, targets }));
  while (Date.now() - started < 180000) {
    await new Promise(resolve => setTimeout(resolve, 2000));
    const current = await state();
    const counts = await evaluate(client, `({events:__archProbe.events.length,civil:__archProbe.events.filter(e=>e.direction==='receive'&&e.civil?.length).length,longTasks:__archProbe.longTasks.length})`);
    const processes = execFileSync('ps', ['-axo', 'pid=,ppid=,%cpu=,rss=,comm='], { encoding: 'utf8', timeout: 1000 }).split('\n').filter(line => line.includes('Google Chrome for Testing')).map(line => {
      const fields = line.trim().split(/\s+/); return { pid: Number(fields[0]), ppid: Number(fields[1]), cpu: Number(fields[2]), rssKiB: Number(fields[3]) };
    });
    samples.push({ ...current, elapsedMs: Date.now() - started, counts, processes });
    console.log(JSON.stringify({ ...current, counts, elapsedMs: Date.now() - started, cpuPercent: processes.reduce((sum, item) => sum + item.cpu, 0) }));
    if (profiling && Date.now() - started > 10000) {
      await writeFile(`${output}/main.cpuprofile`, JSON.stringify((await client.send('Profiler.stop')).profile));
      profiling = false;
    }
    if (current.tick >= 15301) finishAt = Math.min(finishAt, Date.now() + 12000);
    if (current.error) throw new Error(current.error);
    if (Date.now() >= finishAt) break;
  }
  await evaluate(client, `document.querySelector('[aria-label="暂停模拟"]')?.click()`);
  await new Promise(resolve => setTimeout(resolve, 1000));
  const after = await metrics(), final = await state(), main = await evaluate(client, '__archProbe'), engine = null;
  await writeFile(`${output}/observed-run.json`, JSON.stringify({ version, provenance: JSON.parse(await readFile('apps/web/dist/wasm-build-provenance.json', 'utf8')), before, after, initial, final, samples, targets, main, engine }, null, 2));
  console.log(JSON.stringify({ completed: true, final, events: main.events.length, workerEvents: null }));
} catch (error) {
  failures.push(String(error.stack || error)); console.error(error);
  await writeFile(`${output}/observed-failure.json`, JSON.stringify({ failures, samples, targets }, null, 2)); process.exitCode = 1;
} finally { root?.close(); await run.close(); }
