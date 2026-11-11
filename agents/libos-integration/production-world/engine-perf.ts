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
      const setupBytes = new TextEncoder().encode(JSON.stringify(DEFAULT_SETUP));
      const setupDigest = Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256',setupBytes)), byte=>byte.toString(16).padStart(2,'0')).join('');
      const handle = glue.create_session(DEFAULT_SETUP, 42n);
      for (let index=0; index<1000; index++) glue.step(handle);
      const started = performance.now();
      for (let index=0; index<3000; index++) glue.step(handle);
      const elapsedMs = performance.now() - started;
      const snapshot = glue.snapshot(handle);
      const serialized = JSON.stringify(snapshot, (_key,value) => {
        if (typeof value === 'bigint') return value.toString();
        if (value instanceof Map) return Object.fromEntries([...value].sort(([a],[b]) => String(a).localeCompare(String(b))));
        return value;
      });
      if (typeof serialized !== 'string') throw Error('产品 snapshot 不可序列化');
      const snapshotDigest = Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256',new TextEncoder().encode(serialized))), byte=>byte.toString(16).padStart(2,'0')).join('');
      const shutdown = plain(glue.shutdown_session(handle));
      self.postMessage({type:'result', seed:'42', setupDigest, warm:1000, ticks:3000, elapsedMs, snapshotDigest, shutdown});
    }
  } catch (error) { self.postMessage({type:'failure', error:String(error)}); }
};
