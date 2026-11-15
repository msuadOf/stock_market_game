import { createEngineSupervisor } from '../../../apps/web/src/host/engine-supervisor.ts';
const params = new URLSearchParams(location.search);
const mode = Number(params.get('mode'));
const pending = params.get('pending') === '1';
const fixture = params.get('fixture') ?? 'child-death';
const results = { events: [] as unknown[], failure: undefined as unknown };
Object.assign(window, { results });
const engine = new Worker(new URL('./engine.ts', import.meta.url), {type:'module'});
const supervisor = createEngineSupervisor({
  engine,
  createChildWorker: () => {
    if (fixture === 'deferred-lifecycle' && mode === 4) throw new Error('LIBOS_RECLAIMER_SPAWN_DENIED_PROBE');
    return new Worker(new URL('./child.ts', import.meta.url), {type:'module'});
  },
  onFatalFailure(failure) { results.failure = failure; },
  onFailurePending(failure) { results.events.push({type:'failure-pending', failure}); supervisor.beginShutdown(performance.now() + 300); },
  onSpawnFailure(failure) { results.events.push({spawnFailure:failure}); },
  onLifecycleObservation(event) {
    if (fixture === 'deferred-drop' || fixture === 'deferred-lifecycle') results.events.push(event);
  },
  reportError(error) { results.events.push({reportError:String(error)}); },
});
engine.addEventListener('message', event => {
  if (event.data.type === 'ready') {
    const {module,memory,layout,waitDeath} = event.data;
    supervisor.installSubstrate({module,memory,layout,diagnostics:false,...(pending ? {waitDeath} : {})});
    if (fixture !== 'child-death') {
      results.events.push({type:'ready'});
      engine.postMessage({type:'go'});
      return;
    }
    const word = new Int32Array(memory.buffer, waitDeath.address, 1);
    const waiting = Atomics.waitAsync(word, 0, 0, 5000);
    if (!waiting.async) throw new Error('必须先进入 death futex 等待');
    void waiting.value.then(value => results.events.push({type:'death-wake', value, word:Atomics.load(word, 0)}));
    results.events.push({type:'ready',waitDeath});
    engine.postMessage({type:'go'});
  } else if (fixture === 'deferred-lifecycle' && mode === 3 && event.data.type === 'deferred-fault-armed') {
    results.events.push(event.data, {type:'deferred-fault-trigger'});
    engine.postMessage({type:'deferred-fault-go'});
  } else if (event.data.type === 'verification-result') {
    void (async () => {
      try {
        // Worker 返回成功以后仍要从 supervisor 取得真实 child 退出证明。
        await supervisor.waitForChildrenExit(performance.now() + 2000);
        const {children, protocolEntries, pendingRecovery} = supervisor.snapshot();
        if (fixture === 'deferred-drop' || fixture === 'deferred-lifecycle') {
          results.events.push({...event.data, children, protocolEntries, pendingRecovery});
        } else {
          const {memory} = supervisor.getSubstrate();
          const length = fixture === 'world-stop' ? 16 : 4;
          const words = new Uint32Array(memory.buffer, event.data.address, length);
          const observation = Array.from(words, (_, index) => Atomics.load(words, index));
          results.events.push({...event.data, observation, children, protocolEntries, pendingRecovery});
        }
      } catch (error) {
        results.events.push({type:'verification-error',message:String(error)});
      }
    })();
  } else results.events.push(event.data);
});
supervisor.bootstrapEngine({type:'start',mode,fixture});
