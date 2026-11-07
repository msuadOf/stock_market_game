import { readWasmTombstone } from '../../../apps/web/src/host/wasm-tombstone.ts';
const role = new URLSearchParams(location.search).get('role');
const result = {events: [] as unknown[], tombstone: undefined as unknown};
Object.assign(window,{result});
const engine = new Worker(new URL('./primary-worker.ts',import.meta.url),{type:'module'});
let active = engine;
let observeTrap: ((message: unknown) => void) | undefined;
engine.onmessage = ({data}) => {
  if (data.type === 'trap') { observeTrap?.(data); return; }
  if (data.type !== 'ready') return;
  const {memory,module,primary} = data;
  result.events.push({type:'layout',primary});
  const target = primary[role === 'ingress' ? 'ingress' : 'engine'];
  const death = new Int32Array(memory.buffer,primary.death.address,1);
  const wait = Atomics.waitAsync(death,0,0,5000);
  if (!wait.async) throw new Error('必须先进入 death futex 等待');
  void wait.value.then(value => result.events.push({type:'wake',value,word:Atomics.load(death,0)}));
  const observe = () => {
    result.tombstone = readWasmTombstone(memory,target.address,target.workerId);
    try { readWasmTombstone(memory,target.address,target.workerId === 0 ? 1 : 0); throw new Error('错误身份未拒绝'); }
    catch (error) { if (!String(error).includes('身份不匹配')) throw error; result.events.push({type:'identity-rejected'}); }
    engine.terminate(); if (active !== engine) active.terminate();
  };
  observeTrap = message => { result.events.push(message); observe(); };
  if (role === 'ingress') {
    active = new Worker(new URL('./primary-worker.ts',import.meta.url),{type:'module'});
    active.onerror = observe;
    active.onmessage = ({data:message}) => { if (message.type === 'trap') observeTrap?.(message); else { result.events.push(message); if (message.type === 'bound') active.postMessage({type:'panic'}); } };
    active.postMessage({type:'ingress',memory,module,primary});
  } else { engine.onerror = observe; engine.postMessage({type:'panic'}); }
};
engine.postMessage({type:'engine'});
