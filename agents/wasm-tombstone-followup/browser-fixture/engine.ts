import * as glue from './pkg/web_wasm.js';
import { bindFacadeThreadEnvironment, installFacadeSlotLayout } from '../../../apps/web/src/host/engine-clock-env.ts';
import { createFacadeSharedMemory, instantiateFacadeModule } from '../../../apps/web/src/host/facade-wasm-module.ts';
let mode = 0;
self.onmessage = async ({data}) => {
  if (data.type === 'start') {
    mode = data.mode;
    const memory = createFacadeSharedMemory({isolated: self.crossOriginIsolated});
    bindFacadeThreadEnvironment({memory, role:'engine', postControl: message => data.controlPort.postMessage(message)});
    const module = await WebAssembly.compileStreaming(fetch('/agents/wasm-tombstone-followup/browser-fixture/pkg/web_wasm_bg.wasm'));
    instantiateFacadeModule(glue, module, memory);
    const primary = glue.wasm_tombstone_init_primary();
    glue.facade_slot_init(8);
    const layout = glue.facade_slot_layout();
    installFacadeSlotLayout(layout);
    glue.init_wasm_wait(0, 0, 100, 1000);
    const waitDeath = glue.wasm_wait_death_layout();
    self.postMessage({type:'ready',module,memory,layout,waitDeath,primary});
  } else if (data.type === 'go') {
    const joinIndex = glue.verification_facade_spawn_death(mode);
    self.postMessage({type:'spawned',joinIndex});
    try { glue.verification_facade_join(); self.postMessage({type:'unexpected-join-success'}); }
    catch (error) { self.postMessage({type:'join-failed', message:String(error)}); }
  }
};
