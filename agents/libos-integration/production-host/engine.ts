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
self.onmessage = async ({data}) => {
  try {
    const substrate = await initializer.initialize({type:'init', controlPort:data.controlPort, setup:DEFAULT_SETUP,
      runtimeOverride:true, waitSettings:{spinUs:500,quietUs:50000,fineUs:500,coarseUs:500}});
    const layout = substrate.libosFault as {hostReadyAddress:number, globalStateAddress:number, domainAbortAddress:number, slots:{abortAddress:number, waitAddress:number}[]};
    if (!layout || !Array.isArray(layout.slots) || layout.slots.length === 0) throw Error('正式故障布局缺失');
    const hostReady = Atomics.load(new Int32Array(substrate.memory.buffer), layout.hostReadyAddress / 4);
    self.postMessage({type:'ready', hostReady, slots:layout.slots.length, memory:substrate.memory, layout});
  } catch (error) { self.postMessage({type:'failure', error:String(error)}); }
};
