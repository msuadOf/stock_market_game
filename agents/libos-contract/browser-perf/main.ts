import { createEngineSupervisor } from '../../../apps/web/src/host/engine-supervisor.ts';

if (!crossOriginIsolated) throw new Error('需要实际跨源隔离的 Browser 页面');
let failure: unknown;
const engine = new Worker(new URL('./engine.ts', import.meta.url), { type: 'module' });
const observations: unknown[] = [];
const supervisor = createEngineSupervisor({
  engine,
  createChildWorker: () => new Worker(new URL('./child.ts', import.meta.url), { type: 'module' }),
  onFatalFailure(value) { failure = value; },
  onFailurePending(value) { failure = value; },
  onSpawnFailure(value) { failure = value; },
  onLifecycleObservation(observation) { observations.push(observation); },
  reportError(error) { failure = error; },
});

function ask<T>(expected: string, send: () => void): Promise<T> {
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => finish(() => reject(new Error('Browser Worker 样本超时'))), 8_000);
    const finish = (done: () => void) => {
      clearTimeout(timeout);
      engine.removeEventListener('message', message);
      engine.removeEventListener('error', broken);
      done();
    };
    const message = ({ data }: MessageEvent) => finish(() => {
      if (data.type === expected) resolve(data);
      else reject(new Error(data.error ?? '未收到预期的 Browser Worker 消息'));
    });
    const broken = (error: ErrorEvent) => finish(() => reject(new Error(error.message)));
    engine.addEventListener('message', message);
    engine.addEventListener('error', broken);
    try { send(); } catch (error) { finish(() => reject(error)); }
  });
}

const now = () => performance.now();
// waitForChildrenExit 会收紧此 supervisor 的共同 deadline，不能把每段预算传成永久上限。
const scenarioDeadline = now() + 180_000;
const ready = ask<Parameters<typeof supervisor.installSubstrate>[0] & { workerResolutionNs: number }>(
  'ready', () => supervisor.bootstrapEngine({ type: 'start' }),
);
const workerResolutionNs = await ready.then(({ module, memory, layout, workerResolutionNs }) => {
  if (!(memory.buffer instanceof SharedArrayBuffer)) throw new Error('需要真实共享的 WASM Memory');
  if (!Number.isFinite(workerResolutionNs) || workerResolutionNs <= 0) throw new Error('缺少实际 Worker 定时器分辨率');
  supervisor.installSubstrate({ module, memory, layout, diagnostics: false });
  return workerResolutionNs;
});

async function drainChildren() {
  let timeout: ReturnType<typeof setTimeout> | undefined;
  try {
    await Promise.race([
      supervisor.waitForChildrenExit(scenarioDeadline),
      new Promise<never>((_, reject) => {
        timeout = setTimeout(() => reject(new Error('样本 Worker 物理回收超时')), 1500);
      }),
    ]);
  } finally {
    if (timeout !== undefined) clearTimeout(timeout);
  }
}

let resolving = false;
async function sample(name: string, wrapped: boolean, operations: number) {
  if (resolving || failure !== undefined) throw new Error('上个样本未回收或 Worker 故障');
  resolving = true;
  const from = observations.length;
  try {
    const start = now();
    const { result } = await ask<{ result: { elapsedNs: number; completedNs: number | null } }>(
      'success', () => engine.postMessage({ type: 'sample', name, wrapped, operations }),
    );
    // Worker 结果与实际释放使用不同完成点；等待同一个 supervisor 回收协议。
    await drainChildren();
    const retiredNs = (now() - start) * 1_000_000;
    if (failure !== undefined) throw new Error('Worker 监督失败：' + JSON.stringify(failure));
    const { children, protocolEntries, pendingRecovery } = supervisor.snapshot();
    return { ...result, retiredNs, observations: observations.slice(from), children, protocolEntries, pendingRecovery };
  } finally {
    resolving = false;
  }
}

function timerResolutionNs() {
  let previous = now();
  let interval = Infinity;
  for (let i = 0; i < 100_000; i++) {
    const next = now();
    if (next > previous) interval = Math.min(interval, next - previous);
    previous = next;
  }
  if (!Number.isFinite(interval)) throw new Error('缺失浏览器定时器分辨率样本');
  return Math.max(workerResolutionNs, Math.ceil(interval * 1_000_000));
}
Object.assign(window, { libosBrowserPerf: { sample, timerResolutionNs } });
