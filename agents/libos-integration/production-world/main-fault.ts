import { createEngineSupervisor } from '../../../apps/web/src/host/engine-supervisor.ts';
const engine = new Worker(new URL('./engine-fault.ts', import.meta.url), {type:'module'});
const result: {result?:unknown, children?:unknown, failure?:unknown} = {};
Object.assign(window, {productionWorldProbe:result});
const supervisor = createEngineSupervisor({engine,
  createChildWorker:()=>new Worker(new URL('./child.ts', import.meta.url), {type:'module'}),
  onFatalFailure:error=>{if (error.code !== 'UPSTREAM_DEFAULT_FAULT') result.failure=error;}, onFailurePending:error=>{if (error.code !== 'UPSTREAM_DEFAULT_FAULT') result.failure=error;},
  onSpawnFailure:error=>{result.failure=error;}, reportError:error=>{result.failure=String(error);},
});
engine.addEventListener('error', event=>{result.failure=event.message;});
engine.addEventListener('message', event=>{
  if (event.data.type === 'ready') {
    if (event.data.hostReady !== 1) result.failure = 'host-ready 不为 1';
    else engine.postMessage({type:'go'});
  }
  if (event.data.type === 'executing') supervisor.discard({code:'UPSTREAM_DEFAULT_FAULT',where:'production-world',message:'验证默认上行故障',recoverable:false,recoveryActions:[]});
  if (event.data.type === 'result') {
    result.result=event.data;
    const deadline=performance.now()+3500;
    supervisor.beginShutdown(deadline);
    void supervisor.waitForChildrenExit(deadline).then(()=>{result.children={drained:true};},error=>{result.failure=String(error);});
  }
  if (event.data.type === 'failure') result.failure=event.data;
});
supervisor.bootstrapEngine({type:'start'});
