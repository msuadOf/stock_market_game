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
  import type { AccountDividendTaxStatusView } from "./types/generated/AccountDividendTaxStatusView";
  import type { DividendTaxOutstandingView } from "./types/generated/DividendTaxOutstandingView";
  import type { DividendTaxProfile } from "./save/schema/corporate-actions";

  const init: (input: BufferSource | WebAssembly.Module) => Promise<{ memory: WebAssembly.Memory }>;
  export default init;
  export function initSync(options: { module: WebAssembly.Module; memory: WebAssembly.Memory }): unknown;
  export function ingress_token(handle: number): number;
  export function ingress_enqueue(token: number, intent: unknown): void;
  export function create_session(setup: SessionSetup, seed: bigint): number;
  export function step(handle: number): EngineUpdate;
  export function snapshot(handle: number): Snapshot;
  export function runtime_snapshot(handle: number): Snapshot;
  export function tick(handle: number): bigint;
  export function day(handle: number): number;
  export function civil_date(handle: number): string;
  export function end_civil_day(handle: number): EngineUpdate;
  export function owner_dividend_tax_status(handle: number): AccountDividendTaxStatusView;
  export function owner_dividend_tax_outstanding_views(handle: number): DividendTaxOutstandingView[];
  export function configure_dividend_tax_book(handle: number, account: string, stock: string, profile: DividendTaxProfile): void;
  export function owner_rejected_rights_subscriptions(handle: number): unknown[];
  export function subscribe_rights_offering(handle: number, eventId: string, shares: string): unknown;
  export function company_preference_rejections(handle: number, company: string): unknown[];
  export function company_capabilities(handle: number, company: string): unknown;
  export function company_period_explanation(handle: number, company: string, periodEnd: string): unknown;
  export function owner_rights_offerings(handle: number): unknown[];
  export function owner_flat_withholding_receipts(handle: number): unknown[];
  export function propose_company_action(handle: number, proposal: unknown): unknown;
  export function set_simple_preferences(handle: number, company: string, preferences: unknown): void;
  export function company_simple_preferences(handle: number, company: string): unknown;
  export function public_report_page(handle: number, query: PublicReportQuery): PublicReportPage;
  export function public_report_by_id(handle: number, id: string): PublicReportSummary;
  export function query_stock_history(handle: number, code: import("./types/generated/StockCode").StockCode): HistoricalStockData;
  export function enqueue(handle: number, intent: Intent): void;
  export function save(handle: number): SaveSlot;
  export function restore(slot: SaveSlot): number;
  export function restore_json(saveJson: string): number;
  export function drop_session(handle: number): void;
  export function initThreadPool(threads: number): Promise<void>;
}
