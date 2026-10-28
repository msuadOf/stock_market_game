declare module "*wasm-pkg/web_wasm.js" {
  import type {
    Intent,
    PublicReportPage,
    PublicReportQuery,
    PublicReportSummary,
    HistoricalStockData,
    SaveSlot,
    SessionSetup,
    Snapshot,
  } from "./types/engine";
  import type { EngineUpdate } from "./types/generated/EngineUpdate";

  export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbindgen_thread_destroy: (tls: number, stack: number, stackSize: number) => void;
  }
  export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;
  const init: (input?: InitInput | Promise<InitInput> | { module_or_path: InitInput | Promise<InitInput>; memory?: WebAssembly.Memory; thread_stack_size?: number }, memory?: WebAssembly.Memory) => Promise<InitOutput>;
  export default init;
  export function initSync(options: { module: BufferSource | WebAssembly.Module; memory?: WebAssembly.Memory; thread_stack_size?: number } | BufferSource | WebAssembly.Module, memory?: WebAssembly.Memory): InitOutput;
  export function facade_slot_init(capacity: number): void;
  export function facade_slot_init_for_setup(setup: SessionSetup): void;
  export function facade_slot_layout(): import("./host/facade-thread-binding.ts").FacadeSlotLayout;
  export function facade_child_trampoline(slot: number, gen: number, workerId: number): number;
  export function ingress_token(handle: number): number;
  export function ingress_enqueue(token: number, intent: unknown): void;
  export function create_session(setup: SessionSetup, seed: bigint): number;
  export function step(handle: number): EngineUpdate;
  export function snapshot(handle: number): Snapshot;
  export function stock_catalog_view(handle: number): import("./types/generated/StockIdentityEntry").StockIdentityEntry[];
  export function company_catalog_view(handle: number): import("./types/generated/CompanyCatalogEntry").CompanyCatalogEntry[];
  export function runtime_snapshot(handle: number): Snapshot;
  export function tick(handle: number): bigint;
  export function day(handle: number): number;
  export function civil_date(handle: number): string;
  export function end_civil_day(handle: number): EngineUpdate;
  export function public_report_page(handle: number, query: PublicReportQuery): PublicReportPage;
  export function public_report_by_id(handle: number, id: string): PublicReportSummary;
  export function query_stock_history(handle: number, code: import("./types/generated/StockCode").StockCode): HistoricalStockData;
  export function enqueue(handle: number, intent: Intent): void;
  export function save(handle: number): SaveSlot;
  export function restore(slot: SaveSlot): number;
  export function restore_json(saveJson: string): number;
  export function drop_session(handle: number): void;
}
