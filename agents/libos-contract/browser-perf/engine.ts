import * as glue from './pkg/web_wasm.js';
import { bindFacadeThreadEnvironment, installFacadeSlotLayout } from '../../../apps/web/src/host/engine-clock-env.ts';
import { createFacadeSharedMemory, instantiateFacadeModule } from '../../../apps/web/src/host/facade-wasm-module.ts';

const exports = glue as unknown as {
  verification_libos_boundary_cost?: (name: string, wrapped: boolean, operations: number) => string;
  verification_libos_boundary_timer_resolution?: () => number;
  start_wasm_reclaimer?: () => void;
  close_wasm_reclaimer?: () => boolean;
};

self.onmessage = async ({ data }) => {
  try {
    if (data.type === 'start') {
      const memory = createFacadeSharedMemory({ isolated: self.crossOriginIsolated });
      bindFacadeThreadEnvironment({ memory, role: 'engine', postControl: message => data.controlPort.postMessage(message) });
      const module = await WebAssembly.compileStreaming(fetch('/agents/libos-contract/browser-perf/pkg/web_wasm_bg.wasm'));
      instantiateFacadeModule(glue, module, memory);
      glue.wasm_tombstone_init_primary();
      glue.facade_slot_init(8);
      const layout = glue.facade_slot_layout();
      installFacadeSlotLayout(layout);
      glue.init_wasm_wait(0, 0, 100, 1000);
      if (typeof exports.verification_libos_boundary_cost !== 'function' ||
          typeof exports.verification_libos_boundary_timer_resolution !== 'function') {
        throw new Error('缺少性能验证制品导出');
      }
      const workerResolutionNs = exports.verification_libos_boundary_timer_resolution();
      self.postMessage({ type: 'ready', module, memory, layout, workerResolutionNs });
    } else if (data.type === 'sample') {
      if (typeof exports.verification_libos_boundary_cost !== 'function') throw new Error('性能验证制品未就绪');
      const reclaimer = data.name === 'deferred' && data.wrapped;
      if (reclaimer) {
        if (typeof exports.start_wasm_reclaimer !== 'function' || typeof exports.close_wasm_reclaimer !== 'function') {
          throw new Error('异步析构性能未验证：缺少生产 owner start/close；不允许 Inline 代替排队');
        }
        exports.start_wasm_reclaimer();
      }
      let result;
      try {
        result = JSON.parse(exports.verification_libos_boundary_cost(data.name, data.wrapped, data.operations));
      } finally {
        if (reclaimer && exports.close_wasm_reclaimer?.() !== true) throw new Error('需要实际 drain/join 完成');
      }
      self.postMessage({ type: 'success', result });
    } else {
      throw new Error('未知性能夹具请求');
    }
  } catch (error) {
    self.postMessage({ type: 'failure', error: String(error) });
  }
};
