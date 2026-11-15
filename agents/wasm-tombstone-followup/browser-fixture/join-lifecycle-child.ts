import { createFacadeChildBootstrap } from "../../../apps/web/src/host/facade-child-bootstrap.ts";
import { bindFacadeThreadEnvironment, getArenaView, installFacadeSlotLayout, requireExitIntent } from "../../../apps/web/src/host/engine-clock-env.ts";

// 独立的测试用 SAB（不在产品 arena）：[闸门命令, 进入 destroy 包装器, 调用 raw destroy, raw 成功返回]。
let gate: Int32Array | undefined;
const bootstrap = createFacadeChildBootstrap({
  environment: { bindFacadeThreadEnvironment, installFacadeSlotLayout, getArenaView, requireExitIntent },
  async loadGlue() {
    const glue = await import("./pkg/web_wasm.js");
    return {
      facade_child_trampoline: glue.facade_child_trampoline,
      facade_slot_layout: glue.facade_slot_layout,
      initSync(options) {
        const raw = glue.initSync(options);
        return {
          memory: raw.memory,
          __wbindgen_thread_destroy(tls, stack, stackSize) {
            const words = gate;
            if (words === undefined || Atomics.add(words, 1, 1) !== 0) {
              throw new Error("destroy 闸门必须绑定且只进入一次");
            }
            // 已经走过真实 trampoline 与退出意图校验。通知 owner 后停在实际 raw 调用之前。
            self.postMessage({ type: "join-lifecycle-gate-entered" });
            while (Atomics.load(words, 0) === 0) {
              if (Atomics.wait(words, 0, 0, 15000) === "timed-out") {
                throw new Error("等待测试端打开 destroy 闸门超时");
              }
            }
            if (Atomics.load(words, 0) === 2) throw new Error("JOIN_LIFECYCLE_CLEANUP_FAILURE_PROBE");
            if (Atomics.load(words, 0) !== 1) throw new Error("未知 destroy 闸门命令");
            if (Atomics.add(words, 2, 1) !== 0) throw new Error("raw destroy 被重复执行");
            raw.__wbindgen_thread_destroy(tls, stack, stackSize);
            Atomics.add(words, 3, 1);
          },
        };
      },
    };
  },
  postControl(message, input) { input.controlPort.postMessage(message); },
  postValidationFailure(message) { self.postMessage(message); },
  reportError(error) { queueMicrotask(() => { throw error; }); },
  close() { self.close(); },
});

self.addEventListener("message", (event) => {
  if (event.data?.type === "join-lifecycle-gate") {
    if (gate !== undefined || !(event.data.signal instanceof SharedArrayBuffer) || event.data.signal.byteLength !== 16) {
      throw new Error("无效或重复的 join 生命周期闸门");
    }
    gate = new Int32Array(event.data.signal);
  } else {
    void bootstrap.handleMessage(event.data);
  }
});
