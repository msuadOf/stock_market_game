import type { WasmSessionShutdown } from "./wasm-shutdown-report.ts";

/** 只含数值、字符串和纯数据快照；禁止把 Memory、SAB、view 或 Worker 交给观测者。 */
export type WasmLifecycleObservation =
  | { readonly type: "rustShutdown"; readonly report: WasmSessionShutdown }
  | { readonly type: "childExit"; readonly workerId: number; readonly slot: number; readonly gen: number; readonly activeChildren: number; readonly manifestEntries: number; readonly pendingRecovery: number }
  | { readonly type: "quiescent"; readonly role: "engine" | "ingress"; readonly activeChildren: number; readonly manifestEntries: number; readonly pendingRecovery: number }
  | { readonly type: "childrenDrained"; readonly activeChildren: number; readonly manifestEntries: number; readonly pendingRecovery: number }
  | { readonly type: "finalRelease"; readonly kind: "released"; readonly finalBytes: number }
  | { readonly type: "finalRelease"; readonly kind: "not-allocated" }
  | { readonly type: "releaseUnknown"; readonly message: string };
