/**
 * engine 类型（TS 镜像 Rust serde 结构）。
 * WS-2: 手写（typeshare 待后续接入），精确匹配 engine 的 serde JSON 输出。
 *
 * 关键 serde 规则：
 * - Money(i64) → newtype 透明 → 裸 number（分/cents）
 * - StockCode(String) → 裸 string
 * - AccountId(u64) → 裸 number（但在 Map 中作 key 时为 string）
 * - enum → serde 外部标签：{ "Variant": { ...fields } } 或 "UnitVariant"
 * - BTreeMap → serde-wasm-bindgen 默认为 JS Map（host 适配器规整为 Object）
 */

// ── 基础 ──

/** 金额（分）。显示时 / 100 → 元，toFixed(2)。 */
export type Cents = number;

/** 股票代码。 */
export type StockCode = string;

/** 账户 ID（u64，在 JS 里是 number；Map key 时为 string）。 */
export type AccountId = number;

// ── 枚举 ──

export type Side = "Buy" | "Sell";

export type FloatAllocation = "Random" | { ByKind: { retail: number; inst: number; hot: number } };

export type RejectionReason = "InsufficientCash" | "InsufficientShares" | "LimitExceeded" | "UnknownStock";

// ── Intent（玩家/NPC 意图）──

export type Intent =
  | { PlaceLimit: { code: StockCode; side: Side; price: Cents; qty: number } }
  | { PlaceMarket: { code: StockCode; side: Side; qty: number } }
  | { Cancel: { code: StockCode; id: number } };

// ── Event（增量事件，带 seq）──

export interface TradeEvent {
  seq: number;
  code: StockCode;
  price: Cents;
  qty: number;
  maker: AccountId;
  taker: AccountId;
}

export interface PriceTickEvent {
  seq: number;
  code: StockCode;
  last_price: Cents;
}

export interface DayBoundaryEvent {
  seq: number;
  day: number;
}

export interface IntentRejectedEvent {
  seq: number;
  account: AccountId;
  code: StockCode;
  reason: RejectionReason;
}

export interface SettlementErrorEvent {
  seq: number;
  account: AccountId;
  code: StockCode;
  reason: string;
}

export interface VErrorEvent {
  seq: number;
  code: StockCode;
  reason: string;
}

/** 事件联合。serde 外部标签格式。 */
export type EngineEvent =
  | { Trade: TradeEvent }
  | { PriceTick: PriceTickEvent }
  | { DayBoundary: DayBoundaryEvent }
  | { IntentRejected: IntentRejectedEvent }
  | { SettlementError: SettlementErrorEvent }
  | { VError: VErrorEvent };

// ── Snapshot（完整快照）──

/** 价格+量 pair（盘口深度）。 */
export type PriceLevel = [Cents, number];

export interface MarketSnap {
  last_price: Cents;
  last_close: Cents;
  best_bid: Cents | null;
  best_ask: Cents | null;
  fundamental_value: Cents;
  bids: PriceLevel[];
  asks: PriceLevel[];
}

export interface PositionSnap {
  qty: number;
  t1_locked: number;
  invested_cents: number;
  recovered_cents: number;
}

export interface AccountSnap {
  cash: Cents;
  positions: Record<StockCode, PositionSnap>;
}

/** Snapshot（markets/accounts 在 host 适配器中从 Map 规整为 Object）。 */
export interface Snapshot {
  seq: number;
  tick: number;
  day: number;
  markets: Record<StockCode, MarketSnap>;
  accounts: Record<string, AccountSnap>;
}

// ── SessionSetup（构造参数）──

export interface StockSpec {
  code: StockCode;
  initial_price: Cents;
  limit_pct: number;
  v_initial: Cents;
  tick: Cents;
  float_shares: number;
}

export interface NpcSetup {
  retail_count: number;
  inst_count: number;
  hot_count: number;
  cash_per_npc: Cents;
}

export interface GameConfig {
  commission_rate: number;
  commission_min: Cents;
  stamp_tax_rate: number;
  default_limit: number;
  st_limit: number;
  lot_size: number;
  starting_cash: Cents;
}

export interface VParams {
  long_run_mean: Cents;
  mean_reversion: number;
  volatility: number;
}

export interface StrategyParams {
  retail: { arrival_rate: number; order_size_mean: number; chase_prob: number; tick_cents: number };
  inst: { margin: number; order_size: number };
  hot: { lookback: number; trend_threshold: number; order_size: number };
}

export interface SessionSetup {
  stocks: StockSpec[];
  npcs: NpcSetup;
  config: GameConfig;
  v_params: VParams;
  strategy_params: StrategyParams;
  player_cash: Cents;
  ticks_per_day: number;
  history_len: number;
  t1_enabled: boolean;
  float_allocation: FloatAllocation;
}

// ── WASM 句柄 API（apps/web-wasm/pkg/web_wasm.d.ts 镜像）──

export interface WasmApi {
  create_session(setup: SessionSetup, seed: bigint): number;
  step(handle: number): EngineEvent[];
  snapshot(handle: number): Snapshot;
  tick(handle: number): bigint;
  day(handle: number): number;
  enqueue(handle: number, intent: Intent): void;
  drop_session(handle: number): void;
}
