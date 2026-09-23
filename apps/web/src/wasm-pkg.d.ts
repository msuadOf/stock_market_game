declare module "*wasm-pkg/web_wasm.js" {
  import type {
    Intent,
    PublicReportPage,
    PublicReportQuery,
    PublicReportSummary,
    SaveSlot,
    SessionSetup,
    Snapshot,
  } from "./types/engine";
  import type { EngineUpdate } from "./types/generated/EngineUpdate";
  import type { NpcDecisionTraceRecord } from "./host/npc-decision-trace";

  const init: (input: BufferSource) => Promise<void>;
  export default init;
  export function create_session(setup: SessionSetup, seed: bigint): number;
  export function step(handle: number): EngineUpdate;
  export function snapshot(handle: number): Snapshot;
  export function runtime_snapshot(handle: number): Snapshot;
  export function tick(handle: number): bigint;
  export function day(handle: number): number;
  export function civil_date(handle: number): string;
  export function end_civil_day(handle: number): EngineUpdate;
  export function public_report_page(handle: number, query: PublicReportQuery): PublicReportPage;
  export function public_report_by_id(handle: number, id: string): PublicReportSummary;
  export function npc_decision_trace(handle: number, account: bigint): NpcDecisionTraceRecord[];
  export function enqueue(handle: number, intent: Intent): void;
  export function save(handle: number): SaveSlot;
  export function restore(slot: SaveSlot): number;
  export function restore_json(saveJson: string): number;
  export function drop_session(handle: number): void;
  export function initThreadPool(threads: number): Promise<void>;
}
