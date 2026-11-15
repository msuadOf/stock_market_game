import type { WasmLifecycleObservation as LibosLifecycleObservation } from "../../../../packages/libos/browser/wasm-lifecycle-observation.ts";
import type { WasmSessionShutdown } from "./wasm-shutdown-report.ts";

export type WasmLifecycleObservation = LibosLifecycleObservation
  | { readonly type: "rustShutdown"; readonly report: WasmSessionShutdown };
