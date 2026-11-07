import * as glue from './pkg/web_wasm.js';
import { bindFacadeThreadEnvironment } from '../../../apps/web/src/host/engine-clock-env.ts';
import { createFacadeSharedMemory, instantiateFacadeModule } from '../../../apps/web/src/host/facade-wasm-module.ts';
import { createFacadeEngineHandshake } from '../../../apps/web/src/host/facade-engine-handshake.ts';
self.onmessage = async ({data}) => {
  if (data.type !== 'bindEngineTransport') throw new Error('未知 engine transport');
  const handshake = createFacadeEngineHandshake({port:data.controlPort,onFailure(error) { throw error; }});
  const memory = createFacadeSharedMemory({isolated:self.crossOriginIsolated});
  await handshake.registerMemory(memory);
  bindFacadeThreadEnvironment({memory,role:'engine',postControl:message => handshake.postControl(message)});
  const module = await WebAssembly.compileStreaming(fetch('/agents/wasm-tombstone-followup/browser-fixture/pkg/web_wasm_bg.wasm'));
  instantiateFacadeModule(glue,module,memory);
  const primary = glue.wasm_tombstone_init_primary();
  await handshake.registerDeath(primary);
  setTimeout(() => glue.verification_tombstone_primary_panic(),0);
};
