import {createEngineSupervisor} from '../../../../apps/web/src/host/engine-supervisor.ts';
const sleepUs=Number(new URLSearchParams(location.search).get('sleepUs'));
const results:any={events:[]};Object.assign(window,{results});
const engine=new Worker(new URL('./engine.ts',import.meta.url),{type:'module'});
const supervisor=createEngineSupervisor({engine,createChildWorker:()=>new Worker(new URL('./child.ts',import.meta.url),{type:'module'}),onFatalFailure:f=>results.failure=f,onFailurePending:f=>{results.failure=f;},onSpawnFailure:f=>{results.failure=f;},reportError:e=>{results.failure=String(e);}});
engine.addEventListener('error', e=>results.failure=e.message);
engine.addEventListener('message',event=>{const d=event.data;results.events.push(d.type);if(d.type==='ready'){supervisor.installSubstrate({module:d.module,memory:d.memory,layout:d.layout,diagnostics:false,waitDeath:d.waitDeath});engine.postMessage({type:'go',sleepUs});} else if(d.type==='result') results.result=d;else if(d.type==='error') results.failure=d;});
supervisor.bootstrapEngine({type:'start',sleepUs});
