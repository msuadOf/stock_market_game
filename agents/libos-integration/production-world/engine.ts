import * as glue from './pkg/web_wasm.js';
import { DEFAULT_SETUP } from '../../../apps/web/src/config/defaults.ts';
import { createFacadeEngineInitializer } from '../../../apps/web/src/host/facade-engine-initialize.ts';
import { bindFacadeThreadEnvironment, installFacadeSlotLayout, getArenaView } from '../../../apps/web/src/host/engine-clock-env.ts';

const initializer = createFacadeEngineInitializer({
  isolated: () => self.crossOriginIsolated,
  environment: {bindFacadeThreadEnvironment, installFacadeSlotLayout, getArenaView},
  load: async () => ({module: await WebAssembly.compileStreaming(fetch(new URL('./pkg/web_wasm_bg.wasm', import.meta.url))), diagnostics:false, glue}),
  onFailure: error => self.postMessage({type:'failure', error:String(error)}),
});
function plain(value: unknown): unknown {
  if (value instanceof Map) return Object.fromEntries([...value].map(([key, entry]) => [key, plain(entry)]));
  if (Array.isArray(value)) return value.map(plain);
  return value;
}
self.onmessage = async ({data}) => {
  try {
    if (data.type === 'start') {
      const substrate = await initializer.initialize({type:'init', controlPort:data.controlPort, setup:DEFAULT_SETUP,
        runtimeOverride:true, waitSettings:{spinUs:500,quietUs:50000,fineUs:500,coarseUs:500}});
      const layout = substrate.libosFault as {hostReadyAddress:number};
      const hostReady = Atomics.load(new Int32Array(substrate.memory.buffer), layout.hostReadyAddress / 4);
      self.postMessage({type:'ready', hostReady});
    } else if (data.type === 'go') {
      const handle = glue.create_session(DEFAULT_SETUP, 42n);
      for (let index=0; index<10; index++) glue.step(handle);
      const shutdown = plain(glue.shutdown_session(handle));
      self.postMessage({type:'result', ticks:10, shutdown});
    }
  } catch (error) { self.postMessage({type:'failure', error:String(error)}); }
};
