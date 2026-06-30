/**
 * 默认 SessionSetup。
 *
 * 与 engine 的 GameConfig.proposed_defaults 对齐的取值，加上 5 只预设股票、NPC 配额、
 * 策略参数与 v 模型参数。金额一律为「分」。
 */
import type { SessionSetup, StockSpec } from "../types/engine";

export interface StockMeta {
  code: string;
  name: string;
}

/** 代码 → 中文名 映射，仅用于 UI 显示（engine 不感知名字）。 */
export const STOCK_NAMES: Record<string, string> = {
  "600101": "稳健实业",
  "002156": "芯片科技",
  "300260": "短线题材",
  "600610": "人气妖股",
  "000812": "ST低价股",
};

/** UI 用的代码顺序与元数据。 */
export const STOCK_LIST: StockMeta[] = [
  { code: "600101", name: "稳健实业" },
  { code: "002156", name: "芯片科技" },
  { code: "300260", name: "短线题材" },
  { code: "600610", name: "人气妖股" },
  { code: "000812", name: "ST低价股" },
];

const STOCK_SPECS: StockSpec[] = [
  // 稳健实业 11.20 元 / 涨跌停 10%
  mkSpec("600101", 1120, 0.10),
  // 芯片科技 27.35 元 / 涨跌停 10%
  mkSpec("002156", 2735, 0.10),
  // 短线题材 36.80 元 / 涨跌停 10%
  mkSpec("300260", 3680, 0.10),
  // 人气妖股 7.55 元 / 涨跌停 10%
  mkSpec("600610", 755, 0.10),
  // ST低价股 2.85 元 / 涨跌停 5%
  mkSpec("000812", 285, 0.05),
];

/** 构造单只股票的 StockSpec。v_initial 与 initial_price 相同，tick 取最小价位 1 分。 */
function mkSpec(code: string, initialPrice: number, limitPct: number): StockSpec {
  return {
    code,
    initial_price: initialPrice,
    limit_pct: limitPct,
    v_initial: initialPrice,
    tick: 1,
    float_shares: 1_000_000,
  };
}

export const DEFAULT_SETUP: SessionSetup = {
  stocks: STOCK_SPECS,
  npcs: {
    retail_count: 3,
    inst_count: 2,
    hot_count: 1,
    // 每个_npc 初始资金 1 千万元（分）
    cash_per_npc: 1_000_000_00,
  },
  config: {
    commission_rate: 0.00025,
    commission_min: 500,
    stamp_tax_rate: 0.0005,
    default_limit: 0.10,
    st_limit: 0.05,
    lot_size: 100,
    starting_cash: 10_000_000,
  },
  v_params: {
    // long_run_mean 用作均值回归基准；这里逐股由 engine 读取，但 setup 字段是全局的，
    // 取一个代表性初值（取自首只股票），engine 内部会按各股 v_initial 覆盖。
    long_run_mean: 1120,
    mean_reversion: 0.5,
    volatility: 0.02,
  },
  strategy_params: {
    retail: { arrival_rate: 0.3, order_size_mean: 2, chase_prob: 0.4, tick_cents: 1 },
    inst: { margin: 0.02, order_size: 20 },
    hot: { lookback: 20, trend_threshold: 0.03, order_size: 10 },
  },
  player_cash: 10_000_000_00,
  ticks_per_day: 240, // A股交易时段 240 分钟（9:30-11:30 + 13:00-15:00），1 step = 1 交易分钟
  history_len: 20,
  t1_enabled: false,
  float_allocation: "Random",
};

/** 会话随机种子。 */
export const DEFAULT_SEED = 42n;
