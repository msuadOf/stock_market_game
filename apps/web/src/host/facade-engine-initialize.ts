import type { FacadeSlotLayout, FacadeThreadConfiguration } from "./facade-thread-binding.ts";
import type { FacadeArenaView } from "./facade-supervisor.ts";
import type { FacadeSubstrate } from "./engine-supervisor.ts";
import { createFacadeEngineHandshake } from "./facade-engine-handshake.ts";
import { createFacadeSharedMemory, instantiateFacadeModule, type FacadeSyncInitializer } from "./facade-wasm-module.ts";

export interface FacadeEngineGlue extends FacadeSyncInitializer {
  readonly facade_slot_init_for_setup: (setup: unknown) => void;
  readonly facade_slot_layout: () => FacadeSlotLayout;
}
interface EngineEnvironment {
  bindFacadeThreadEnvironment(input: FacadeThreadConfiguration): void;
  installFacadeSlotLayout(layout: FacadeSlotLayout): void;
  getArenaView(): FacadeArenaView;
}
/** engine 首实例初始化；setup 仅保存一份，Rust 取得真实默认拓扑并先锚 clock。 */
export function createFacadeEngineInitializer(options: {
  readonly isolated: () => boolean;
  readonly createMemory?: () => WebAssembly.Memory;
  readonly environment: EngineEnvironment;
  readonly load: () => Promise<{ readonly module: WebAssembly.Module; readonly diagnostics: boolean; readonly glue: FacadeEngineGlue }>;
  readonly onFailure: (error: unknown) => void;
}) {
  let started = false, disposed = false;
  let setup: unknown;
  let current: FacadeSubstrate | undefined;
  let ownedPort: MessagePort | undefined;
  let handshake: ReturnType<typeof createFacadeEngineHandshake> | undefined;
  function dispose(): void {
    if (disposed) return;
    disposed = true; current = undefined; setup = undefined;
    handshake?.dispose(); handshake = undefined;
  }
  return {
    async initialize(value: unknown): Promise<FacadeSubstrate> {
      let receivedPort: MessagePort | undefined;
      try {
        if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Error("engine init 必须为对象");
        const source = value as Record<string, unknown>;
        if (source.controlPort instanceof MessagePort) receivedPort = source.controlPort;
        if (started || disposed) throw new Error("engine init 重复或已 dispose；每个 Worker 只允许一次");
        started = true;
        if (source.type !== "init" || receivedPort === undefined || !Object.hasOwn(source, "setup") || Object.keys(source).some((key) => !["type", "setup", "controlPort"].includes(key))) throw new Error("engine init 缺少合法 controlPort/setup 或含未知字段");
        handshake = createFacadeEngineHandshake({ port: receivedPort, onFailure: (error) => { dispose(); options.onFailure(error); } });
        ownedPort = receivedPort;
        // 此处仍在 Worker 本地，克隆失败直接交唯一 reporter，禁止 String 降格。
        setup = structuredClone(source.setup);
        const isolated = options.isolated();
        if (isolated !== true) throw new Error("engine 需要 crossOriginIsolated 隔离环境");
        const memory = options.createMemory === undefined ? createFacadeSharedMemory({ isolated }) : options.createMemory();
        options.environment.bindFacadeThreadEnvironment({ memory, role: "engine", postControl: handshake.postControl });
        const loaded = await options.load();
        if (disposed) throw new Error("engine 加载完成时实例已 dispose");
        instantiateFacadeModule(loaded.glue, loaded.module, memory);
        loaded.glue.facade_slot_init_for_setup(setup);
        const layout = loaded.glue.facade_slot_layout();
        options.environment.installFacadeSlotLayout(layout); options.environment.getArenaView();
        const candidate = Object.freeze({ module: loaded.module, memory, layout, diagnostics: loaded.diagnostics });
        await handshake.configure(candidate);
        if (disposed) throw new Error("engine configured 到达时实例已 dispose");
        current = candidate;
        return candidate;
      } catch (error) {
        if (receivedPort !== undefined && receivedPort !== ownedPort) receivedPort.close();
        dispose(); throw error;
      }
    },
    requireConfigured(): { readonly substrate: FacadeSubstrate; readonly setup: unknown } {
      if (disposed || current === undefined || handshake === undefined) throw new Error("engine 尚未 configured 或已 dispose");
      handshake.assertConfigured(); return { substrate: current, setup };
    },
    dispose,
  };
}
