import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';

const require = createRequire(new URL('../../../apps/web/package.json', import.meta.url));
const { chromium } = require('@playwright/test');
const wasm = await readFile('/tmp/stock-libos-browser-target/wasm32-unknown-unknown/debug/libos_browser_probe.wasm');
const worker = await readFile(new URL('./worker.js', import.meta.url));
const server = createServer((req, res) => {
  res.setHeader('Cross-Origin-Opener-Policy', 'same-origin');
  res.setHeader('Cross-Origin-Embedder-Policy', 'require-corp');
  if (req.url === '/probe.wasm') { res.setHeader('Content-Type', 'application/wasm'); res.end(wasm); }
  else if (req.url === '/worker.js') { res.setHeader('Content-Type', 'text/javascript'); res.end(worker); }
  else { res.setHeader('Content-Type', 'text/html'); res.end('<!doctype html><title>LibOS probe</title>'); }
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));

let browser;
try {
  browser = await chromium.launch({ headless: true, executablePath: '/Users/asuad/Library/Caches/ms-playwright/chromium-1243/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing' });
  const page = await browser.newPage();
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  const result = await page.evaluate(async () => {
    if (!crossOriginIsolated) throw Error('缺少跨域隔离');
    const module = await WebAssembly.compileStreaming(fetch('/probe.wasm'));
    const memory = new WebAssembly.Memory({ initial: 256, maximum: 16384, shared: true });
    const workers = [new Worker('/worker.js'), new Worker('/worker.js'), new Worker('/worker.js')];
    const messageQueues = new WeakMap(workers.map(worker => [worker, []]));
    const messageWaiters = new WeakMap();
    const workerErrors = new WeakMap(workers.map(worker => [worker, []]));
    for (const worker of workers) {
      worker.addEventListener('message', ({ data }) => {
        const waiter = messageWaiters.get(worker);
        if (waiter) {
          clearTimeout(waiter.timer);
          messageWaiters.delete(worker);
          if (data.type !== waiter.expected) waiter.reject(Error(JSON.stringify(data)));
          else waiter.resolve(data);
        } else {
          messageQueues.get(worker).push(data);
        }
      });
      worker.addEventListener('error', event => {
        const waiter = messageWaiters.get(worker);
        if (waiter) {
          clearTimeout(waiter.timer);
          messageWaiters.delete(worker);
          waiter.reject(Error(event.message || 'Worker 执行失败'));
        } else {
          workerErrors.get(worker).push(event.message || 'Worker 执行失败');
        }
      });
    }
    const next = (worker, expected) => {
      const queued = messageQueues.get(worker).shift();
      if (queued) {
        if (queued.type !== expected) return Promise.reject(Error(JSON.stringify(queued)));
        return Promise.resolve(queued);
      }
      const priorError = workerErrors.get(worker).shift();
      if (priorError) return Promise.reject(Error(priorError));
      if (messageWaiters.has(worker)) return Promise.reject(Error(`Worker 已有未完成的消息等待: ${expected}`));
      return new Promise((resolve, reject) => {
        const waiter = { expected, resolve, reject };
        waiter.timer = setTimeout(() => {
          messageWaiters.delete(worker);
          reject(Error(`等待 ${expected} 超时`));
        }, 3000);
        messageWaiters.set(worker, waiter);
      });
    };
    const send = async (worker, message, expected) => {
      const pending = next(worker, expected);
      worker.postMessage(message);
      return pending;
    };
    const prepare = async (mode = 0) => send(workers[0], { type: 'prepare', mode }, 'prepared');
    const prepareGroup = async count => send(workers[0], { type: 'prepareGroup', count }, 'preparedGroup');
    const finish = async layout => send(workers[0], { type: 'finish', ptr: layout.ptr }, 'finished');
    const receive = async (worker, layout, { timeoutMs = 0, phase } = {}) => {
      const started = next(worker, phase === undefined ? 'receiving' : 'receivingPhase');
      worker.postMessage(phase === undefined
        ? { type: 'receive', session: layout.session, timeoutMs }
        : { type: 'phaseReceive', session: layout.session, phase, normalMs: timeoutMs, longMs: timeoutMs });
      await started;
      const result = await next(worker, 'received');
      return result.result;
    };
    const waitParked = async layout => {
      if (layout.waitWord === 0 || layout.active === 0) throw Error('LibOS verification 未返回等待观察地址');
      const word = new Int32Array(memory.buffer, layout.waitWord, 1);
      const active = new Int8Array(memory.buffer, layout.active, 1);
      for (let i = 0; i < 200; i++) {
        if (Atomics.load(active, 0) === 1 && Atomics.load(word, 0) === -1) return;
        await new Promise(resolve => setTimeout(resolve, 5));
      }
      throw Error('接收 Worker 未进入 LibOS PARKED 状态');
    };
    try {
      const ready0 = next(workers[0], 'ready');
      workers[0].postMessage({ type: 'init', module, memory });
      await ready0;
      const allocateStack = async () => (await send(workers[0], { type: 'stack' }, 'stack')).top;
      const stacks = [await allocateStack(), await allocateStack(), await allocateStack()];
      if (stacks.some(top => top % 16 !== 0 || top < 524288 || top > memory.buffer.byteLength) || stacks.some((top, i) => stacks.some((other, j) => i !== j && Math.abs(top - other) < 524288))) {
        throw Error(`测试栈范围非法: ${stacks}`);
      }
      const set = await send(workers[0], { type: 'sentinelSet', value: 1234567 }, 'sentinelSet');
      void set;
      const ready1 = next(workers[1], 'ready');
      workers[1].postMessage({ type: 'init', module, memory, stackTop: stacks[1] });
      await ready1;
      const ready2 = next(workers[2], 'ready');
      workers[2].postMessage({ type: 'init', module, memory, stackTop: stacks[2] });
      await ready2;
      await send(workers[0], { type: 'setStack', top: stacks[0] }, 'stackSet');
      const sentinel = await send(workers[1], { type: 'sentinelGet' }, 'sentinelGet');
      if (sentinel.value !== 1234567) throw Error('第二次实例化改变了共享 sentinel');

      const sendTrial = async () => {
        const layout = await prepare(0);
        const receiving = receive(workers[1], layout, { timeoutMs: 2000 });
        await waitParked(layout);
        const sent = next(workers[0], 'sent');
        workers[0].postMessage({ type: 'send', session: layout.session, value: 123 });
        await sent;
        const [status, value] = await receiving;
        if (status !== 0 || value !== 123) throw Error(`消息发送结果错误: ${status},${value}`);
        await finish(layout);
        return { kind: 'send', status, value };
      };
      const abortTrial = async () => {
        const layout = await prepare(0);
        const receiving = receive(workers[1], layout, { timeoutMs: 2000 });
        await waitParked(layout);
        const aborted = next(workers[0], 'aborted');
        workers[0].postMessage({ type: 'abort', session: layout.session });
        await aborted;
        const [status] = await receiving;
        if (status !== 3) throw Error(`LibOS FaultDomain abort 结果错误: ${status}`);
        await finish(layout);
        return { kind: 'faultDomainAbort', status };
      };
      const disconnectTrial = async () => {
        const layout = await prepare(0);
        const receiving = receive(workers[1], layout, { timeoutMs: 2000 });
        await waitParked(layout);
        const disconnected = next(workers[0], 'disconnected');
        workers[0].postMessage({ type: 'disconnect', session: layout.session });
        await disconnected;
        const [status] = await receiving;
        if (status !== 2) throw Error(`LibOS 断连结果错误: ${status}`);
        await finish(layout);
        return { kind: 'disconnect', status };
      };
      const timeoutTrial = async () => {
        const layout = await prepare(0);
        const started = performance.now();
        const [status, , waitedMs] = await receive(workers[1], layout, { timeoutMs: 80 });
        const elapsedMs = performance.now() - started;
        if (status !== 4 || waitedMs < 65) throw Error(`LibOS timeout 结果错误: ${status},${waitedMs}`);
        await finish(layout);
        return { kind: 'timeout', status, waitedMs, elapsedMs };
      };
      const phaseTimeoutTrial = async () => {
        const layout = await prepare(0);
        const [status, , waitedMs, phase] = await receive(workers[1], layout, { timeoutMs: 80, phase: 2 });
        if (status !== 4 || waitedMs < 65 || phase !== 2) throw Error(`LibOS phase deadline 结果错误: ${status},${waitedMs},${phase}`);
        await finish(layout);
        return { kind: 'phaseTimeout', status, waitedMs, phase };
      };
      const prebufferTrial = async () => {
        const layout = await prepare(0);
        const sent = next(workers[0], 'sent');
        workers[0].postMessage({ type: 'send', session: layout.session, value: 456 });
        await sent;
        const [status, value] = await receive(workers[1], layout, { timeoutMs: 500 });
        if (status !== 0 || value !== 456) throw Error(`预入队消息结果错误: ${status},${value}`);
        await finish(layout);
        return { kind: 'prebuffer', status, value };
      };
      const stopTrial = async () => {
        const layout = await prepare(1);
        const receiving = receive(workers[1], layout);
        await waitParked(layout);
        const stopped = next(workers[0], 'stopped');
        workers[0].postMessage({ type: 'stop', session: layout.session });
        await stopped;
        const [status] = await receiving;
        if (status !== 1) throw Error(`LibOS downlink stop 结果错误: ${status}`);
        const empty = await send(workers[1], { type: 'tryReceive', session: layout.session }, 'triedReceive');
        if (empty.result[0] !== 1) throw Error(`stop 不应将控制载荷插入业务队列: ${empty.result}`);
        await finish(layout);
        return { kind: 'downlinkStop', status, queueStatus: empty.result[0] };
      };
      const multiSlotAbortTrial = async () => {
        const group = await prepareGroup(2);
        const [first, second] = group.layouts;
        if (!first || !second || first.waitWord === 0 || second.waitWord === 0) {
          throw Error('同域多槽布局不完整');
        }
        const firstReceive = receive(workers[1], first, { timeoutMs: 3000 });
        const secondReceive = receive(workers[2], second, { timeoutMs: 3000 });
        await Promise.all([waitParked(first), waitParked(second)]);
        const aborted = next(workers[0], 'aborted');
        workers[0].postMessage({ type: 'abort', session: first.session });
        await aborted;
        const [[firstStatus], [secondStatus]] = await Promise.all([firstReceive, secondReceive]);
        if (firstStatus !== 3 || secondStatus !== 3) {
          throw Error(`同一 FaultDomain 多槽故障结果错误: ${firstStatus},${secondStatus}`);
        }
        await Promise.all([finish(first), finish(second)]);
        const released = await send(workers[0], {
          type: 'freeWords', ptr: group.ptr, length: group.count + 1,
        }, 'wordsFreed');
        void released;
        return { kind: 'multiSlotFaultDomainAbort', statuses: [firstStatus, secondStatus] };
      };
      const hardTrapTrial = async () => {
        let timer;
        const trapped = new Promise((resolve, reject) => {
          workers[2].onerror = event => { clearTimeout(timer); resolve(event.message || 'uncaught worker error'); };
          timer = setTimeout(() => reject(Error('WASM hard trap 未被 Worker 宿主观察到')), 3000);
        });
        workers[2].postMessage({ type: 'hardTrap' });
        const detail = await trapped;
        return { kind: 'isolatedWasmHardTrap', observed: true, detail };
      };

      return [
        await sendTrial(),
        await abortTrial(),
        await disconnectTrial(),
        await timeoutTrial(),
        await phaseTimeoutTrial(),
        await prebufferTrial(),
        await multiSlotAbortTrial(),
        await stopTrial(),
        await hardTrapTrial(),
      ];
    } finally {
      workers.forEach(worker => worker.terminate());
    }
  });
  console.log(JSON.stringify(result));
} finally {
  await browser?.close();
  await new Promise(resolve => server.close(resolve));
}
