import { validateDayEndArchive } from "../save/day-end-candidate.ts";
import { parseSaveSlot } from "../save/save-schema.ts";
import type { StrictSaveEnvelope } from "../save/schema/root.ts";
import type { FacadeSlotLayout, FacadeThreadConfiguration } from "./facade-thread-binding.ts";
import type { FacadeArenaView } from "./facade-supervisor.ts";
import type { FacadeSubstrate } from "./engine-supervisor.ts";
import { createFacadeEngineHandshake } from "./facade-engine-handshake.ts";
import { createFacadeSharedMemory, instantiateFacadeModule, type FacadeSyncInitializer } from "./facade-wasm-module.ts";
import { parseWasmWaitAbortLayout, type WasmWaitAbortLayout } from "./wasm-wait-abort.ts";

export interface FacadeEngineGlue extends FacadeSyncInitializer {
  readonly facade_slot_init_for_setup: (setup: unknown) => void;
  readonly facade_slot_init_for_restore_json?: (saveJson: string) => void;
  readonly clear_prepared_restore?: () => void;
  readonly facade_slot_layout: () => FacadeSlotLayout;
  readonly set_step_runtime_override?: (enabled: boolean) => void;
  readonly init_wasm_wait?: (spinUs: number, quietUs: number, fineUs: number, coarseUs: number) => void;
  readonly wasm_wait_abort_layout?: () => unknown;
}
interface FacadeWaitSettings {
  readonly spinUs: number;
  readonly quietUs: number;
  readonly fineUs: number;
  readonly coarseUs: number;
}
function parseRuntimeOptions(source: Record<string, unknown>): { readonly requested: boolean; readonly enabled?: boolean; readonly wait?: FacadeWaitSettings } {
  const hasOverride = Object.hasOwn(source, "runtimeOverride");
  const hasWait = Object.hasOwn(source, "waitSettings");
  if (!hasOverride) {
    if (hasWait) throw new Error("engine init waitSettings 必须与 runtimeOverride: true 同时提供");
    return { requested: false };
  }
  const runtimeOverride = source.runtimeOverride;
  if (typeof runtimeOverride !== "boolean") throw new Error("engine init runtimeOverride 必须为 boolean");
  if (runtimeOverride === false) {
    if (hasWait) throw new Error("runtimeOverride: false 不得提供 waitSettings");
    return { requested: true, enabled: false };
  }
  if (!hasWait) throw new Error("runtimeOverride: true 必须提供自有 waitSettings 字段");
  const rawWait = source.waitSettings;
  if (rawWait === null || typeof rawWait !== "object" || Array.isArray(rawWait)) {
    throw new Error("runtimeOverride: true 必须提供 waitSettings 对象");
  }
  const wait = rawWait as Record<string, unknown>;
  const keys = ["spinUs", "quietUs", "fineUs", "coarseUs"];
  if (Object.keys(wait).length !== keys.length || keys.some((key) => !Object.hasOwn(wait, key))) {
    throw new Error("waitSettings 必须且只能包含 spinUs、quietUs、fineUs、coarseUs");
  }
  const snapshot: Record<string, number> = {};
  for (const key of keys) {
    const value = wait[key];
    if (typeof value !== "number" || !Number.isInteger(value) || value < 0 || value > 0xffff_ffff) {
      throw new Error(`waitSettings.${key} 必须为 u32`);
    }
    if ((key === "fineUs" || key === "coarseUs") && value === 0) {
      throw new Error(`waitSettings.${key} 必须大于零`);
    }
    snapshot[key] = value;
  }
  return { requested: true, enabled: true, wait: snapshot as unknown as FacadeWaitSettings };
}
interface EngineEnvironment {
  bindFacadeThreadEnvironment(input: FacadeThreadConfiguration): void;
  installFacadeSlotLayout(layout: FacadeSlotLayout): void;
  getArenaView(): FacadeArenaView;
}
/** new 使用 setup 默认拓扑，restore 消费唯一完整 X；均由 Rust 初始化 arena 并锚 clock。 */
export function createFacadeEngineInitializer(options: {
  readonly isolated: () => boolean;
  readonly ownsIncomingArchive?: boolean;
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
  let clearPrepared: (() => void) | undefined;
  let handshake: ReturnType<typeof createFacadeEngineHandshake> | undefined;
  function dispose(): void {
    if (disposed) return;
    disposed = true; current = undefined; setup = undefined; initialArchive = null;
    clearPrepared?.(); clearPrepared = undefined;
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
          || Object.keys(source).some((key) => !["type", "setup", "initialArchive", "controlPort", "runtimeOverride", "waitSettings"].includes(key))) {
          throw new Error("engine init 需要合法 controlPort 与互斥的唯一 setup 或 initialArchive，禁止未知字段");
        }
        const runtime = parseRuntimeOptions(source);
        handshake = createFacadeEngineHandshake({ port: receivedPort, onFailure: (error) => { dispose(); options.onFailure(error); } });
        ownedPort = receivedPort;
        // Worker 的 MessagePort 已创建独占副本；直接调用方仍须克隆以隔离嵌套引用。
        if (hasArchive) {
          initialArchive = validateDayEndArchive(parseSaveSlot(options.ownsIncomingArchive === true ? source.initialArchive : structuredClone(source.initialArchive)));
          setup = initialArchive.setup;
        } else setup = structuredClone(source.setup);
        const isolated = options.isolated();
        if (isolated !== true) throw new Error("engine 需要 crossOriginIsolated 隔离环境");
        const memory = options.createMemory === undefined ? createFacadeSharedMemory({ isolated }) : options.createMemory();
        await handshake.registerMemory(memory);
        if (disposed) throw new Error("Memory 登记后实例已 dispose");
        options.environment.bindFacadeThreadEnvironment({ memory, role: "engine", postControl: handshake.postControl });
        const loaded = await options.load();
        if (disposed) throw new Error("engine 加载完成时实例已 dispose");
        instantiateFacadeModule(loaded.glue, loaded.module, memory);
        let waitAbort: WasmWaitAbortLayout | undefined;
        if (runtime.requested) {
          const setRuntimeOverride = loaded.glue.set_step_runtime_override;
          const initializeWait = runtime.enabled ? loaded.glue.init_wasm_wait : undefined;
          const abortLayout = runtime.enabled ? loaded.glue.wasm_wait_abort_layout : undefined;
          const missing = typeof setRuntimeOverride !== "function" ? "set_step_runtime_override"
            : runtime.enabled && typeof initializeWait !== "function" ? "init_wasm_wait"
            : runtime.enabled && typeof abortLayout !== "function" ? "wasm_wait_abort_layout" : undefined;
          if (missing !== undefined) throw Object.assign(new Error(`facade glue 缺少 runtime initializer: ${missing}`), {
            code: "WASM_RUNTIME_NOT_READY", where: "facade-engine-initialize.runtime", context: { operation: missing },
          });
          if (runtime.enabled) {
            const wait = runtime.wait;
            if (wait === undefined || typeof initializeWait !== "function" || typeof setRuntimeOverride !== "function") {
              throw new Error("已校验的 runtime initializer 状态丢失");
            }
            setRuntimeOverride.call(loaded.glue, true);
            initializeWait.call(loaded.glue, wait.spinUs, wait.quietUs, wait.fineUs, wait.coarseUs);
            if (typeof abortLayout !== "function") throw new Error("已校验的 abort layout getter 状态丢失");
            waitAbort = parseWasmWaitAbortLayout(abortLayout.call(loaded.glue), memory);
          } else {
            if (typeof setRuntimeOverride !== "function") throw new Error("已校验的 runtime setter 状态丢失");
            setRuntimeOverride.call(loaded.glue, false);
          }
        }
        if (initialArchive !== null) {
          const initializeRestore = loaded.glue.facade_slot_init_for_restore_json;
          if (typeof initializeRestore !== "function") throw Object.assign(new Error("facade glue 缺少 String restore topology initializer"), {
            code: "WASM_RESTORE_TOPOLOGY_NOT_READY", where: "facade-engine-initialize.initial-archive", context: { operation: "restore" },
          });
          const clear = loaded.glue.clear_prepared_restore;
          if (typeof clear !== "function") throw new Error("facade glue 缺少初始档释放接口");
          clearPrepared = clear;
          initializeRestore(JSON.stringify(initialArchive));
        } else loaded.glue.facade_slot_init_for_setup(setup);
        const layout = loaded.glue.facade_slot_layout();
        options.environment.installFacadeSlotLayout(layout); options.environment.getArenaView();
        const candidate = Object.freeze({ module: loaded.module, memory, layout, diagnostics: loaded.diagnostics });
        await handshake.configure(candidate, waitAbort);
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
