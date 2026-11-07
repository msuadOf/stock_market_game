import { createEngineSupervisor } from '../../../apps/web/src/host/engine-supervisor.ts';
const params = new URLSearchParams(location.search);
const mode = Number(params.get('mode'));
const pending = params.get('pending') === '1';
const results = { events: [] as unknown[], failure: undefined as unknown };
Object.assign(window, { results });
const engine = new Worker(new URL('./engine.ts', import.meta.url), {type:'module'});
const supervisor = createEngineSupervisor({
  engine,
  createChildWorker: () => new Worker(new URL('./child.ts', import.meta.url), {type:'module'}),
  onFatalFailure(failure) { results.failure = failure; },
  onFailurePending(failure) { results.events.push({type:'failure-pending', failure}); supervisor.beginShutdown(performance.now() + 300); },
  onSpawnFailure(failure) { results.events.push({spawnFailure:failure}); },
  reportError(error) { results.events.push({reportError:String(error)}); },
});
engine.addEventListener('message', event => {
  if (event.data.type === 'ready') {
    const {module,memory,layout,waitDeath} = event.data;
    supervisor.installSubstrate({module,memory,layout,diagnostics:false,...(pending ? {waitDeath} : {})});
    const word = new Int32Array(memory.buffer, waitDeath.address, 1);
    const waiting = Atomics.waitAsync(word, 0, 0, 5000);
    if (!waiting.async) throw new Error('必须先进入 death futex 等待');
    void waiting.value.then(value => results.events.push({type:'death-wake', value, word:Atomics.load(word, 0)}));
    results.events.push({type:'ready',waitDeath});
    engine.postMessage({type:'go'});
  } else results.events.push(event.data);
});
supervisor.bootstrapEngine({type:'start',mode});
