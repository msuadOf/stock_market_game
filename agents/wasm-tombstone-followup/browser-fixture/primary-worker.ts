import * as glue from './pkg/web_wasm.js';
import { bindFacadeThreadEnvironment } from '../../../apps/web/src/host/engine-clock-env.ts';
import { createFacadeSharedMemory, instantiateFacadeModule } from '../../../apps/web/src/host/facade-wasm-module.ts';
self.onmessage = async ({data}) => {
  if (data.type === 'engine') {
    const memory = createFacadeSharedMemory({isolated:self.crossOriginIsolated});
    bindFacadeThreadEnvironment({memory,role:'engine',postControl() { throw new Error('primary fixture 禁止 spawn'); }});
    const module = await WebAssembly.compileStreaming(fetch('/agents/wasm-tombstone-followup/browser-fixture/pkg/web_wasm_bg.wasm'));
    instantiateFacadeModule(glue,module,memory);
    const primary = glue.wasm_tombstone_init_primary();
    self.postMessage({type:'ready',memory,module,primary});
  } else if (data.type === 'ingress') {
    bindFacadeThreadEnvironment({memory:data.memory,role:'ingress',postControl() { throw new Error('primary fixture 禁止 spawn'); }});
    instantiateFacadeModule(glue,data.module,data.memory);
    let invalidIdentity = false, invalidAddress = false, duplicate = false;
    try { glue.wasm_tombstone_bind_ingress(data.primary.ingress.address, 1); } catch { invalidIdentity = true; }
    try { glue.wasm_tombstone_bind_ingress(data.primary.engine.address, data.primary.ingress.workerId); } catch { invalidAddress = true; }
    glue.wasm_tombstone_bind_ingress(data.primary.ingress.address, data.primary.ingress.workerId);
    try { glue.wasm_tombstone_bind_ingress(data.primary.ingress.address, data.primary.ingress.workerId); } catch { duplicate = true; }
    self.postMessage({type:'bound',invalidIdentity,invalidAddress,duplicate});
  } else if (data.type === 'panic') {
    try { glue.verification_tombstone_primary_panic(); throw new Error('panic 意外返回'); }
    catch (error) { self.postMessage({type:'trap',message:String(error)}); }
  }
};
