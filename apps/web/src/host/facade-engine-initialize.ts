import { validateDayEndArchive } from "../save/day-end-candidate.ts";
import { parseSaveSlot } from "../save/save-schema.ts";
import type { StrictSaveEnvelope } from "../save/schema/root.ts";
import type { FacadeSlotLayout, FacadeThreadConfiguration } from "./facade-thread-binding.ts";
import type { FacadeArenaView } from "./facade-supervisor.ts";
import type { FacadeSubstrate } from "./engine-supervisor.ts";
import { createFacadeEngineHandshake } from "./facade-engine-handshake.ts";
import { createFacadeSharedMemory, instantiateFacadeModule, type FacadeSyncInitializer } from "./facade-wasm-module.ts";

export interface FacadeEngineGlue extends FacadeSyncInitializer {
  readonly facade_slot_init_for_setup: (setup: unknown) => void;
  readonly facade_slot_init_for_restore_json?: (saveJson: string) => void;
  readonly facade_slot_layout: () => FacadeSlotLayout;
}
interface EngineEnvironment {
  bindFacadeThreadEnvironment(input: FacadeThreadConfiguration): void;
  installFacadeSlotLayout(layout: FacadeSlotLayout): void;
  getArenaView(): FacadeArenaView;
}
/** new 使用 setup 默认拓扑，restore 消费唯一完整 X；均由 Rust 初始化 arena 并锚 clock。 */
export function createFacadeEngineInitializer(options: {
  readonly isolated: () => boolean;
  readonly createMemory?: () => WebAssembly.Memory;
  readonly environment: EngineEnvironment;
  readonly load: () => Promise<{ readonly module: WebAssembly.Module; readonly diagnostics: boolean; readonly glue: FacadeEngineGlue }>;
  readonly onFailure: (error: unknown) => void;
}) {
  let started = false, disposed = false;
  let setup: unknown;
  let initialArchive: StrictSaveEnvelope | null = null;
  let current: FacadeSubstrate | undefined;
  let ownedPort: MessagePort | undefined;
  let handshake: ReturnType<typeof createFacadeEngineHandshake> | undefined;
  function dispose(): void {
    if (disposed) return;
    disposed = true; current = undefined; setup = undefined; initialArchive = null;
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
        const hasSetup = Object.hasOwn(source, "setup"), hasArchive = Object.hasOwn(source, "initialArchive");
        if (source.type !== "init" || receivedPort === undefined || hasSetup === hasArchive
          || Object.keys(source).some((key) => !["type", "setup", "initialArchive", "controlPort"].includes(key))) {
          throw new Error("engine init 需要合法 controlPort 与互斥的唯一 setup 或 initialArchive，禁止未知字段");
        }
        handshake = createFacadeEngineHandshake({ port: receivedPort, onFailure: (error) => { dispose(); options.onFailure(error); } });
        ownedPort = receivedPort;
        // 此处仍在 Worker 本地，克隆失败直接交唯一 reporter，禁止 String 降格。
        if (hasArchive) {
          initialArchive = validateDayEndArchive(parseSaveSlot(structuredClone(source.initialArchive)));
          setup = initialArchive.setup;
        } else setup = structuredClone(source.setup);
        const isolated = options.isolated();
        if (isolated !== true) throw new Error("engine 需要 crossOriginIsolated 隔离环境");
        const memory = options.createMemory === undefined ? createFacadeSharedMemory({ isolated }) : options.createMemory();
        options.environment.bindFacadeThreadEnvironment({ memory, role: "engine", postControl: handshake.postControl });
        const loaded = await options.load();
        if (disposed) throw new Error("engine 加载完成时实例已 dispose");
        instantiateFacadeModule(loaded.glue, loaded.module, memory);
        if (initialArchive !== null) {
          const initializeRestore = loaded.glue.facade_slot_init_for_restore_json;
          if (typeof initializeRestore !== "function") throw Object.assign(new Error("facade glue 缺少 String restore topology initializer"), {
            code: "WASM_RESTORE_TOPOLOGY_NOT_READY", where: "facade-engine-initialize.initial-archive", context: { operation: "restore" },
          });
          initializeRestore(JSON.stringify(initialArchive));
        } else loaded.glue.facade_slot_init_for_setup(setup);
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
    requireConfigured(): { readonly substrate: FacadeSubstrate; readonly setup: unknown; readonly initialArchive: StrictSaveEnvelope | null } {
      if (disposed || current === undefined || handshake === undefined) throw new Error("engine 尚未 configured 或已 dispose");
      handshake.assertConfigured(); return { substrate: current, setup, initialArchive };
    },
    dispose,
  };
}
