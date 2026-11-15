/** 只含数值、字符串和纯数据快照；禁止把 Memory、SAB、view 或 Worker 交给观测者。 */
export type WasmLifecycleObservation =
  | { readonly type: "memoryRegistered"; readonly allocationId: 1; readonly role: "engine" }
  | { readonly type: "allocationConfigured"; readonly allocationId: 1; readonly role: "engine" | "ingress" | "publisher" | "child"; readonly workerId: number | null; readonly slot: number | null; readonly gen: number | null }
  | { readonly type: "childSpawn"; readonly workerId: number; readonly slot: number; readonly gen: number; readonly name: string; readonly runtimeName: string }
  | { readonly type: "childExit"; readonly workerId: number; readonly slot: number; readonly gen: number; readonly activeChildren: number; readonly manifestEntries: number; readonly pendingRecovery: number }
  | { readonly type: "quiescent"; readonly role: "engine" | "ingress" | "publisher"; readonly activeChildren: number; readonly manifestEntries: number; readonly pendingRecovery: number }
  | { readonly type: "childrenDrained"; readonly activeChildren: number; readonly manifestEntries: number; readonly pendingRecovery: number }
  | { readonly type: "finalRelease"; readonly kind: "released"; readonly finalBytes: number }
  | { readonly type: "finalRelease"; readonly kind: "not-allocated" }
  | { readonly type: "releaseUnknown"; readonly message: string };
