import { createEngineSupervisor } from '../../../apps/web/src/host/engine-supervisor.ts';
import { parseFineWaitUs } from './wait-config.mjs';
const results = {events:[] as string[], failure:undefined as unknown, finalFailure:undefined as unknown, finalSnapshot:undefined as unknown, shutdown:undefined as unknown, children:undefined as unknown, result:undefined as unknown};
Object.assign(window, {results});
const engine = new Worker(new URL('./engine.ts', import.meta.url), {type:'module'});
const faultMode = new URLSearchParams(location.search).get('fault') === '1';
const upstreamMode = new URLSearchParams(location.search).get('upstreamMode');
const workerMode = new URLSearchParams(location.search).get('workerMode');
const mathMode = new URLSearchParams(location.search).get('mathMode');
const profile = new URLSearchParams(location.search).get('profile') === '1';
const checkShutdown = new URLSearchParams(location.search).get('shutdown') === '1';
const checkChildren = new URLSearchParams(location.search).get('children') === '1';
const fineWaitUs = parseFineWaitUs(new URLSearchParams(location.search).get('fineWaitUs') ?? undefined);
if (upstreamMode !== 'std' && upstreamMode !== 'libos') throw Error('必须指定 std 或 libos 上行模式');
if (workerMode !== 'std' && workerMode !== 'libos') throw Error('必须指定 std 或 libos 下行模式');
if (mathMode !== 'std' && mathMode !== 'libm') throw Error('必须指定 std 或 libm 观察概率模式');
const supervisor = createEngineSupervisor({
  engine,
  createChildWorker:() => new Worker(new URL('./child.ts', import.meta.url), {type:'module'}),
  onFatalFailure:failure => { if (faultMode) { results.finalFailure = failure; results.finalSnapshot = supervisor.snapshot(); } else results.failure = failure; },
  onFailurePending:failure => { if (faultMode) results.events.push(`failure-pending:${failure.code}`); else results.failure = failure; },
  onSpawnFailure:failure => { results.failure = failure; },
  reportError:error => { results.failure = String(error); },
});
engine.addEventListener('error', event => { results.failure = event.message; });
engine.addEventListener('message', event => {
  const data = event.data;
  if (data.type === 'cpu-sample-start' || data.type === 'cpu-sample-end') console.log(`LIBOS_CPU_WINDOW:${data.type}`);
  results.events.push(data.type);
  if (data.type === 'ready') engine.postMessage({type:faultMode ? 'fault-go' : 'go'});
  else if (data.type === 'executing' && faultMode) {
    supervisor.discard({code:'LIBOS_HOST_ABORT_PROBE', where:'world-benchmark', message:'验证宿主永久故障', recoverable:false, recoveryActions:[]});
  }
  else if (data.type === 'fault-result') {
    results.result = {...data, supervisor:supervisor.snapshot()};
    const deadline = performance.now() + 3500;
    supervisor.beginShutdown(deadline);
    void supervisor.waitForChildrenExit(deadline).then(
      () => { results.children = {drained:true}; },
      error => { results.children = {drained:false, error:String(error)}; },
    );
    engine.postMessage({type:'fault-shutdown'});
  }
  else if (data.type === 'shutdown-result') {
    results.shutdown = data.shutdown;
  }
  else if (data.type === 'result') {
    results.result = data;
    if (checkChildren) {
      const deadline = performance.now() + 3500;
      supervisor.beginShutdown(deadline);
      void supervisor.waitForChildrenExit(deadline).then(
        () => { results.children = {drained:true}; },
        error => { results.children = {drained:false, error:String(error)}; },
      );
    }
  }
  else if (data.type === 'error') results.failure = data;
});
supervisor.bootstrapEngine({type:'start', upstreamMode, workerMode, mathMode, profile, checkShutdown, fineWaitUs, runtimeOverride:true});
