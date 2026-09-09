//! NPC 策略抽象与实现（ADR-0006）：统一数据模型、兼容 trait 与 ZI/价值/动量策略。
//!
//! 设计：策略是纯函数式决策——看多股市场快照 + 自己的快照 + 注入的 RNG，返回 0..N 个「意图」(Intent)。
//! 策略不直接碰 orderbook，只产 Intent，由 account/market 层执行 → 可单测/可插拔/可并行。

use crate::account::{AccountKind, StockCode};
use crate::behavior::{
    decide_retail_position, decide_retail_position_with_experience, BehaviorMarketObservation,
    PositionDecision,
};
use crate::experience::RetailExperienceState;
use crate::money::Money;
use crate::observation::AccountRiskObservation;
use crate::orderbook::{OrderId, Side};
use std::collections::BTreeMap;
use std::collections::BTreeSet;

/// 独立自然人散户的长期行为风格。风格只决定参数分布，不共享账户、库存或 RNG。
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    ts_rs::TS,
)]
pub enum RetailStyle {
    Dormant,
    LongTerm,
    Noise,
    DipBuyer,
    Momentum,
    Panic,
}

/// 独立机构账户的投资风格。五个默认机构按账户序号轮换，账户与库存不共享。
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    ts_rs::TS,
)]
pub enum InstitutionStyle {
    DeepValue,
    Growth,
    Balanced,
    Defensive,
    ActiveTrader,
}

/// 独立游资账户的短线风格。
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    ts_rs::TS,
)]
pub enum HotStyle {
    Momentum,
    Reversal,
}

/// 账户身份之外的决策策略族。
///
/// 身份仍决定账户规模和会话编排；策略能力单独决定可见数据，策略族说明如何从其允许的观测生成委托。
/// 二者分开后，一个积极交易型机构可以使用动量，而不会被重解释成游资账户。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum StrategyFamily {
    RetailBehavior,
    FundamentalValue,
    Momentum,
}

/// 账户策略的可恢复身份档案；运行期 trait 对象不得作为唯一的存档依据。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub enum StrategyProfile {
    Retail(RetailStyle),
    Institution(InstitutionStyle),
    Hot(HotStyle),
}

// ─── 数据驱动策略（ADR-0006 数据化改造，为 GPU 化铺路）──────────────────────────
//
// 设计目标：把「每类一个 struct + impl Strategy」收敛为「统一参数表 StrategyData +
// 统一 decide 内核」。StrategyData 是一个扁平可序列化 struct——未来可直接映射到 GPU
// StorageBuffer（每个 NPC 一份参数）。CPU trait/struct 路径继续作为权威实现，
// 内部全部委托给这里的纯函数实现，保证「同种子同输出」不漂移。

/// 统一策略参数（数据驱动，可 serde → 未来塞进 GPU buffer）。
///
/// `kind` 决定走哪个 `decide` 分支；其余字段是三类 NPC 参数的并集（无关字段对该 kind 无效）。
/// 运行进度不存入策略数据；DriftUp 只读取 [`MarketView::market_minute`]，避免形成无法随存档恢复的
/// 第二套时钟。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct StrategyData {
    /// 账户种类：决定走哪个 decide 分支。
    pub kind: AccountKind,
    // ── 散户（Retail / ZiNoise）参数 ──
    /// 每次观察时的下单到达概率，∈[0,1]。
    pub arrival_rate: f64,
    /// 个体每单股数；由群体基准采样后写入。
    pub order_size_mean: u32,
    /// 追势概率，∈[0,1]。
    pub chase_prob: f64,
    /// 价格跨 tick 的「分」数（>0）。
    pub tick_cents: i64,
    /// 近期跌幅达到该阈值后开始尝试抄底。
    pub dip_threshold: f64,
    /// 个人持仓亏损达到该阈值后触发止损。
    pub stop_loss_threshold: f64,
    /// 个人持仓盈利达到该阈值后倾向获利了结。
    pub take_profit_threshold: f64,
    /// 追涨所需的最低相对成交量。
    pub volume_confirmation: f64,
    /// 单只股票市值占总资产的上限。
    pub max_stock_fraction: f64,
    /// 个体在平静市场下每 tick 至少观察一次的基础概率，∈(0,1]。
    pub base_observation_probability: f64,
    // ── 机构（Inst / Value）参数 ──
    /// 容忍带宽度，∈[0,1)。
    pub margin: f64,
    /// 每单股数，>0（机构/游资共用字段名 order_size）。
    pub order_size: u32,
    /// 机构目标价策略。
    pub target_policy: TargetPolicy,
    // ── 游资（Hot / Momentum）参数 ──
    /// 回看完整交易分钟数，≥2。
    pub lookback: usize,
    /// 触发动作的相对变化阈值（绝对值），≥0。
    pub trend_threshold: f64,
}

impl StrategyData {
    /// 构造散户参数集（inst/hot 字段填 0 占位，对该 kind 无效）。
    pub fn retail(
        arrival_rate: f64,
        order_size_mean: u32,
        chase_prob: f64,
        tick_cents: i64,
    ) -> Self {
        StrategyData {
            kind: AccountKind::Retail,
            arrival_rate,
            order_size_mean,
            chase_prob,
            tick_cents,
            dip_threshold: 0.02,
            stop_loss_threshold: 0.05,
            take_profit_threshold: 0.08,
            volume_confirmation: 0.60,
            max_stock_fraction: 0.35,
            base_observation_probability: 1.0,
            margin: 0.0,
            order_size: 0,
            target_policy: TargetPolicy::Fixed(Money::ZERO),
            lookback: 0,
            trend_threshold: 0.0,
        }
    }

    /// 构造机构参数集（retail/hot 字段填占位）。
    pub fn inst(target_policy: TargetPolicy, margin: f64, order_size: u32) -> Self {
        StrategyData {
            kind: AccountKind::Inst,
            arrival_rate: 0.0,
            order_size_mean: 0,
            chase_prob: 0.0,
            tick_cents: 0,
            dip_threshold: 0.0,
            stop_loss_threshold: 0.0,
            take_profit_threshold: 0.0,
            volume_confirmation: 0.0,
            max_stock_fraction: 0.60,
            base_observation_probability: 1.0,
            margin,
            order_size,
            target_policy,
            lookback: 0,
            trend_threshold: 0.0,
        }
    }

    /// 构造游资参数集（retail/inst 字段填占位）。
    pub fn hot(lookback: usize, trend_threshold: f64, order_size: u32) -> Self {
        StrategyData {
            kind: AccountKind::Hot,
            arrival_rate: 0.0,
            order_size_mean: 0,
            chase_prob: 0.0,
            tick_cents: 0,
            dip_threshold: 0.0,
            stop_loss_threshold: 0.0,
            take_profit_threshold: 0.0,
            volume_confirmation: 0.60,
            max_stock_fraction: 0.25,
            base_observation_probability: 1.0,
            margin: 0.0,
            order_size,
            target_policy: TargetPolicy::Fixed(Money::ZERO),
            lookback,
            trend_threshold,
        }
    }
}

/// 统一 decide 内核（数据驱动入口）。按 `strategy.kind` 分派到三类纯函数实现。
///
/// Player → 恒空 Vec（玩家不持算法策略）。
/// 数据路径与 trait 路径共享同一决策内核，确保同种子同输出（铁律三）。
pub fn decide_data(
    strategy: &StrategyData,
    market: &MarketView,
    own: &SelfView,
    rng: &mut dyn Rng,
) -> Vec<Intent> {
    match strategy.kind {
        AccountKind::Retail => decide_retail(strategy, market, own, rng),
        AccountKind::Inst => decide_inst(strategy, market, own),
        AccountKind::Hot => decide_hot(strategy, market, own),
        AccountKind::Player => Vec::new(),
    }
}

/// 散户 decide 内核（噪音到达 + 追涨、下跌抄底与亏损止损）。
///
/// 关键修正（修复「只有一只股票有成交」）：散户**从全部股票中均匀随机选一只**下单，
/// 而非恒取 `first_key_value()`（旧实现会让全部散户 NPC 的订单集中在字典序最小的那只股票——
/// BTreeMap<StockCode> 按 string 排序，首键固定——其余股票毫无散户流动性、无人撮合、价格不动）。
/// 随机选股经注入 RNG，保持确定性（同种子同输出，铁律三）。
fn decide_retail(
    strategy: &StrategyData,
    market: &MarketView,
    own: &SelfView,
    rng: &mut dyn Rng,
) -> Vec<Intent> {
    if market.stocks.is_empty() {
        return Vec::new();
    }
    // 从全部股票中均匀随机选一只（注入 RNG，确定性）。BTreeMap 无随机访问 → 先按 key 取索引。
    let n = market.stocks.len();
    let idx = rng.next_range_u32(0, n as u32) as usize;
    let (code, sv) = match market.stocks.keys().nth(idx) {
        Some(c) => {
            let v = market
                .stocks
                .get(c)
                .expect("key 刚从同一 BTreeMap 取出，必存在（防御式：不可达则显式 panic）");
            (c.clone(), v)
        }
        None => return Vec::new(),
    };
    let change = market_minute_price_change(sv).unwrap_or(0.0);
    let volume_activity = 0.5
        + 0.5 * sv.relative_volume.clamp(0.0, 1.0)
        + 0.25 * sv.order_book_imbalance.abs().clamp(0.0, 1.0);
    let price_activity = if strategy.dip_threshold > 0.0 {
        (change.abs() / strategy.dip_threshold).min(1.0)
    } else {
        0.0
    };
    let effective_arrival = (strategy.arrival_rate * (volume_activity + price_activity)).min(1.0);
    if rng.next_f64() >= effective_arrival {
        return Vec::new();
    }
    if rng.next_f64() < strategy.chase_prob {
        let position = own.positions.get(&code);
        let pnl = position
            .and_then(|p| p.cost_price)
            .filter(|cost| cost.cents() > 0)
            .map(|cost| (sv.last_price.cents() - cost.cents()) as f64 / cost.cents() as f64);
        if change > 0.0 {
            if pnl.is_some_and(|value| value >= strategy.take_profit_threshold) {
                let sellable = position.map_or(0, |p| p.sellable_qty);
                if let Some(qty) = a_share_sell_qty(strategy.order_size_mean, sellable) {
                    return vec![Intent::PlaceLimit {
                        code,
                        side: Side::Sell,
                        price: sv.best_bid.unwrap_or(sv.last_price),
                        qty,
                    }];
                }
                return Vec::new();
            }
            if sv.relative_volume < strategy.volume_confirmation {
                return Vec::new();
            }
            let price = sv.best_ask.unwrap_or(sv.last_price);
            let Some(qty) = risk_capped_buy_qty(
                strategy.order_size_mean,
                &code,
                price,
                market,
                own,
                strategy.max_stock_fraction,
            ) else {
                return Vec::new();
            };
            return vec![Intent::PlaceLimit {
                code,
                side: Side::Buy,
                price,
                qty,
            }];
        } else if change < 0.0 {
            let loss = pnl.map_or(0.0, |value| (-value).max(0.0));
            let effective_stop_threshold =
                strategy.stop_loss_threshold * (1.0 + 0.35 * sv.order_book_imbalance);
            if loss >= effective_stop_threshold {
                let sellable = position.map(|p| p.sellable_qty).unwrap_or(0);
                if let Some(qty) = a_share_sell_qty(strategy.order_size_mean, sellable) {
                    return vec![Intent::PlaceLimit {
                        code,
                        side: Side::Sell,
                        price: sv.best_bid.unwrap_or(sv.last_price),
                        qty,
                    }];
                }
                // 当日抄底后继续下跌：受 A 股 T+1 约束，只能等待下一交易日再止损。
                return Vec::new();
            }
            let effective_dip_threshold = strategy.dip_threshold
                * (1.0 - 0.25 * sv.order_book_imbalance)
                * (1.0 + 0.50 * (loss / strategy.stop_loss_threshold.max(f64::EPSILON)).min(1.0));
            if -change >= effective_dip_threshold {
                // 不知道内在价值的散户把足够大的跌幅当作“变便宜”，以小单主动吃卖一试探。
                let price = sv.best_ask.unwrap_or(sv.last_price);
                let Some(qty) = risk_capped_buy_qty(
                    strategy.order_size_mean,
                    &code,
                    price,
                    market,
                    own,
                    strategy.max_stock_fraction,
                ) else {
                    return Vec::new();
                };
                return vec![Intent::PlaceLimit {
                    code,
                    side: Side::Buy,
                    price,
                    qty,
                }];
            }
            return Vec::new();
        }
    }
    let side = if rng.next_f64() < 0.5 {
        Side::Buy
    } else {
        Side::Sell
    };
    let (code, sv) = if side == Side::Sell {
        let sellable_codes: Vec<&StockCode> = market
            .stocks
            .keys()
            .filter(|candidate| {
                own.positions
                    .get(*candidate)
                    .is_some_and(|position| position.sellable_qty > 0)
            })
            .collect();
        if sellable_codes.is_empty() {
            return Vec::new();
        }
        let selected =
            sellable_codes[rng.next_range_u32(0, sellable_codes.len() as u32) as usize].clone();
        let selected_view = market
            .stocks
            .get(&selected)
            .expect("sellable stock must exist in the same market view");
        (selected, selected_view)
    } else {
        (code, sv)
    };
    let price = match side {
        Side::Buy => {
            Money::from_cents(sv.best_bid.unwrap_or(sv.last_price).cents() + strategy.tick_cents)
        }
        Side::Sell => Money::from_cents(
            (sv.best_ask.unwrap_or(sv.last_price).cents() - strategy.tick_cents).max(0),
        ),
    };
    let qty = match side {
        Side::Buy => risk_capped_buy_qty(
            strategy.order_size_mean,
            &code,
            price,
            market,
            own,
            strategy.max_stock_fraction,
        ),
        Side::Sell => own
            .positions
            .get(&code)
            .and_then(|position| a_share_sell_qty(strategy.order_size_mean, position.sellable_qty)),
    };
    qty.map_or_else(Vec::new, |qty| {
        vec![Intent::PlaceLimit {
            code,
            side,
            price,
            qty,
        }]
    })
}

/// 机构 decide 内核（基本面价值策略）：按可成交报价试探并随折价分档加仓。
/// DriftUp 使用权威标准交易分钟，不维护策略私有时钟。
fn decide_inst(strategy: &StrategyData, market: &MarketView, own: &SelfView) -> Vec<Intent> {
    let mut out = Vec::new();
    for (code, sv) in &market.stocks {
        let target = match target_cents(
            &strategy.target_policy,
            sv.fundamental_value,
            market.market_minute.saturating_add(1),
        ) {
            Some(t) => t,
            None => continue,
        };
        let buy_price = sv.best_ask.unwrap_or(sv.last_price);
        let buy_quote = buy_price.cents() as f64;
        let sell_price = sv.best_bid.unwrap_or(sv.last_price);
        let sell_quote = sell_price.cents() as f64;
        let low = target * (1.0 - strategy.margin);
        let high = target * (1.0 + strategy.margin);
        if buy_quote < low {
            let discount = (target - buy_quote) / target;
            let divisor = if discount < strategy.margin * 2.0 {
                4
            } else if discount < strategy.margin * 4.0 {
                2
            } else {
                1
            };
            if let Some(qty) = a_share_tranche(strategy.order_size, divisor).and_then(|qty| {
                risk_capped_buy_qty(
                    qty,
                    code,
                    buy_price,
                    market,
                    own,
                    strategy.max_stock_fraction,
                )
            }) {
                out.push(Intent::PlaceLimit {
                    code: code.clone(),
                    side: Side::Buy,
                    price: buy_price,
                    qty,
                });
            }
        } else if sell_quote > high {
            let sellable = own.positions.get(code).map(|p| p.sellable_qty).unwrap_or(0);
            if let Some(qty) = a_share_sell_qty(strategy.order_size, sellable) {
                out.push(Intent::PlaceLimit {
                    code: code.clone(),
                    side: Side::Sell,
                    price: sell_price,
                    qty,
                });
            }
        }
    }
    out
}

/// 按 A 股 100 股申报单位把机构计划单拆成试探/加仓档位。
fn a_share_tranche(order_size: u32, divisor: u32) -> Option<u32> {
    const LOT: u32 = 100;
    if order_size < LOT {
        return None;
    }
    let raw = (order_size / divisor).max(LOT);
    Some(raw - raw % LOT)
}

/// 卖出量与会话边界使用同一规则：整手之外，可一次性带走全部可卖零股余量。
pub(crate) fn a_share_sell_qty(order_size: u32, sellable: u32) -> Option<u32> {
    const LOT: u32 = 100;
    let requested = order_size.min(sellable);
    let odd_lot = sellable % LOT;
    let board_lot_candidate = requested - requested % LOT;
    let odd_lot_candidate = if requested >= odd_lot && odd_lot > 0 {
        odd_lot + ((requested - odd_lot) / LOT) * LOT
    } else {
        0
    };
    let qty = board_lot_candidate.max(odd_lot_candidate);
    (qty > 0).then_some(qty)
}

/// 游资 decide 内核（动量策略）。趋势窗口以完整交易分钟计，不受宿主 tick 密度影响。
fn decide_hot(strategy: &StrategyData, market: &MarketView, own: &SelfView) -> Vec<Intent> {
    let mut out = Vec::new();
    for (code, sv) in &market.stocks {
        let p = &sv.recent_market_minute_prices;
        if p.len() < 2 {
            continue;
        }
        let start = p.len().saturating_sub(strategy.lookback);
        let first = p[start].cents() as f64;
        let last = p.last().unwrap().cents() as f64;
        if first <= 0.0 {
            continue;
        }
        let change = (last - first) / first;
        if change > strategy.trend_threshold
            && sv.relative_volume >= strategy.volume_confirmation
            && sv.order_book_imbalance > -0.50
        {
            let price = sv.best_ask.unwrap_or(sv.last_price);
            if let Some(qty) = risk_capped_buy_qty(
                strategy.order_size,
                code,
                price,
                market,
                own,
                strategy.max_stock_fraction,
            ) {
                out.push(Intent::PlaceLimit {
                    code: code.clone(),
                    side: Side::Buy,
                    price,
                    qty,
                });
            }
        } else if change < -strategy.trend_threshold {
            let sellable = own
                .positions
                .get(code)
                .map(|pp| pp.sellable_qty)
                .unwrap_or(0);
            if let Some(qty) = a_share_sell_qty(strategy.order_size, sellable) {
                out.push(Intent::PlaceLimit {
                    code: code.clone(),
                    side: Side::Sell,
                    price: sv.best_bid.unwrap_or(sv.last_price),
                    qty,
                });
            }
        }
    }
    out
}

/// 反转型游资：放量急跌时承接，放量急涨且已有可卖库存时兑现。
fn decide_hot_reversal(
    strategy: &StrategyData,
    market: &MarketView,
    own: &SelfView,
) -> Vec<Intent> {
    let mut out = Vec::new();
    for (code, sv) in &market.stocks {
        let prices = &sv.recent_market_minute_prices;
        if prices.len() < 2 || sv.relative_volume < strategy.volume_confirmation {
            continue;
        }
        let start = prices.len().saturating_sub(strategy.lookback);
        let first = prices[start].cents() as f64;
        let last = prices.last().expect("non-empty price window").cents() as f64;
        if first <= 0.0 {
            continue;
        }
        let change = (last - first) / first;
        if change < -strategy.trend_threshold && sv.order_book_imbalance < 0.50 {
            let price = sv.best_bid.unwrap_or(sv.last_price);
            if let Some(qty) = risk_capped_buy_qty(
                strategy.order_size,
                code,
                price,
                market,
                own,
                strategy.max_stock_fraction,
            ) {
                out.push(Intent::PlaceLimit {
                    code: code.clone(),
                    side: Side::Buy,
                    price,
                    qty,
                });
            }
        } else if change > strategy.trend_threshold {
            let sellable = own
                .positions
                .get(code)
                .map(|position| position.sellable_qty)
                .unwrap_or(0);
            if let Some(qty) = a_share_sell_qty(strategy.order_size, sellable) {
                out.push(Intent::PlaceLimit {
                    code: code.clone(),
                    side: Side::Sell,
                    price: sv.best_ask.unwrap_or(sv.last_price),
                    qty,
                });
            }
        }
    }
    out
}

/// 依目标价策略算目标价（返回 cents 的 f64）。V 不可见且 TrackV → None。
/// 抽出为自由函数，供数据驱动内核与旧 ValueStrategy 共用（保证两者永不漂移）。
fn target_cents(
    policy: &TargetPolicy,
    v: Option<Money>,
    elapsed_market_minutes: u64,
) -> Option<f64> {
    match policy {
        TargetPolicy::Fixed(m) => Some(m.cents() as f64),
        TargetPolicy::TrackV { bias } => v.map(|vv| vv.cents() as f64 * (1.0 + bias)),
        TargetPolicy::DriftUp { rate, base } => {
            Some(base.cents() as f64 * (1.0 + rate * elapsed_market_minutes as f64))
        }
    }
}

/// 在策略层先约束现金与单股暴露，账户层仍保留最终费用及冻结校验。
pub(crate) fn risk_capped_buy_qty(
    desired_qty: u32,
    code: &StockCode,
    price: Money,
    market: &MarketView,
    own: &SelfView,
    max_stock_fraction: f64,
) -> Option<u32> {
    const LOT: u32 = 100;
    const CASH_RESERVE_CENTS: i64 = 1_000;
    if price.cents() <= 0
        || !max_stock_fraction.is_finite()
        || !(0.0..=1.0).contains(&max_stock_fraction)
    {
        return None;
    }
    let cash = i128::from(own.cash.cents().max(0));
    let positions_value = own
        .positions
        .iter()
        .try_fold(0_i128, |total, (stock, position)| {
            let last = market.stocks.get(stock)?.last_price.cents();
            (last > 0).then(|| total + i128::from(last) * i128::from(position.qty))
        })?;
    let equity = cash + positions_value;
    let current_value = own.positions.get(code).map_or(0_i128, |position| {
        i128::from(price.cents()) * i128::from(position.qty)
    });
    // 单股测试局不能因为“分散化”而失去全部买方；股票越少，最低允许权重越高。
    let universe_floor = 1.0 / market.stocks.len() as f64;
    let effective_fraction = max_stock_fraction.max(universe_floor).min(1.0);
    let exposure_limit = (equity as f64 * effective_fraction).floor() as i128;
    let exposure_room = exposure_limit.saturating_sub(current_value);
    let spendable_cash = cash
        .saturating_sub(i128::from(CASH_RESERVE_CENTS))
        .min(cash * 99 / 100);
    let price_cents = i128::from(price.cents());
    let affordable = spendable_cash.max(0) / price_cents;
    let exposure_capacity = exposure_room.max(0) / price_cents;
    let capacity = affordable.min(exposure_capacity).min(i128::from(u32::MAX)) as u32;
    let qty = desired_qty.min(capacity);
    let board_lot_qty = qty - qty % LOT;
    (board_lot_qty >= LOT).then_some(board_lot_qty)
}

/// 单只股票的市场视图（多股 MarketView 的元素）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct StockView {
    pub best_bid: Option<Money>,
    pub best_ask: Option<Money>,
    pub last_price: Money,
    /// 隐藏公允价 V；Some 仅对该策略可见（编排层决定：机构 Some、散户/游资/玩家 None）。
    pub fundamental_value: Option<Money>,
    /// 最近 N 个 last_price（滚动窗口，供 tick 级观察使用）。
    ///
    /// 仅保留给短期盘口、注意力等 tick 级观测；游资趋势不得读取它。
    pub recent_prices: Vec<Money>,
    /// 当前交易日已经完成的标准交易分钟收盘价；游资趋势窗口使用该序列。
    /// 不足两个完整分钟时趋势显式不可用，不能以 tick 点数补齐时间跨度。
    pub recent_market_minute_prices: Vec<Money>,
    /// 当前交易日截至本 tick 的成交量，相对于历史同期预期量的倍率。
    pub relative_volume: f64,
    /// 前五档买卖盘数量失衡，范围 [-1,1]；正值表示买盘更厚，负值表示卖盘更厚。
    pub order_book_imbalance: f64,
}

/// 整个市场快照（多股）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MarketView {
    pub stocks: BTreeMap<StockCode, StockView>,
    /// 当前权威游戏 tick；用于逐 tick 的注意力等采样。
    pub tick: u64,
    /// 从开局累计的已完成标准交易分钟；用于依赖市场时间的策略（例如 DriftUp）。
    pub market_minute: u64,
}

/// 策略所属账户的自身快照（跨股）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SelfView {
    /// 当前决策可用现金：账户现金扣除不可撤换委托的冻结额；本观察点会原子撤换的
    /// NPC 工作单冻结额可继续用于同一轮目标报价。
    pub cash: Money,
    pub positions: BTreeMap<StockCode, PositionView>,
}

/// 单只股票的持仓视图。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PositionView {
    pub qty: u32,
    pub sellable_qty: u32,
    pub cost_price: Option<Money>,
}

/// 策略决策产物。account/market 层据此执行（下单/撤单）；返回空 Vec 表示本 tick 不动作。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub enum Intent {
    /// 限价单：在 price 挂 qty 股。
    PlaceLimit {
        code: StockCode,
        side: Side,
        price: Money,
        qty: u32,
    },
    /// 市价单（按对手最优即时成交）：挂 qty 股。（市价单延后支持，类型先就位）
    PlaceMarket {
        code: StockCode,
        side: Side,
        qty: u32,
    },
    /// 撤单。
    Cancel { code: StockCode, id: OrderId },
}

/// 一次策略评估的委托与工作单对齐范围。
///
/// `reviewed_stocks` 为空表示旧策略没有声明完整目标；非空时 Session 必须把这些股票的
/// 买卖两侧工作单都与最新目标对齐，即使本次判断是 Hold/Watch 且没有新委托。
#[derive(Clone, Debug, Default)]
pub struct StrategyDecision {
    pub intents: Vec<Intent>,
    pub reviewed_stocks: BTreeSet<StockCode>,
    /// 仅散户 B02/B03 判断路径提供的目标仓位解释。Session 将其作为非权威诊断样本读取；
    /// 它不参与存档、撮合或下一 tick 的决策。
    pub position_decision: Option<PositionDecision>,
}

/// 随机源抽象。生产用种子化 PRNG（ADR-0005），测试可注入固定实现。
/// 本 trait 让 Strategy 不绑定具体 RNG 实现（SplitMix64 等待 market 模块引入）。
pub trait Rng {
    /// 返回 [0, 1) 的 f64（用于泊松到达率、参数采样等）。NaN/Inf 不允许（实现方保证）。
    fn next_f64(&mut self) -> f64;
    /// 返回 [lo, hi) 的 u32。
    fn next_range_u32(&mut self, lo: u32, hi: u32) -> u32;
}

/// NPC 下单策略的统一抽象（ADR-0006）。看多股市场 + 自身快照 + 注入 RNG，返回 0..N 个 Intent。
/// 玩家账户不实现此 trait（strategy = None，UI 动作直接产 Intent）。
pub trait Strategy: Send + Sync {
    /// 可序列化的策略身份；会话把它写入权威存档以防恢复时静默换策略。
    fn profile(&self) -> StrategyProfile;
    /// 当前实例的决策策略族；用于审计身份与策略不再强制一一对应。
    fn strategy_family(&self) -> StrategyFamily;

    /// 是否需要读取隐藏公允价值 V。此能力由策略、而不是账户身份决定；会话层据此选择
    /// 含 V 或公共市场视图，防止非价值策略未来意外获得内部信息。
    fn needs_fundamental_value(&self) -> bool {
        self.strategy_family() == StrategyFamily::FundamentalValue
    }

    /// 是否把策略给出的限价目标交给会话层按母单执行。
    ///
    /// 默认的逐笔意图语义保持不变；只有主动选择该模式的策略才会由
    /// `GameSession` 将目标拆分为可恢复的子单，并以真实成交推进进度。
    fn uses_parent_order_execution(&self) -> bool {
        false
    }
    fn retail_style(&self) -> Option<RetailStyle> {
        None
    }

    fn institution_style(&self) -> Option<InstitutionStyle> {
        None
    }

    fn hot_style(&self) -> Option<HotStyle> {
        None
    }

    /// 平静市场下每 tick 至少观察一次的基础概率。Session 使用它初始化个体注意力调度；
    /// 行情刺激只会在此基础上提高概率。
    fn base_observation_probability(&self) -> f64 {
        1.0
    }

    /// 空决策是否代表“已重新评估且现有报价失效”。事件到达型策略的空结果包含
    /// 未发生随机到达、到达后无可执行意图，均不能据此伪造撤单；持续扫描型策略返回 true。
    fn updates_working_quotes_on_empty_decision(&self) -> bool {
        true
    }

    /// 带标准市场时间与账户风险观测的决策入口。未接入新闭环的策略沿用原决定；
    /// 会话对散户始终提供两类观测，不能只提供其中之一。
    fn decide_with_behavior(
        &mut self,
        market: &MarketView,
        own: &SelfView,
        behavior_market: Option<&BehaviorMarketObservation>,
        account_risk: Option<&AccountRiskObservation>,
        rng: &mut dyn Rng,
    ) -> StrategyDecision {
        assert_eq!(
            behavior_market.is_some(),
            account_risk.is_some(),
            "behavior market and account-risk observations must be supplied together"
        );
        StrategyDecision {
            intents: self.decide(market, own, rng),
            reviewed_stocks: BTreeSet::new(),
            position_decision: None,
        }
    }

    /// 带可恢复个体经历的决策入口；未接入经历的策略保持 B02 行为。
    #[allow(clippy::too_many_arguments)]
    fn decide_with_experience(
        &mut self,
        market: &MarketView,
        own: &SelfView,
        behavior_market: Option<&BehaviorMarketObservation>,
        account_risk: Option<&AccountRiskObservation>,
        _experience: Option<&RetailExperienceState>,
        _market_minute: u64,
        rng: &mut dyn Rng,
    ) -> StrategyDecision {
        self.decide_with_behavior(market, own, behavior_market, account_risk, rng)
    }

    fn decide(&mut self, market: &MarketView, own: &SelfView, rng: &mut dyn Rng) -> Vec<Intent>;
}

/// 策略构造/参数失败。绝不静默吞掉（铁律二）：非法参数一律 Err + 上报。
#[derive(Debug, thiserror::Error)]
pub enum StrategyError {
    /// 参数非法（如 arrival_rate∉[0,1]、order_size=0）。param 指明哪个参数、reason 说明为何非法。
    #[error("invalid param {param}: {reason}")]
    InvalidParam { param: &'static str, reason: String },
}

/// 散户：噪音到达 + 追涨、下跌抄底与亏损止损。
///
/// 在个体观察 tick 上以 arrival_rate 概率「到达」；若到达，按 chase_prob 概率
/// 顺近期趋势。下跌时，未触发止损者可尝试抄底，达到动态止损线且可卖者主动止损；
/// 当日买入仓位遵守 T+1。非趋势分支仍随机选择方向，但无资产支持时不产生无效意图。
/// 纯逻辑：所有随机经注入 `&mut dyn Rng`（可重放、可单测）；价格用 Money，禁止 f64 存储权威状态。
pub struct ZiNoiseStrategy {
    retail_style: RetailStyle,
    /// 每次观察时的到达概率，∈[0,1]。0 → 永不动作。
    arrival_rate: f64,
    /// 个体每单股数；工厂以配置值为群体中心采样。
    order_size_mean: u32,
    /// 追势概率，∈[0,1]。
    chase_prob: f64,
    /// 价格跨 tick 的「分」数（>0）。
    tick_cents: i64,
    dip_threshold: f64,
    stop_loss_threshold: f64,
    take_profit_threshold: f64,
    volume_confirmation: f64,
    max_stock_fraction: f64,
    base_observation_probability: f64,
}

impl ZiNoiseStrategy {
    /// 构造并校验参数。任一非法 → `StrategyError::InvalidParam`（防御式：不静默用默认值）。
    pub fn new(
        arrival_rate: f64,
        order_size_mean: u32,
        chase_prob: f64,
        tick_cents: i64,
    ) -> Result<Self, StrategyError> {
        if !(0.0..=1.0).contains(&arrival_rate) {
            return Err(StrategyError::InvalidParam {
                param: "arrival_rate",
                reason: format!("{arrival_rate} not in [0,1]"),
            });
        }
        if order_size_mean == 0 {
            return Err(StrategyError::InvalidParam {
                param: "order_size_mean",
                reason: "must be > 0".to_string(),
            });
        }
        if !(0.0..=1.0).contains(&chase_prob) {
            return Err(StrategyError::InvalidParam {
                param: "chase_prob",
                reason: format!("{chase_prob} not in [0,1]"),
            });
        }
        if tick_cents <= 0 {
            return Err(StrategyError::InvalidParam {
                param: "tick_cents",
                reason: "must be > 0".to_string(),
            });
        }
        Ok(ZiNoiseStrategy {
            retail_style: RetailStyle::Noise,
            arrival_rate,
            order_size_mean,
            chase_prob,
            tick_cents,
            dip_threshold: 0.02,
            stop_loss_threshold: 0.05,
            take_profit_threshold: 0.08,
            volume_confirmation: 0.60,
            max_stock_fraction: 0.35,
            base_observation_probability: 1.0,
        })
    }
}

impl Strategy for ZiNoiseStrategy {
    fn profile(&self) -> StrategyProfile {
        StrategyProfile::Retail(self.retail_style)
    }
    fn strategy_family(&self) -> StrategyFamily {
        StrategyFamily::RetailBehavior
    }

    fn retail_style(&self) -> Option<RetailStyle> {
        Some(self.retail_style)
    }

    fn base_observation_probability(&self) -> f64 {
        self.base_observation_probability
    }

    fn updates_working_quotes_on_empty_decision(&self) -> bool {
        false
    }

    fn decide_with_behavior(
        &mut self,
        market: &MarketView,
        own: &SelfView,
        behavior_market: Option<&BehaviorMarketObservation>,
        account_risk: Option<&AccountRiskObservation>,
        rng: &mut dyn Rng,
    ) -> StrategyDecision {
        match (behavior_market, account_risk) {
            (Some(behavior_market), Some(account_risk)) => {
                let mut data = StrategyData::retail(
                    self.arrival_rate,
                    self.order_size_mean,
                    self.chase_prob,
                    self.tick_cents,
                );
                data.dip_threshold = self.dip_threshold;
                data.stop_loss_threshold = self.stop_loss_threshold;
                data.take_profit_threshold = self.take_profit_threshold;
                data.volume_confirmation = self.volume_confirmation;
                data.max_stock_fraction = self.max_stock_fraction;
                data.base_observation_probability = self.base_observation_probability;
                let decision = decide_retail_position(
                    &data,
                    self.retail_style,
                    market,
                    own,
                    behavior_market,
                    account_risk,
                    rng,
                );
                StrategyDecision {
                    intents: retail_position_decision_to_intents(&data, &decision, market, own),
                    reviewed_stocks: decision.code.clone().into_iter().collect(),
                    position_decision: Some(decision),
                }
            }
            (None, None) => StrategyDecision {
                intents: self.decide(market, own, rng),
                reviewed_stocks: BTreeSet::new(),
                position_decision: None,
            },
            _ => panic!("behavior market and account-risk observations must be supplied together"),
        }
    }

    fn decide_with_experience(
        &mut self,
        market: &MarketView,
        own: &SelfView,
        behavior_market: Option<&BehaviorMarketObservation>,
        account_risk: Option<&AccountRiskObservation>,
        experience: Option<&RetailExperienceState>,
        market_minute: u64,
        rng: &mut dyn Rng,
    ) -> StrategyDecision {
        match (behavior_market, account_risk, experience) {
            (Some(behavior_market), Some(account_risk), Some(experience)) => {
                let mut data = StrategyData::retail(
                    self.arrival_rate,
                    self.order_size_mean,
                    self.chase_prob,
                    self.tick_cents,
                );
                data.dip_threshold = self.dip_threshold;
                data.stop_loss_threshold = self.stop_loss_threshold;
                data.take_profit_threshold = self.take_profit_threshold;
                data.volume_confirmation = self.volume_confirmation;
                data.max_stock_fraction = self.max_stock_fraction;
                data.base_observation_probability = self.base_observation_probability;
                let decision = decide_retail_position_with_experience(
                    &data,
                    self.retail_style,
                    market,
                    own,
                    behavior_market,
                    account_risk,
                    experience,
                    market_minute,
                    rng,
                );
                StrategyDecision {
                    intents: retail_position_decision_to_intents(&data, &decision, market, own),
                    reviewed_stocks: decision.code.clone().into_iter().collect(),
                    position_decision: Some(decision),
                }
            }
            (Some(_), Some(_), None) => {
                panic!("retail behavior observations require retail experience state")
            }
            (None, None, None) => StrategyDecision {
                intents: self.decide(market, own, rng),
                reviewed_stocks: BTreeSet::new(),
                position_decision: None,
            },
            _ => panic!(
                "behavior market, account-risk, and retail experience must be supplied together"
            ),
        }
    }

    fn decide(&mut self, market: &MarketView, own: &SelfView, rng: &mut dyn Rng) -> Vec<Intent> {
        // 委托给数据驱动内核（ADR-0006 数据化改造）：旧 struct 字段映射成 StrategyData，
        // 调统一纯函数 decide_retail，保证「同种子同输出」不漂移。
        let data = StrategyData::retail(
            self.arrival_rate,
            self.order_size_mean,
            self.chase_prob,
            self.tick_cents,
        );
        let mut data = data;
        data.dip_threshold = self.dip_threshold;
        data.stop_loss_threshold = self.stop_loss_threshold;
        data.take_profit_threshold = self.take_profit_threshold;
        data.volume_confirmation = self.volume_confirmation;
        data.max_stock_fraction = self.max_stock_fraction;
        data.base_observation_probability = self.base_observation_probability;
        decide_retail(&data, market, own, rng)
    }
}

fn retail_position_decision_to_intents(
    strategy: &StrategyData,
    decision: &PositionDecision,
    market: &MarketView,
    own: &SelfView,
) -> Vec<Intent> {
    let Some(code) = decision.code.as_ref() else {
        return Vec::new();
    };
    let Some(stock) = market.stocks.get(code) else {
        panic!(
            "position decision references unknown market stock {}",
            code.0
        );
    };
    if decision.executable_delta_shares > 0 {
        let desired = u32::try_from(decision.executable_delta_shares)
            .unwrap_or(u32::MAX)
            .min(strategy.order_size_mean);
        let price = stock.best_ask.unwrap_or(stock.last_price);
        return risk_capped_buy_qty(
            desired,
            code,
            price,
            market,
            own,
            strategy.max_stock_fraction,
        )
        .map_or_else(Vec::new, |qty| {
            vec![Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price,
                qty,
            }]
        });
    }
    if decision.executable_delta_shares < 0 {
        let desired = u32::try_from(-i128::from(decision.executable_delta_shares))
            .unwrap_or(u32::MAX)
            .min(strategy.order_size_mean);
        let sellable = own
            .positions
            .get(code)
            .map_or(0, |value| value.sellable_qty);
        return a_share_sell_qty(desired, sellable).map_or_else(Vec::new, |qty| {
            vec![Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Sell,
                price: stock.best_bid.unwrap_or(stock.last_price),
                qty,
            }]
        });
    }
    Vec::new()
}

/// 完整交易分钟收盘序列的相对变化（至少 2 个点，首价必须为正）。
///
/// 散户的追涨、抄底和止损不把宿主 tick 密度当作行情经历；tick 级 `recent_prices`
/// 仅可表达盘口/注意力等即时观测。
fn market_minute_price_change(sv: &StockView) -> Option<f64> {
    let p = &sv.recent_market_minute_prices;
    let first = p.first()?.cents();
    let last = p.last()?.cents();
    (p.len() >= 2 && first > 0).then(|| (last - first) as f64 / first as f64)
}

/// 机构目标价策略（每实例一种，机构看法各异）。
///
/// - `Fixed(m)`：固定目标价 m。
/// - `TrackV { bias }`：跟随隐藏公允价 V，target = V × (1 + bias)。
/// - `DriftUp { rate, base }`：认为大致上涨，target = base × (1 + rate × 已完成标准交易分钟)。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum TargetPolicy {
    /// 固定目标价。
    Fixed(Money),
    /// 跟随 V：target = V × (1 + bias)。
    TrackV { bias: f64 },
    /// 认为大致上涨：target = base × (1 + rate × 已完成标准交易分钟)。
    DriftUp { rate: f64, base: Money },
}

/// 机构：基本面价值策略。读隐藏 V + 目标价策略，低买高卖（带 margin 容忍带）。
///
/// 纯逻辑：价格/qty 用 Money/u32；f64 仅在 target/band 计算边界，立即落回 Intent。
/// 不直接碰 orderbook，只产 Intent。
pub struct ValueStrategy {
    style: InstitutionStyle,
    policy: TargetPolicy,
    /// 容忍带宽度，∈[0,1)。last 落在 [target×(1-margin), target×(1+margin)] 内不动作。
    margin: f64,
    /// 个体每单股数（>0）；工厂以配置值为群体中心采样。
    order_size: u32,
    max_stock_fraction: f64,
    base_observation_probability: f64,
}

impl ValueStrategy {
    /// 构造并校验参数。margin∉[0,1) 或 order_size=0 → `StrategyError::InvalidParam`（防御式：不静默用默认值）。
    pub fn new(policy: TargetPolicy, margin: f64, order_size: u32) -> Result<Self, StrategyError> {
        if !(0.0..1.0).contains(&margin) {
            return Err(StrategyError::InvalidParam {
                param: "margin",
                reason: format!("{margin} not in [0,1)"),
            });
        }
        if order_size == 0 {
            return Err(StrategyError::InvalidParam {
                param: "order_size",
                reason: "must be > 0".to_string(),
            });
        }
        Ok(ValueStrategy {
            style: InstitutionStyle::Balanced,
            policy,
            margin,
            order_size,
            max_stock_fraction: 0.60,
            base_observation_probability: 1.0,
        })
    }

    /// 为测试或显式配置保留机构身份；不会改变估值策略本身。
    pub fn with_institution_style(mut self, style: InstitutionStyle) -> Self {
        self.style = style;
        self
    }
}

impl Strategy for ValueStrategy {
    fn profile(&self) -> StrategyProfile {
        StrategyProfile::Institution(self.style)
    }
    fn strategy_family(&self) -> StrategyFamily {
        StrategyFamily::FundamentalValue
    }

    fn uses_parent_order_execution(&self) -> bool {
        true
    }

    fn institution_style(&self) -> Option<InstitutionStyle> {
        Some(self.style)
    }

    fn base_observation_probability(&self) -> f64 {
        self.base_observation_probability
    }

    fn decide(&mut self, market: &MarketView, own: &SelfView, _rng: &mut dyn Rng) -> Vec<Intent> {
        // 委托给数据驱动内核（ADR-0006 数据化改造）；DriftUp 从 MarketView 读取权威标准交易分钟。
        let mut data = StrategyData::inst(self.policy.clone(), self.margin, self.order_size);
        data.max_stock_fraction = self.max_stock_fraction;
        data.base_observation_probability = self.base_observation_probability;
        decide_inst(&data, market, own)
    }
}

/// 游资：短期趋势/动量策略，快进快出。
///
/// 看每只股票的完整交易分钟收盘窗口：取最近 `lookback` 个分钟点，算相对变化 change=(末-首)/首。
/// change > threshold、量能确认且盘口没有严重偏空 → 追涨买入；
/// change < -threshold 且有可卖持仓 → 杀跌卖出。
/// 持仓不足或趋势不明（|change| ≤ threshold、点数不足）→ 不动作。
///
/// 纯逻辑：随机经注入 `&mut dyn Rng`（本策略实际不消费 RNG，签名对齐 trait）；价格/qty 用 Money/u32；
/// f64 仅在 change 计算边界，立即落回 Intent。不直接碰 orderbook，只产 Intent。
pub struct MomentumStrategy {
    style: HotStyle,
    /// 回看完整交易分钟数，≥2。
    lookback: usize,
    /// 触发动作的相对变化阈值（绝对值），≥0。
    trend_threshold: f64,
    /// 个体每单股数，>0；工厂以配置值为群体中心采样。
    order_size: u32,
    /// 追涨所需的最低相对成交量。
    volume_confirmation: f64,
    max_stock_fraction: f64,
    base_observation_probability: f64,
}

impl MomentumStrategy {
    /// 构造并校验参数。lookback<2 / threshold<0 / order_size=0 → `StrategyError::InvalidParam`（防御式：不静默用默认值）。
    pub fn new(
        lookback: usize,
        trend_threshold: f64,
        order_size: u32,
    ) -> Result<Self, StrategyError> {
        if lookback < 2 {
            return Err(StrategyError::InvalidParam {
                param: "lookback",
                reason: "must be >= 2".to_string(),
            });
        }
        if trend_threshold < 0.0 {
            return Err(StrategyError::InvalidParam {
                param: "trend_threshold",
                reason: "must be >= 0".to_string(),
            });
        }
        if order_size == 0 {
            return Err(StrategyError::InvalidParam {
                param: "order_size",
                reason: "must be > 0".to_string(),
            });
        }
        Ok(MomentumStrategy {
            style: HotStyle::Momentum,
            lookback,
            trend_threshold,
            order_size,
            volume_confirmation: 0.60,
            max_stock_fraction: 0.25,
            base_observation_probability: 1.0,
        })
    }
}

impl Strategy for MomentumStrategy {
    fn profile(&self) -> StrategyProfile {
        StrategyProfile::Hot(self.style)
    }
    fn strategy_family(&self) -> StrategyFamily {
        StrategyFamily::Momentum
    }

    fn hot_style(&self) -> Option<HotStyle> {
        Some(self.style)
    }

    fn base_observation_probability(&self) -> f64 {
        self.base_observation_probability
    }

    fn decide(&mut self, market: &MarketView, own: &SelfView, _rng: &mut dyn Rng) -> Vec<Intent> {
        // 委托给数据驱动内核（ADR-0006 数据化改造）：字段映射成 StrategyData，
        // 调统一纯函数 decide_hot，保证「同种子同输出」不漂移。
        let data = StrategyData::hot(self.lookback, self.trend_threshold, self.order_size);
        let mut data = data;
        data.volume_confirmation = self.volume_confirmation;
        data.max_stock_fraction = self.max_stock_fraction;
        data.base_observation_probability = self.base_observation_probability;
        match self.style {
            HotStyle::Momentum => decide_hot(&data, market, own),
            HotStyle::Reversal => decide_hot_reversal(&data, market, own),
        }
    }
}

/// 身份为机构、但按动量执行的积极交易策略。
///
/// 它复用游资动量内核，却保留机构身份、资金规模、注意力敏感度与具名机构风格；因此既不
/// 把账户改成游资，也不读取隐藏公允价值 V。它不使用价值机构的母单执行，而是走普通工作报价。
struct InstitutionMomentumStrategy {
    style: InstitutionStyle,
    inner: MomentumStrategy,
}

impl Strategy for InstitutionMomentumStrategy {
    fn profile(&self) -> StrategyProfile {
        StrategyProfile::Institution(self.style)
    }
    fn strategy_family(&self) -> StrategyFamily {
        StrategyFamily::Momentum
    }

    fn institution_style(&self) -> Option<InstitutionStyle> {
        Some(self.style)
    }

    fn base_observation_probability(&self) -> f64 {
        self.inner.base_observation_probability()
    }

    fn decide(&mut self, market: &MarketView, own: &SelfView, rng: &mut dyn Rng) -> Vec<Intent> {
        self.inner.decide(market, own, rng)
    }
}

/// 散户策略分布参数（每实例从中采样/直接取）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub struct RetailParams {
    /// 每次观察时的到达概率，∈[0,1]。
    pub arrival_rate: f64,
    /// 群体订单规模基准，>0；基准不少于一手时，每个散户实例在其 60%–140%
    /// 范围内采样整手数量；不足一手时保持原值，不静默放大。
    pub order_size_mean: u32,
    /// 追势概率，∈[0,1]。
    pub chase_prob: f64,
    /// 价格跨 tick 的「分」数（>0）。
    pub tick_cents: i64,
}

/// 机构策略分布参数。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub struct InstParams {
    /// 容忍带宽度，∈[0,1)。
    pub margin: f64,
    /// 群体订单规模基准，>0；基准不少于一手时，每个机构实例在其 60%–140%
    /// 范围内采样整手数量；不足一手时保持原值，不静默放大。
    pub order_size: u32,
}

/// 游资策略分布参数。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub struct HotParams {
    /// 回看完整交易分钟数，≥2。
    pub lookback: usize,
    /// 触发动作的相对变化阈值（绝对值），≥0。
    pub trend_threshold: f64,
    /// 群体订单规模基准，>0；基准不少于一手时，每个游资实例在其 60%–140%
    /// 范围内采样整手数量；不足一手时保持原值，不静默放大。
    pub order_size: u32,
}

/// 每类 NPC 的群体基准参数。
///
/// `StrategyFactory` 在这些基准之上，经注入 RNG 为每个实例采样行为阈值；同一会话 seed
/// 可重建相同的个体性格，不同 NPC 则不会在同一个跌幅或量能点同步行动。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub struct StrategyParams {
    /// 散户参数。
    pub retail: RetailParams,
    /// 机构参数。
    pub inst: InstParams,
    /// 游资参数。
    pub hot: HotParams,
}

impl StrategyParams {
    /// 校验可能由 serde 直接构造的全部策略参数，不创建或静默禁用 NPC。
    pub fn validate(&self) -> Result<(), StrategyError> {
        ZiNoiseStrategy::new(
            self.retail.arrival_rate,
            self.retail.order_size_mean,
            self.retail.chase_prob,
            self.retail.tick_cents,
        )?;
        ValueStrategy::new(
            TargetPolicy::TrackV { bias: 0.0 },
            self.inst.margin,
            self.inst.order_size,
        )?;
        MomentumStrategy::new(
            self.hot.lookback,
            self.hot.trend_threshold,
            self.hot.order_size,
        )?;
        validate_order_size_sampling_range(self.retail.order_size_mean, "order_size_mean")?;
        validate_order_size_sampling_range(self.inst.order_size, "order_size")?;
        validate_order_size_sampling_range(self.hot.order_size, "order_size")?;
        Ok(())
    }
}

/// 策略工厂：按账户种类构造策略实例。
///
/// Player → `None`（玩家不持算法策略，UI 动作直接产 Intent）；
/// Retail/Inst/Hot → 按各自 `StrategyParams` 构造对应策略。
/// 构造失败（参数非法）→ 返回带上下文的 [`StrategyError`]；绝不静默禁用 NPC 或回退默认值。
pub struct StrategyFactory;

impl StrategyFactory {
    /// 按种类构造策略；个体阈值在创建时从会话 RNG 采样，同一 seed 可重建同一性格。
    pub fn build(
        kind: AccountKind,
        params: &StrategyParams,
        rng: &mut dyn Rng,
    ) -> Result<Option<Box<dyn Strategy + Send + Sync>>, StrategyError> {
        Self::build_for_market_day(kind, params, 15_300, rng)
    }

    /// 使用权威交易日 tick 数把“每天观察次数”换算成逐 tick 概率。
    pub fn build_for_market_day(
        kind: AccountKind,
        params: &StrategyParams,
        ticks_per_day: u64,
        rng: &mut dyn Rng,
    ) -> Result<Option<Box<dyn Strategy + Send + Sync>>, StrategyError> {
        Self::build_for_market_day_with_ordinal(kind, params, ticks_per_day, 0, rng)
    }

    /// 按账户在同类 NPC 中的序号分配稳定风格；序号只决定风格，不合并账户状态。
    pub fn build_for_market_day_with_ordinal(
        kind: AccountKind,
        params: &StrategyParams,
        ticks_per_day: u64,
        ordinal: u32,
        rng: &mut dyn Rng,
    ) -> Result<Option<Box<dyn Strategy + Send + Sync>>, StrategyError> {
        debug_assert!(ticks_per_day > 0);
        match kind {
            AccountKind::Retail => {
                let r = &params.retail;
                let mut strategy = ZiNoiseStrategy::new(
                    r.arrival_rate,
                    sample_individual_order_size(rng, r.order_size_mean, "order_size_mean")?,
                    r.chase_prob,
                    r.tick_cents,
                )?;
                let style_draw = rng.next_f64();
                strategy.retail_style = if style_draw < 0.35 {
                    RetailStyle::Dormant
                } else if style_draw < 0.55 {
                    RetailStyle::LongTerm
                } else if style_draw < 0.75 {
                    RetailStyle::Noise
                } else if style_draw < 0.85 {
                    RetailStyle::DipBuyer
                } else if style_draw < 0.95 {
                    RetailStyle::Momentum
                } else {
                    RetailStyle::Panic
                };
                strategy.dip_threshold = sample_between(rng, 0.015, 0.08);
                strategy.stop_loss_threshold = sample_between(rng, 0.03, 0.12);
                strategy.take_profit_threshold = sample_between(rng, 0.05, 0.18);
                strategy.volume_confirmation = sample_between(rng, 0.35, 1.25);
                strategy.max_stock_fraction = sample_between(rng, 0.15, 0.40);
                let observations_per_day = match strategy.retail_style {
                    RetailStyle::Dormant => sample_between(rng, 0.1, 0.5),
                    RetailStyle::LongTerm => sample_between(rng, 0.5, 2.0),
                    RetailStyle::Noise | RetailStyle::DipBuyer => sample_between(rng, 2.0, 8.0),
                    RetailStyle::Momentum => sample_between(rng, 10.0, 40.0),
                    RetailStyle::Panic => sample_between(rng, 5.0, 20.0),
                };
                match strategy.retail_style {
                    RetailStyle::Dormant => {
                        strategy.arrival_rate *= 0.20;
                        strategy.chase_prob *= 0.25;
                        strategy.max_stock_fraction = 0.80;
                    }
                    RetailStyle::LongTerm => {
                        strategy.arrival_rate *= 0.50;
                        strategy.chase_prob *= 0.40;
                        strategy.take_profit_threshold *= 1.50;
                        strategy.stop_loss_threshold *= 1.50;
                        strategy.max_stock_fraction = 0.65;
                    }
                    RetailStyle::Noise => {}
                    RetailStyle::DipBuyer => {
                        strategy.chase_prob = strategy.chase_prob.max(0.75);
                        strategy.dip_threshold *= 0.70;
                    }
                    RetailStyle::Momentum => {
                        strategy.chase_prob = strategy.chase_prob.max(0.85);
                        strategy.volume_confirmation *= 0.80;
                    }
                    RetailStyle::Panic => {
                        strategy.chase_prob = strategy.chase_prob.max(0.70);
                        strategy.stop_loss_threshold *= 0.55;
                    }
                }
                strategy.base_observation_probability =
                    daily_observations_to_tick_probability(observations_per_day, ticks_per_day);
                Ok(Some(Box::new(strategy)))
            }
            AccountKind::Inst => {
                let i = &params.inst;
                let style = match ordinal % 5 {
                    0 => InstitutionStyle::DeepValue,
                    1 => InstitutionStyle::Growth,
                    2 => InstitutionStyle::Balanced,
                    3 => InstitutionStyle::Defensive,
                    _ => InstitutionStyle::ActiveTrader,
                };
                let (bias_center, margin_factor, max_fraction, observations) = match style {
                    InstitutionStyle::DeepValue => (-0.025, 1.35, (0.45, 0.70), (15.0, 40.0)),
                    InstitutionStyle::Growth => (0.030, 1.00, (0.45, 0.75), (20.0, 55.0)),
                    InstitutionStyle::Balanced => (0.000, 1.00, (0.35, 0.60), (20.0, 50.0)),
                    InstitutionStyle::Defensive => (-0.010, 1.60, (0.25, 0.45), (10.0, 30.0)),
                    InstitutionStyle::ActiveTrader => (0.005, 0.65, (0.25, 0.50), (50.0, 100.0)),
                };
                // 同风格内部仍保留独立估值误差，避免机构同步行动。
                let individual_order_size =
                    sample_individual_order_size(rng, i.order_size, "order_size")?;
                let base_observation_probability = daily_observations_to_tick_probability(
                    sample_between(rng, observations.0, observations.1),
                    ticks_per_day,
                );
                if style == InstitutionStyle::ActiveTrader {
                    // 积极交易机构采用同一动量内核，但身份、资金账户与调度参数仍是机构。
                    // 不把隐藏 V 传入该策略，也不把它的限价目标交给价值机构母单。
                    let h = &params.hot;
                    let mut inner = MomentumStrategy::new(
                        h.lookback,
                        h.trend_threshold,
                        individual_order_size,
                    )?;
                    inner.style = HotStyle::Momentum;
                    inner.volume_confirmation = sample_between(rng, 0.50, 1.50);
                    inner.max_stock_fraction = sample_between(rng, max_fraction.0, max_fraction.1);
                    inner.base_observation_probability = base_observation_probability;
                    return Ok(Some(Box::new(InstitutionMomentumStrategy { style, inner })));
                }
                let bias = bias_center + sample_between(rng, -0.01, 0.01);
                let mut strategy = ValueStrategy::new(
                    TargetPolicy::TrackV { bias },
                    (i.margin * margin_factor).min(0.95),
                    individual_order_size,
                )?;
                strategy.style = style;
                strategy.max_stock_fraction = sample_between(rng, max_fraction.0, max_fraction.1);
                strategy.base_observation_probability = base_observation_probability;
                Ok(Some(Box::new(strategy)))
            }
            AccountKind::Hot => {
                let h = &params.hot;
                let individual_order_size =
                    sample_individual_order_size(rng, h.order_size, "order_size")?;
                let mut strategy =
                    MomentumStrategy::new(h.lookback, h.trend_threshold, individual_order_size)?;
                strategy.style = if ordinal.is_multiple_of(2) {
                    HotStyle::Momentum
                } else {
                    HotStyle::Reversal
                };
                strategy.volume_confirmation = sample_between(rng, 0.50, 1.50);
                strategy.max_stock_fraction = sample_between(rng, 0.10, 0.30);
                strategy.base_observation_probability = daily_observations_to_tick_probability(
                    sample_between(rng, 80.0, 300.0),
                    ticks_per_day,
                );
                Ok(Some(Box::new(strategy)))
            }
            AccountKind::Player => Ok(None),
        }
    }
}

fn daily_observations_to_tick_probability(observations_per_day: f64, ticks_per_day: u64) -> f64 {
    1.0 - (-observations_per_day / ticks_per_day as f64).exp()
}

fn sample_between(rng: &mut dyn Rng, low: f64, high: f64) -> f64 {
    low + (high - low) * rng.next_f64()
}

/// 以配置数量为群体中心，为每个 NPC 固定采样一个 60%–140% 的个体订单规模。
/// 可形成整手的基准始终返回 100 股整数倍；不足一手的显式配置保持原值，避免静默放大。
const MAX_ORDER_SIZE_BASELINE: u32 = ((u32::MAX as u64) * 5 / 7) as u32;

fn validate_order_size_sampling_range(
    baseline: u32,
    param: &'static str,
) -> Result<(), StrategyError> {
    if baseline > MAX_ORDER_SIZE_BASELINE {
        return Err(StrategyError::InvalidParam {
            param,
            reason: format!(
                "{baseline} cannot represent the complete 60%-140% sampling range; maximum is {MAX_ORDER_SIZE_BASELINE}"
            ),
        });
    }
    Ok(())
}

fn sample_individual_order_size(
    rng: &mut dyn Rng,
    baseline: u32,
    param: &'static str,
) -> Result<u32, StrategyError> {
    validate_order_size_sampling_range(baseline, param)?;
    const LOT: u64 = 100;
    if baseline < LOT as u32 {
        return Ok(baseline);
    }
    let baseline = u64::from(baseline);
    let lower_shares = baseline * 3 / 5;
    let upper_shares = baseline * 7 / 5;
    let lower_lots = lower_shares.div_ceil(LOT).max(1);
    let upper_lots = (upper_shares / LOT).max(lower_lots);
    let lots = sample_u64_inclusive(rng, lower_lots, upper_lots);
    Ok((lots * LOT) as u32)
}

fn sample_u64_inclusive(rng: &mut dyn Rng, low: u64, high: u64) -> u64 {
    let width = high - low + 1;
    low + ((rng.next_f64() * width as f64) as u64).min(width - 1)
}

#[cfg(test)]
mod order_size_sampling_tests {
    use super::{sample_individual_order_size, Rng};

    struct FixedRng(f64);

    impl Rng for FixedRng {
        fn next_f64(&mut self) -> f64 {
            self.0
        }

        fn next_range_u32(&mut self, lo: u32, _hi: u32) -> u32 {
            lo
        }
    }

    #[test]
    fn individual_order_size_uses_bounded_board_lots() {
        assert_eq!(
            sample_individual_order_size(&mut FixedRng(0.0), 2_000, "order_size").unwrap(),
            1_200
        );
        assert_eq!(
            sample_individual_order_size(&mut FixedRng(0.999_999), 2_000, "order_size").unwrap(),
            2_800
        );
        assert_eq!(
            sample_individual_order_size(&mut FixedRng(0.0), 150, "order_size").unwrap(),
            100
        );
        assert_eq!(
            sample_individual_order_size(&mut FixedRng(0.999_999), 150, "order_size").unwrap(),
            200
        );
    }

    #[test]
    fn sub_lot_configuration_is_not_silently_inflated() {
        assert_eq!(
            sample_individual_order_size(&mut FixedRng(0.5), 99, "order_size").unwrap(),
            99
        );
    }

    #[test]
    fn largest_complete_sampling_range_is_accepted_and_the_next_value_is_rejected() {
        let sampled = sample_individual_order_size(
            &mut FixedRng(0.999_999),
            super::MAX_ORDER_SIZE_BASELINE,
            "order_size",
        )
        .unwrap();
        assert_eq!(sampled % 100, 0);
        assert!(sampled > super::MAX_ORDER_SIZE_BASELINE);
        assert!(sample_individual_order_size(
            &mut FixedRng(0.5),
            super::MAX_ORDER_SIZE_BASELINE + 1,
            "order_size"
        )
        .is_err());
    }
}
