import { validateDayEndArchive } from "../save/day-end-candidate.ts";
import { parseSaveSlot } from "../save/save-schema.ts";
import type { StrictSaveEnvelope } from "../save/schema/root.ts";
import type { FacadeSlotLayout, FacadeThreadConfiguration } from "./facade-thread-binding.ts";
import type { FacadeArenaView } from "./facade-supervisor.ts";
import type { FacadeSubstrate } from "./engine-supervisor.ts";
import { createFacadeEngineHandshake } from "./facade-engine-handshake.ts";
import { createFacadeSharedMemory, instantiateFacadeModule, type FacadeSyncInitializer } from "./facade-wasm-module.ts";
import { receiveVerifiedRestore } from "./wasm-restore-verified-receiver.ts";
import { RestoreConsumerCleanupTimeoutError } from "./restore-chunk-port.ts";
import type { Sha256ChainResult } from "../save/sha256-chain.ts";
import type { RestoreChunkGlue } from "./wasm-restore-chunk-sender.ts";

export interface FacadeEngineGlue extends FacadeSyncInitializer {
  readonly facade_slot_init_for_setup: (setup: unknown) => void;
  readonly facade_slot_init_for_restore_json?: (saveJson: string) => void;
  readonly clear_prepared_restore?: () => void;
  readonly facade_restore_chunks_begin?: (total: number) => number;
  readonly facade_restore_chunks_push?: (token: number, bytes: Uint8Array) => void;
  readonly facade_restore_chunks_finish?: (token: number) => void;
  readonly facade_restore_chunks_clear?: (token: number) => boolean;
  readonly restore_prepared_chunks?: (token: number) => number;
  readonly facade_slot_layout: () => FacadeSlotLayout;
  readonly set_step_runtime_override?: (enabled: boolean) => void;
  readonly libos_host_fault_layout?: () => unknown;
  readonly wasm_tombstone_init_primary: () => unknown;
}
function parseRuntimeOptions(source: Record<string, unknown>): { readonly requested: boolean; readonly enabled?: boolean } {
  const hasOverride = Object.hasOwn(source, "runtimeOverride");
  if (!hasOverride) return { requested: false };
  const runtimeOverride = source.runtimeOverride;
  if (typeof runtimeOverride !== "boolean") throw new Error("engine init runtimeOverride 必须为 boolean");
  return { requested: true, enabled: runtimeOverride };
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
  let chunkToken: number | null = null;
  let receiverAbort: AbortController | undefined;
  let receiverTask: Promise<number> | undefined;
  let chunkPort: MessagePort | undefined;
  let current: FacadeSubstrate | undefined;
  let ownedPort: MessagePort | undefined;
  let clearPrepared: (() => void) | undefined;
  let handshake: ReturnType<typeof createFacadeEngineHandshake> | undefined;
  function dispose(): void {
    if (disposed) return;
    disposed = true; current = undefined; setup = undefined; initialArchive = null; chunkToken = null;
    const failures: unknown[] = [];
    try { receiverAbort?.abort(new Error("分块初始化已取消")); } catch (error) { failures.push(error); }
    if (receiverTask === undefined) { try { chunkPort?.close(); } catch (error) { failures.push(error); } }
    chunkPort = undefined;
    try { clearPrepared?.(); } catch (error) { failures.push(error); } finally { clearPrepared = undefined; }
    try { handshake?.dispose(); } catch (error) { failures.push(error); } finally { handshake = undefined; }
    if (failures.length === 1) throw failures[0];
    if (failures.length > 1) throw new AggregateError(failures, "engine 初始化资源清理失败");
  }
  async function disposeAsync(): Promise<void> {
    let disposalError: unknown; let disposalFailed = false;
    try { dispose(); } catch (error) { disposalError = error; disposalFailed = true; }
    let receiverError: unknown; let receiverFailed = false;
    if (receiverTask !== undefined) {
      try { await receiverTask; }
      catch (error) { if (error instanceof RestoreConsumerCleanupTimeoutError || error instanceof AggregateError) { receiverError = error; receiverFailed = true; } }
    }
    if (disposalFailed && receiverFailed) throw new AggregateError([disposalError, receiverError], "engine 初始化资源及分块接收清理失败");
    if (disposalFailed) throw disposalError;
    if (receiverFailed) throw receiverError;
  }
  return {
    async initialize(value: unknown): Promise<FacadeSubstrate> {
      let receivedPort: MessagePort | undefined;
      let receivedChunkPort: MessagePort | undefined;
      try {
        if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Error("engine init 必须为对象");
        const source = value as Record<string, unknown>;
        if (source.controlPort instanceof MessagePort) receivedPort = source.controlPort;
        if (source.initialChunked !== null && typeof source.initialChunked === "object" && (source.initialChunked as { port?: unknown }).port instanceof MessagePort) receivedChunkPort = (source.initialChunked as { port: MessagePort }).port;
        if (started || disposed) throw new Error("engine init 重复或已 dispose；每个 Worker 只允许一次");
        started = true;
        const hasSetup = Object.hasOwn(source, "setup"), hasArchive = Object.hasOwn(source, "initialArchive"), hasChunked = Object.hasOwn(source, "initialChunked");
        if (source.type !== "init" || receivedPort === undefined || Number(hasSetup) + Number(hasArchive) + Number(hasChunked) !== 1
          || Object.keys(source).some((key) => !["type", "setup", "initialArchive", "initialChunked", "controlPort", "runtimeOverride"].includes(key))) {
          throw new Error("engine init 需要合法 controlPort 与互斥的唯一 setup 或 initialArchive，禁止未知字段");
        }
        let chunkInput: { port: MessagePort; generation: number; contentDigest: Sha256ChainResult; availableWasmBytes: number } | undefined;
        if (hasChunked) {
          const raw = source.initialChunked;
          if (raw === null || typeof raw !== "object" || Array.isArray(raw)) throw new Error("initialChunked 必须为对象");
          const input = raw as Record<string, unknown>;
          if (input.port instanceof MessagePort) chunkPort = input.port;
          if (Object.keys(input).sort().join(",") !== "availableWasmBytes,contentDigest,generation,port" || !(input.port instanceof MessagePort) || !Number.isSafeInteger(input.generation) || Number(input.generation) < 1 || !Number.isSafeInteger(input.availableWasmBytes) || Number(input.availableWasmBytes) < 0) throw new Error("initialChunked 端口、generation 或预算无效");
          chunkInput = input as unknown as typeof chunkInput;
        }
        const runtime = parseRuntimeOptions(source);
        handshake = createFacadeEngineHandshake({ port: receivedPort, onFailure: (error) => {
          try { dispose(); }
          catch (cleanupError) { options.onFailure(new AggregateError([error, cleanupError], "engine 控制故障且清理失败")); return; }
          options.onFailure(error);
        } });
        ownedPort = receivedPort;
        // Worker 的 MessagePort 已创建独占副本；直接调用方仍须克隆以隔离嵌套引用。
        if (hasArchive) {
          initialArchive = validateDayEndArchive(parseSaveSlot(options.ownsIncomingArchive === true ? source.initialArchive : structuredClone(source.initialArchive)));
          setup = initialArchive.setup;
        } else if (hasSetup) setup = structuredClone(source.setup);
        const isolated = options.isolated();
        if (isolated !== true) throw new Error("engine 需要 crossOriginIsolated 隔离环境");
        const memory = options.createMemory === undefined ? createFacadeSharedMemory({ isolated }) : options.createMemory();
        await handshake.registerMemory(memory);
        if (disposed) throw new Error("Memory 登记后实例已 dispose");
        options.environment.bindFacadeThreadEnvironment({ memory, role: "engine", postControl: handshake.postControl });
        const loaded = await options.load();
        if (disposed) throw new Error("engine 加载完成时实例已 dispose");
        instantiateFacadeModule(loaded.glue, loaded.module, memory);
        if (typeof loaded.glue.wasm_tombstone_init_primary !== "function") throw new Error("facade glue 缺少 wasm_tombstone_init_primary");
        const primary = loaded.glue.wasm_tombstone_init_primary();
        await handshake.registerDeath(primary);
        if (disposed) throw new Error("DEAD 登记后实例已 dispose");
        if (runtime.requested) {
          const setRuntimeOverride = loaded.glue.set_step_runtime_override;
          const missing = typeof setRuntimeOverride !== "function" ? "set_step_runtime_override" : undefined;
          if (missing !== undefined) throw Object.assign(new Error(`facade glue 缺少 runtime initializer: ${missing}`), {
            code: "WASM_RUNTIME_NOT_READY", where: "facade-engine-initialize.runtime", context: { operation: missing },
          });
          if (typeof setRuntimeOverride !== "function") throw new Error("已校验的 runtime setter 状态丢失");
          setRuntimeOverride.call(loaded.glue, runtime.enabled === true);
        }
        if (chunkInput !== undefined) {
          const names = ["facade_restore_chunks_begin", "facade_restore_chunks_push", "facade_restore_chunks_finish", "facade_restore_chunks_clear", "restore_prepared_chunks"] as const;
          const missing = names.find((name) => typeof loaded.glue[name] !== "function");
          if (missing !== undefined) throw Object.assign(new Error(`facade glue 缺少 chunked 恢复接口 ${missing}`), { code: "WASM_RESTORE_TOPOLOGY_NOT_READY", where: "facade-engine-initialize.initial-chunked", context: { operation: "restore", missing } });
          receiverAbort = new AbortController();
          receiverTask = receiveVerifiedRestore(chunkInput.port, chunkInput.generation, chunkInput.contentDigest, chunkInput.availableWasmBytes, loaded.glue as RestoreChunkGlue, receiverAbort.signal);
          const receivedToken = await receiverTask;
          if (disposed) {
            if (!loaded.glue.facade_restore_chunks_clear!(receivedToken)) throw new Error("实例已 dispose，但分块 token 清理未确认");
            throw new Error("分块接收完成时实例已 dispose");
          }
          chunkToken = receivedToken;
          const ownedToken = chunkToken;
          clearPrepared = () => { loaded.glue.facade_restore_chunks_clear!(ownedToken); };
        } else if (initialArchive !== null) {
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
        let libosFault: unknown;
        if (runtime.enabled === true) {
          const faultExport = loaded.glue.libos_host_fault_layout;
          if (!Object.hasOwn(loaded.glue, "libos_host_fault_layout") || typeof faultExport !== "function") throw new Error("LibOS 故障域正式布局导出缺失或非法");
          libosFault = faultExport.call(loaded.glue);
          if (libosFault === undefined) throw new Error("LibOS 故障域正式布局导出缺少布局");
        }
        const candidate = Object.freeze({ module: loaded.module, memory, layout, diagnostics: loaded.diagnostics, ...(libosFault === undefined ? {} : { libosFault }) });
        await handshake.configure(candidate);
        if (disposed) throw new Error("engine configured 到达时实例已 dispose");
        current = candidate;
        return candidate;
      } catch (error) {
        if (receivedPort !== undefined && receivedPort !== ownedPort) receivedPort.close();
        if (receivedChunkPort !== undefined && receivedChunkPort !== chunkPort) receivedChunkPort.close();
        try { await disposeAsync(); }
        catch (cleanupError) { if (cleanupError !== error) throw new AggregateError([error, cleanupError], "engine 初始化失败且清理未完成"); }
        throw error;
      }
    },
    requireConfigured(): { readonly substrate: FacadeSubstrate; readonly setup: unknown; readonly initialArchive: StrictSaveEnvelope | null; readonly chunkToken: number | null } {
      if (disposed || current === undefined || handshake === undefined) throw new Error("engine 尚未 configured 或已 dispose");
      handshake.assertConfigured(); return { substrate: current, setup, initialArchive, chunkToken };
    },
    dispose,
    disposeAsync,
  };
}
