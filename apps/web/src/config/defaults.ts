/**
 * 默认 SessionSetup。
 *
 * 与 engine 的 GameConfig.proposed_defaults 对齐的取值，加上 5 只预设股票、NPC 配额、
 * 策略参数与 v 模型参数。金额一律为「分」。
 */
import type { SessionSetup, StockExchange, StockSpec } from "../types/engine";

export interface StockMeta {
  code: string;
  name: string;
}

/** 游戏世界时间：一个 tick 等于一秒；分时图按 60 tick（一分钟）聚合。 */
export const TICKS_PER_TRADING_MINUTE = 60;
export const TRADING_MINUTES_PER_DAY = 240;
export const CALL_AUCTION_ENTRY_MINUTES = 10;
export const PREOPEN_MINUTES = 5;
export const OPENING_WINDOW_MINUTES = CALL_AUCTION_ENTRY_MINUTES + PREOPEN_MINUTES;
export const AUCTION_VOLUME_LINES_PER_MINUTE = 10;
/** 既有引擎契约名；覆盖竞价申报期与 PreOpen 完整开盘窗口。 */
export const CALL_AUCTION_TICKS = OPENING_WINDOW_MINUTES * TICKS_PER_TRADING_MINUTE;
export const TOTAL_TICKS_PER_DAY = CALL_AUCTION_TICKS + TRADING_MINUTES_PER_DAY * TICKS_PER_TRADING_MINUTE;

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
  mkSpec("600101", "Shanghai", 1120, "MainBoard", "8928571429", 3_571_428_571),
  // 芯片科技 27.35 元 / 涨跌停 10%
  mkSpec("002156", "Shenzhen", 2735, "MainBoard", "2925045704", 2_047_531_993),
  // 创业板股票 36.80 元 / 涨跌停 20%
  mkSpec("300260", "Shenzhen", 3680, "ChiNext", "815217391", 611_413_043),
  // 人气妖股 7.55 元 / 涨跌停 10%
  mkSpec("600610", "Shanghai", 755, "MainBoard", "1059602649", 847_682_119),
  // ST低价股 2.85 元 / 2026-07-06 起主板风险警示股票涨跌停 10%
  mkSpec("000812", "Shenzhen", 285, "StMainBoard", "1052631579", 842_105_263),
];

/** 构造单只股票的 StockSpec。v_initial 与 initial_price 相同，tick 取最小价位 1 分。 */
function mkSpec(
  code: string,
  exchange: StockExchange,
  initialPrice: number,
  category: StockSpec["category"],
  totalShares: string,
  floatShares: number,
): StockSpec {
  const limitPct = category === "ChiNext" ? 0.20 : 0.10;
  return {
    code,
    exchange,
    initial_price: initialPrice,
    category,
    limit_pct: limitPct,
    v_initial: initialPrice,
    tick: 1,
    total_shares: totalShares,
    float_shares: floatShares,
  };
}

export const DEFAULT_SETUP: SessionSetup = {
  stocks: STOCK_SPECS,
  npcs: {
    retail_count: 20_000,
    inst_count: 5,
    hot_count: 2,
    // 当前字段作为散户现金基准：20 万元（分）；分类型财富分布在后续模型中显式化。
    retail_cash_median: 20_000_000,
  },
  config: {
    commission_rate: 0.00025,
    commission_min: 500,
    stamp_tax_rate: 0.0005,
    default_limit: 0.10,
    st_limit: 0.10,
    lot_size: 100,
    // 玩家初始资金 1 千万元；初始资金只有这一处真源。
    starting_cash: 1_000_000_000,
  },
  v_params: {
    // VParams 的完整配置字段；逐股长期均值由 fundamental_value_means 提供。
    long_run_mean: 1120,
    mean_reversion: 0.5,
    volatility: 0.02,
  },
  fundamental_value_means: Object.fromEntries(
    STOCK_SPECS.map((stock) => [stock.code, stock.v_initial]),
  ),
  strategy_params: {
    // 三个数量字段是群体中心；基准不少于一手时，每个 NPC 在 60%–140% 内采样一次整手规模。
    // 散户取 300 股，使严格比例区间内存在 200/300/400 三个合法整手档位。
    retail: { arrival_rate: 0.3, order_size_mean: 300, chase_prob: 0.4, tick_cents: 1 },
    // 大账户每次仍只提交合法的子单；库存和现金属于各自账户，不是聚合成交量。
    inst: { margin: 0.02, order_size: 200_000 },
    hot: { lookback: 20, trend_threshold: 0.03, order_size: 100_000 },
  },
  // 09:15–09:25 集合竞价申报、09:25–09:30 盘前静默窗口 + 240 分钟连续竞价；一 tick 为一秒。
  ticks_per_day: TOTAL_TICKS_PER_DAY,
  auction_ticks: CALL_AUCTION_TICKS,
  history_len: 20,
  t1_enabled: true,
  float_allocation: { ByKind: { retail: 0.45, inst: 0.53, hot: 0.02 } },
};

/** 会话随机种子。 */
export const DEFAULT_SEED = 42n;
