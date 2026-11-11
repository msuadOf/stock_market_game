import type { EngineHost } from "../host/engine-host.ts"
import type { HostFailure } from "../host/host-update.ts"
import type { WasmMemoryAdmissionTicket } from "../host/wasm-document-memory-owner.ts"
import type { SessionSetup } from "../types/engine.ts"
import type { ValidatedArchiveLease } from "../save/validated-archive-lease.ts"
import { createChunkedHandoverSources } from "../save/chunked-handover-source.ts"
import { getBrowserChunkedHandoverSources } from "../save/browser-chunked-handover.ts"

/** 可选的真实恢复能力；未注入时仍使用 String 路径，不能把 lease 当 archive 传递。 */
export interface ChunkedArchiveRestore {
  handoverSources?: ReturnType<typeof createChunkedHandoverSources>
  prepareCapability(lease: ValidatedArchiveLease): Promise<void>
  /** 分配前登记 ticket release；resolve 后移除启动取消监听，健康终结继续由 host.dispose 管理。 */
  createHost(input: Readonly<{
    setup: SessionSetup
    seed: bigint
    lease: ValidatedArchiveLease
    ticket: WasmMemoryAdmissionTicket
    /** 仅表示本次构造尝试的取消，不替代就绪宿主的正常关闭协议。 */
    signal: AbortSignal
    onSpawnFailure(failure: HostFailure): void
    onInitializationProgress(message: string): void
  }>): Promise<EngineHost>
}

/** 将已验证的存档租约交给同一 document 的 WASM 宿主。 */
export function createBrowserChunkedArchiveRestore(options: {
  documentKey: object
  runtimeOptions(): import("../host/wasm-runtime-options.ts").WasmRuntimeOptions
  prepare: typeof import("../host/wasm-restore-capability-source.ts").prepareDocumentWasmRestoreCapability
  create: typeof import("../host/worker-host.ts").createWorkerHost
  e2e: boolean
}): ChunkedArchiveRestore {
  const handoverSources = getBrowserChunkedHandoverSources(options.documentKey)
  return {
    handoverSources,
    async prepareCapability() {
      const runtime = options.runtimeOptions()
      await options.prepare(options.documentKey, { runtimeOverride: runtime.runtimeOverride, restoreMode: "chunked" })
    },
    async createHost(input) {
      let runtime: import("../host/wasm-runtime-options.ts").WasmRuntimeOptions
      try { runtime = options.runtimeOptions() }
      catch (error) {
        try { input.ticket.registerRelease(Promise.resolve({ kind: "not-allocated" })) }
        catch (releaseError) { throw new AggregateError([error, releaseError], "WASM runtime 配置和零分配回执均失败") }
        throw error
      }
      return options.create(input.setup, input.seed, {
        ...runtime, enableE2EStepping: options.e2e,
        onSpawnFailure: input.onSpawnFailure, onInitializationProgress: input.onInitializationProgress,
        memoryAdmissionTicket: input.ticket, initialChunkedLease: input.lease, initialChunkedSignal: input.signal,
      })
    },
  }
}
