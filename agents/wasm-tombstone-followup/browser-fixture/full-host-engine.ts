import * as glue from './pkg/web_wasm.js';
import { bindFacadeThreadEnvironment } from '../../../apps/web/src/host/engine-clock-env.ts';
import { createFacadeSharedMemory, instantiateFacadeModule } from '../../../apps/web/src/host/facade-wasm-module.ts';
import { createFacadeEngineHandshake } from '../../../apps/web/src/host/facade-engine-handshake.ts';
const trace = new BroadcastChannel('wasm-full-host-fixture');
self.addEventListener('unhandledrejection', event => trace.postMessage({type:'unhandled',message:String(event.reason)}));
self.onmessage = async ({data}) => {
  if (data.type !== 'bindEngineTransport') throw new Error('未知 engine transport');
  const handshake = createFacadeEngineHandshake({port:data.controlPort,onFailure(error) { throw error; }});
  const memory = createFacadeSharedMemory({isolated:self.crossOriginIsolated});
  await handshake.registerMemory(memory); trace.postMessage({type:"memoryAck"});
  bindFacadeThreadEnvironment({memory,role:'engine',postControl:message => handshake.postControl(message)});
  const module = await WebAssembly.compileStreaming(fetch(new URL('./pkg/web_wasm_bg.wasm', import.meta.url)));
  instantiateFacadeModule(glue,module,memory); trace.postMessage({type:"instantiated"});
  const primary = glue.wasm_tombstone_init_primary();
  await handshake.registerDeath(primary); trace.postMessage({type:"deathAck"});
  setTimeout(() => glue.verification_tombstone_primary_panic(),0);
};
