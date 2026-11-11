import * as glue from './pkg/web_wasm_diagnostic.js';
import { DEFAULT_SETUP } from '../../../apps/web/src/config/defaults.ts';
import { bindFacadeThreadEnvironment, installFacadeSlotLayout } from '../../../apps/web/src/host/engine-clock-env.ts';
import { createFacadeSharedMemory, instantiateFacadeModule } from '../../../apps/web/src/host/facade-wasm-module.ts';
import { createFacadeEngineHandshake } from '../../../apps/web/src/host/facade-engine-handshake.ts';
import { measureGame } from './measure.mjs';
import { setupHash } from './setup-hash.mjs';
import { initializeDiagnosticWait } from './wait-config.mjs';

let configuredFineWaitUs: number | undefined;
let configuredCheckShutdown = false;
let configuredMathMode: 'std' | 'libm' = 'std';
let configuredProfile = false;

function plain(value: unknown): unknown {
  if (value instanceof Map) return Object.fromEntries([...value].map(([key, entry]) => [key, plain(entry)]));
  if (Array.isArray(value)) return value.map(plain);
  return value;
}

self.onmessage = async ({data}) => {
  try {
    if (data.type === 'start') {
      const memory = createFacadeSharedMemory({isolated: self.crossOriginIsolated});
      const handshake = createFacadeEngineHandshake({port:data.controlPort, onFailure:error => self.postMessage({type:'error', message:String(error)})});
      await handshake.registerMemory(memory);
      bindFacadeThreadEnvironment({memory, role:'engine', postControl:handshake.postControl});
      const module = await WebAssembly.compileStreaming(fetch(new URL('./pkg/web_wasm_diagnostic_bg.wasm', import.meta.url)));
      instantiateFacadeModule(glue, module, memory);
      const upstreamMode = glue.diagnostic_upstream_mode();
      if (upstreamMode !== data.upstreamMode) throw Error(`上行 feature 不匹配：期望 ${data.upstreamMode}，实际 ${upstreamMode}`);
      const workerMode = glue.diagnostic_worker_mode();
      if (workerMode !== data.workerMode) throw Error(`下行 feature 不匹配：期望 ${data.workerMode}，实际 ${workerMode}`);
      const primary = glue.wasm_tombstone_init_primary();
      await handshake.registerDeath(primary);
      glue.set_step_runtime_override(true);
      configuredFineWaitUs = data.fineWaitUs;
      configuredCheckShutdown = data.checkShutdown === true;
      if (data.mathMode !== 'std' && data.mathMode !== 'libm') throw Error('未知观察概率数学模式');
      configuredMathMode = data.mathMode;
      configuredProfile = data.profile === true;
      initializeDiagnosticWait(glue, configuredFineWaitUs);
      glue.facade_slot_init_for_setup(DEFAULT_SETUP);
      const layout = glue.facade_slot_layout();
      installFacadeSlotLayout(layout);
      const waitDeath = glue.wasm_wait_death_layout();
      const waitAbort = glue.wasm_wait_abort_layout();
      const libosFault = glue.diagnostic_libos_fault_layout();
      await handshake.configure({module, memory, layout, diagnostics:false, libosFault}, waitAbort, waitDeath);
      self.postMessage({type:'ready', upstreamMode});
    } else if (data.type === 'fault-shutdown') {
      const shutdown = plain(glue.diagnostic_game_shutdown());
      self.postMessage({type:'shutdown-result', shutdown});
    } else if (data.type === 'fault-go') {
      if (configuredMathMode === 'libm') glue.diagnostic_game_create_with_candidate(DEFAULT_SETUP, 42n);
      else glue.diagnostic_game_create(DEFAULT_SETUP, 42n);
      self.postMessage({type:'executing'});
      let stepError;
      try { glue.diagnostic_game_steps(3000); }
      catch (error) { stepError = String(error); }
      const probe = Object.fromEntries(glue.diagnostic_game_failure_probe());
      self.postMessage({type:'fault-result', stepError, probe});
    } else if (data.type === 'go') {
      const setupSha256 = await setupHash(DEFAULT_SETUP, crypto.subtle);
      if (configuredMathMode === 'libm') glue.diagnostic_game_create_with_candidate(DEFAULT_SETUP, 42n);
      else glue.diagnostic_game_create(DEFAULT_SETUP, 42n);
      let stepProfile: unknown;
      const measurement = measureGame(glue, () => performance.now(), {
        onSampleStart: () => { if (configuredProfile) glue.diagnostic_step_profile_start(); self.postMessage({type:'cpu-sample-start'}); },
        onSampleEnd: () => { self.postMessage({type:'cpu-sample-end'}); if (configuredProfile) stepProfile = JSON.parse(glue.diagnostic_step_profile_take()); },
      });
      const trace = JSON.parse(glue.diagnostic_game_field_trace());
      if (trace.businessHash !== measurement.businessHash) throw Error('追踪摘要与业务摘要不一致');
      const shutdown = configuredCheckShutdown ? plain(glue.diagnostic_game_shutdown()) : undefined;
      self.postMessage({type:'result', seed:42, setupSha256, mathMode:configuredMathMode, stepProfile, fineWaitUs:configuredFineWaitUs, ...measurement, shutdown, fieldTrace:trace.fieldTrace, fieldSamples:trace.fieldSamples, accountRoster:trace.accountRoster, observationTrace:trace.observationTrace, candidateTrace:trace.candidateTrace});
    }
  } catch (error) {
    self.postMessage({type:'error', message:String(error), stack:error instanceof Error ? error.stack : undefined});
  }
};
