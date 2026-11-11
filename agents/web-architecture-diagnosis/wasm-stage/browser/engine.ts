import * as glue from '../pkg/web_wasm_diagnostic.js';
import { DEFAULT_SETUP } from '../../../../apps/web/src/config/defaults.ts';
import { bindFacadeThreadEnvironment, installFacadeSlotLayout } from '../../../../apps/web/src/host/engine-clock-env.ts';
import { createFacadeSharedMemory, instantiateFacadeModule } from '../../../../apps/web/src/host/facade-wasm-module.ts';
self.onmessage=async ({data})=>{
 try {
 if(data.type==='start') {
  const memory=createFacadeSharedMemory({isolated:self.crossOriginIsolated});
  bindFacadeThreadEnvironment({memory,role:'engine',postControl:message=>data.controlPort.postMessage(message)});
  const module=await WebAssembly.compileStreaming(fetch(new URL('../pkg/web_wasm_diagnostic_bg.wasm',import.meta.url)));
  instantiateFacadeModule(glue,module,memory);
  const primary=glue.wasm_tombstone_init_primary();
  glue.set_step_runtime_override(true);
  glue.init_wasm_wait(500,50000,data.sleepUs,data.sleepUs);
  glue.facade_slot_init_for_setup(DEFAULT_SETUP);
  const layout=glue.facade_slot_layout(); installFacadeSlotLayout(layout);
  const waitDeath=glue.wasm_wait_death_layout();
  self.postMessage({type:'ready',module,memory,layout,waitDeath,primary});
 } else if(data.type==='go') {
  glue.diagnostic_game_create(DEFAULT_SETUP,42n);
  const warmStart=performance.now(); const warm=Object.fromEntries(glue.diagnostic_game_steps(1000)); const warmMs=performance.now()-warmStart;
  const start=performance.now();const sample=Object.fromEntries(glue.diagnostic_game_steps(3000));const elapsedMs=performance.now()-start;
  self.postMessage({type:'result',warm,warmMs,sample,elapsedMs,sleepUs:data.sleepUs});
 }
 } catch(e) {self.postMessage({type:'error',message:String(e),stack:e?.stack});}
};
