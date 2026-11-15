import * as glue from './pkg/web_wasm.js';
import { bindFacadeThreadEnvironment, installFacadeSlotLayout } from '../../../apps/web/src/host/engine-clock-env.ts';
import { createFacadeSharedMemory, instantiateFacadeModule } from '../../../apps/web/src/host/facade-wasm-module.ts';
let mode = 0;
let fixture = 'child-death';
// 仅验证制品暴露这个入口；正式 WASM 包不得用来跑本场景。
function deferredLifecycleProbe(stage: string): string {
  const { verification_deferred_lifecycle_contract: probe } = glue as unknown as {
    verification_deferred_lifecycle_contract?: (stage: string) => string;
  };
  if (typeof probe !== 'function') throw new Error('缺少 verification_deferred_lifecycle_contract(stage) 测试专用导出');
  return probe(stage);
}
self.onmessage = async ({data}) => {
  if (data.type === 'start') {
    mode = data.mode;
    fixture = data.fixture;
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
  } else if (data.type === 'deferred-fault-go' && fixture === 'deferred-lifecycle' && mode === 3) {
    try {
      deferredLifecycleProbe('release-consumer-panic');
    } catch (error) {
      self.postMessage({type:'verification-error',message:String(error)});
    }
  } else if (data.type === 'go') {
    if (fixture === 'deferred-lifecycle') {
      try {
        const stages: Record<number, string> = {1:'saturation', 2:'close', 3:'arm-consumer-panic', 4:'spawn-failure'};
        if (!Object.hasOwn(stages, mode)) throw new Error('未知 LibOS 析构合同模式');
        const witness = JSON.parse(deferredLifecycleProbe(stages[mode]));
        // 当前真实还是 Inline；未入队时绝不向主线程谎报已 armed，也绝不让 engine 自己 panic。
        if (witness?.kind === 'observed-submissions') {
          self.postMessage({type:'deferred-submissions-observed',mode,witness});
        } else {
          self.postMessage({type:mode === 3 ? 'deferred-fault-armed' : 'verification-result',mode,witness});
        }
      } catch (error) {
        self.postMessage({type:'verification-error',message:String(error)});
      }
      return;
    }
    if (fixture === 'deferred-drop') {
      try {
        const reclamation = JSON.parse(glue.verification_deferred_reclamation());
        self.postMessage({type:'verification-result',reclamation});
      } catch (error) {
        self.postMessage({type:'verification-error',message:String(error)});
      }
      return;
    }
    if (fixture === 'watchdog-contract') {
      try {
        const address = glue.verification_watchdog_wait_prepare(mode);
        const detail = glue.verification_watchdog_wait_boundary(mode);
        self.postMessage({type:'verification-result',address,detail});
      } catch (error) {
        self.postMessage({type:'verification-error',message:String(error)});
      }
      return;
    }
    if (fixture === 'world-stop' || fixture === 'watchdog') {
      try {
        const address = fixture === 'world-stop'
          ? glue.verification_world_stop_prepare(mode)
          : glue.verification_watchdog_prepare(mode);
        const detail = fixture === 'world-stop'
          ? glue.verification_world_stop_run(mode)
          : glue.verification_watchdog_run(mode);
        self.postMessage({type:'verification-result',address,detail});
      } catch (error) {
        self.postMessage({type:'verification-error',message:String(error)});
      }
      return;
    }
    const joinIndex = glue.verification_facade_spawn_death(mode);
    self.postMessage({type:'spawned',joinIndex});
    try { glue.verification_facade_join(); self.postMessage({type:'unexpected-join-success'}); }
    catch (error) { self.postMessage({type:'join-failed', message:String(error)}); }
  }
};
