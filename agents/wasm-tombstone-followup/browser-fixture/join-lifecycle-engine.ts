import * as glue from "./pkg/web_wasm.js";
import { bindFacadeThreadEnvironment, installFacadeSlotLayout } from "../../../apps/web/src/host/engine-clock-env.ts";
import { createFacadeEngineHandshake } from "../../../apps/web/src/host/facade-engine-handshake.ts";
import { createFacadeSharedMemory, instantiateFacadeModule } from "../../../apps/web/src/host/facade-wasm-module.ts";

type Command = { type: "start"; controlPort: MessagePort } | { type: "spawn"; input: number } | { type: "join" | "quiesce" };
let stage: "cold" | "starting" | "ready" | "spawned" | "joined" | "quiescent" = "cold";
self.addEventListener("message", (event: MessageEvent<Command>) => {
  const command = event.data;
  void (async () => {
    if (command.type === "start" && stage === "cold") {
      stage = "starting";
      const handshake = createFacadeEngineHandshake({ port: command.controlPort, onFailure(error) { throw error; } });
      const memory = createFacadeSharedMemory({ isolated: self.crossOriginIsolated });
      await handshake.registerMemory(memory);
      bindFacadeThreadEnvironment({ memory, role: "engine", postControl: (message) => handshake.postControl(message) });
      const module = await WebAssembly.compileStreaming(fetch("/agents/wasm-tombstone-followup/browser-fixture/pkg/web_wasm_bg.wasm"));
      instantiateFacadeModule(glue, module, memory);
      glue.wasm_tombstone_init_primary();
      glue.facade_slot_init(4);
      const layout = glue.facade_slot_layout();
      installFacadeSlotLayout(layout);
      // 本夹具的强制退出来自 owner；不登记 panic 宽限 watcher，不伪造 panic/DEAD。
      await handshake.configure({ module, memory, layout, diagnostics: false });
      stage = "ready";
      self.postMessage({ type: "ready" });
    } else if (command.type === "spawn" && stage === "ready") {
      const joinIndex = glue.verification_worker_handle_spawn(command.input);
      stage = "spawned";
      self.postMessage({ type: "spawned", joinIndex });
    } else if (command.type === "join" && stage === "spawned") {
      const value = glue.verification_worker_handle_join();
      let duplicate: string;
      try {
        glue.verification_worker_handle_join();
        duplicate = "错误：重复 join 成功";
      } catch (error) {
        duplicate = String(error);
      }
      stage = "joined";
      self.postMessage({ type: "joined", value, duplicate });
    } else if (command.type === "quiesce" && stage === "joined") {
      // 本 engine 只有上述两个 verification 调用路径；此后禁止再调用任何 WASM。
      stage = "quiescent";
      self.postMessage({ type: "quiescent" });
    } else {
      throw new Error(`join 生命周期夹具收到非法阶段命令 ${stage}/${command.type}`);
    }
  })().catch((error) => self.postMessage({ type: "fixture-error", message: String(error) }));
});
