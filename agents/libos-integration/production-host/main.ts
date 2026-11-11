import { createEngineSupervisor } from '../../../apps/web/src/host/engine-supervisor.ts';
const engine = new Worker(new URL('./engine.ts', import.meta.url), {type:'module'});
const result: {ready?:unknown, failure?:unknown} = {};
Object.assign(window, {productionHostProbe:result});
const supervisor = createEngineSupervisor({engine,
  createChildWorker:()=>{ throw Error('握手探针不应创建 child'); },
  onFatalFailure:error=>{if (error.code !== 'LIBOS_PRODUCTION_PROBE') result.failure=error;}, onFailurePending:error=>{if (error.code !== 'LIBOS_PRODUCTION_PROBE') result.failure=error;},
  onSpawnFailure:error=>{result.failure=error;}, reportError:error=>{result.failure=String(error);},
});
engine.addEventListener('error', event=>{result.failure=event.message;});
engine.addEventListener('message', event=>{
  if (event.data.type === 'ready') {
    const {hostReady, slots, memory, layout} = event.data;
    supervisor.discard({code:'LIBOS_PRODUCTION_PROBE', where:'production-host', message:'验证正式故障广播', recoverable:false, recoveryActions:[]});
    result.ready = {hostReady, slots, globalState:Atomics.load(new Int32Array(memory.buffer,layout.globalStateAddress,1),0),
      domainAborted:Atomics.load(new Uint8Array(memory.buffer,layout.domainAbortAddress,1),0),
      abortedSlots:layout.slots.filter((slot:{abortAddress:number})=>Atomics.load(new Uint8Array(memory.buffer,slot.abortAddress,1),0)===1).length};
  }
  if (event.data.type === 'failure') result.failure=event.data;
});
supervisor.bootstrapEngine({type:'start'});
