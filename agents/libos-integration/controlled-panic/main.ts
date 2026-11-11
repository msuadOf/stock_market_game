import { createEngineSupervisor } from '../../../apps/web/src/host/engine-supervisor.ts';
const results = { events: [] as any[], failure: undefined as any };
Object.assign(window, { results });
let sharedMemory: WebAssembly.Memory;
let abortAddress: number;
const hostMode = new URLSearchParams(location.search).get('mode') === 'host';
const engine = new Worker(new URL('./engine.ts', import.meta.url), {type:'module'});
const supervisor = createEngineSupervisor({
  engine,
  createChildWorker: () => { results.events.push({type:'child-created'}); return new Worker(new URL('./child.ts', import.meta.url), {type:'module'}); },
  onFatalFailure(failure) { results.failure = failure; },
  onFailurePending(failure) {
    results.events.push({type:'failure-pending', failure, abortValue: Atomics.load(new Uint8Array(sharedMemory.buffer, abortAddress, 1), 0)});
    supervisor.beginShutdown(performance.now() + 1500);
  },
  onSpawnFailure(failure) { results.events.push({type:'spawn-failure', failure}); },
  reportError(error) { results.events.push({type:'report-error', message:String(error)}); },
});
engine.addEventListener('message', event => {
  const data = event.data;
  if (data.type === 'ready') {
    sharedMemory = data.memory;
    abortAddress = data.waitAbort.address;
    supervisor.getSubstrate();
    results.events.push({type:'ready'});
    engine.postMessage({type:'go', hardTrap:hostMode});
  } else if (data.type === 'spawned') {
    results.events.push(data);
    const waiting = new Int32Array(sharedMemory.buffer, data.waitAddress, 1);
    const gate = new Int32Array(sharedMemory.buffer, data.gateAddress, 1);
    const started = performance.now();
    const check = () => {
      if (Atomics.load(waiting, 0) === -1) {
        results.events.push({type:'waiting-registered'});
        Atomics.store(gate, 0, 1); Atomics.notify(gate, 0);
        results.events.push({type:'gate-released'});
      } else if (performance.now() - started >= 1500) {
        results.events.push({type:'fixture-error', message:'接收者等待未登记'});
      } else setTimeout(check, 1);
    };
    check();
  } else results.events.push(data);
});
supervisor.bootstrapEngine({type:'start'});
