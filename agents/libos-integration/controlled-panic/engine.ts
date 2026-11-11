import * as glue from './pkg/web_wasm.js';
import { bindFacadeThreadEnvironment, installFacadeSlotLayout } from '../../../apps/web/src/host/engine-clock-env.ts';
import { createFacadeSharedMemory, instantiateFacadeModule } from '../../../apps/web/src/host/facade-wasm-module.ts';
import { createFacadeEngineHandshake } from '../../../apps/web/src/host/facade-engine-handshake.ts';
self.onmessage = async ({data}) => {
  if (data.type === 'start') {
    try {
      const memory = createFacadeSharedMemory({isolated: self.crossOriginIsolated});
      const handshake = createFacadeEngineHandshake({port:data.controlPort, onFailure: error => self.postMessage({type:'fixture-error', message:String(error)})});
      await handshake.registerMemory(memory);
      bindFacadeThreadEnvironment({memory, role:'engine', postControl: handshake.postControl});
      const module = await WebAssembly.compileStreaming(fetch('/agents/libos-integration/controlled-panic/pkg/web_wasm_bg.wasm'));
      instantiateFacadeModule(glue, module, memory);
      const primary = glue.wasm_tombstone_init_primary();
      await handshake.registerDeath(primary);
      glue.facade_slot_init(8);
      const layout = glue.facade_slot_layout();
      installFacadeSlotLayout(layout);
      glue.init_wasm_wait(0, 0, 100, 1000);
      const waitDeath = glue.wasm_wait_death_layout();
      const waitAbort = glue.wasm_wait_abort_layout();
      const libosFault = glue.verification_host_fault_layout();
      await handshake.configure({module,memory,layout,diagnostics:false,libosFault}, waitAbort, waitDeath);
      self.postMessage({type:'ready', memory, waitAbort});
    } catch (error) { self.postMessage({type:'fixture-error', message:String(error)}); }
  } else if (data.type === 'go') {
    try {
      const waitAddress = glue.verification_fault_prepare();
      const gateAddress = glue.verification_fault_gate_address();
      const joinIndex = glue.verification_fault_spawn(data.hardTrap ? 1 : 0);
      let duplicateMessage = '';
      try { glue.verification_fault_spawn(data.hardTrap ? 1 : 0); } catch (error) { duplicateMessage = String(error); }
      self.postMessage({type:'spawned',joinIndex,waitAddress,gateAddress,duplicateMessage});
      glue.verification_fault_wait();
      self.postMessage({type:'fault-woken'});
      try { glue.verification_facade_join(); self.postMessage({type:'unexpected-join-success'}); }
      catch (error) { self.postMessage({type:'join-failed', message:String(error)}); }
    } catch (error) { self.postMessage({type:'fixture-error', message:String(error)}); }
  }
};
