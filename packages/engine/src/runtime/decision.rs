//! 组线程内联决策（批 4）：到期检查 → 读行情快照（只读）→ `decide_data` 纯函数
//! → Intent → DeskIn（含市价路径）。
//!
//! 业务语义照搬脊树（`strategy/data.rs decide_data` retail/inst/hot 三族），
//! 执行形态改为组线程内联直调——不经 ComputeContext/rayon dispatch。
//!
//! - 观察基：tick 开始的行情快照（协调者广播 `Arc<TickSnapshot>` 附带全股
//!   上一收束点公共态；组端只读）。
//! - RNG：`SplitMix64(seed ^ tick ^ local)`，同种子同输出（铁律三）。
//! - 到期调度：attention 简化为每 NPC `next_due` 单变量，due 即决策，
//!   决策后按间隔上限重排（对齐 spine attention 的"概率驱动观察"，公式简化
//!   登记：信号加权观察概率随批 5 附属状态下沉接入）。
//! - 严格所有权：决策读到的 cash/持仓即组端权威（在途实体已扣除）。
//! - `Intent::Cancel` 依赖 OrderRef 账户侧跟踪（FillFact/回执驱动），批 6 计划
//!   续行接线前以显式计数登记，不静默丢弃。

use super::group::GroupAccount;
use super::protocol::{RoutedPrice, TickSnapshot};
use super::{LocalIdx, StockIdx};
use crate::config::GameConfig;
use crate::money::Money;
use crate::orderbook::Side;
use crate::session::SplitMix64;
use crate::strategy::Rng as _;
use crate::strategy::{
    decide_data, Intent, LimitPrice, MarketView, PositionView, SelfView, StockView, StrategyData,
};
use crate::StockId;
use std::collections::BTreeMap;

/// 组内单 NPC 决策档案。
#[derive(Clone, Debug)]
pub struct NpcProfile {
    /// 策略参数（retail/inst/hot 三族数据驱动）。
    pub strategy: StrategyData,
    /// 独立随机种子（D5 #2 起=注意力共享流续传——与 session rng_state
    /// 同源；决策流另经 seed 派生，见 decide_account）。
    pub seed: u64,
    /// 下次到期 tick（attention 简化态）。
    pub next_due: u64,
    /// 个体基础观察概率（session npc_attention.base_probability 构造期
    /// 投影——个体采样值，非 strategy 的群体基准）。
    pub attention_base: f64,
}

/// 组决策装配（世界构造期确定）。
#[derive(Clone, Debug)]
pub struct DecisionSetup {
    /// 每组内账户的决策档案（下标=LocalIdx；各组同构，种子由世界按组复合）。
    pub profiles: Vec<NpcProfile>,
    /// 到期间隔上限（tick）：重排 next_due = tick + 1 + U[0, gap)；0 ⇒ 每 tick due。
    pub attention_gap_ticks: u32,
}

/// 一次决策产出的待执行操作（组端统一走实体扣除路径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingOp {
    /// 组内名册下标。
    pub local: LocalIdx,
    /// 目标股。
    pub stock: StockIdx,
    /// 方向。
    pub side: Side,
    /// 路由价格（Fixed/Highest/Lowest/Market）。
    pub routed: RoutedPrice,
    /// purse 最坏情况参考价（符号价/市价 = 日涨停；F §1.5）。
    pub purse_reference: i64,
    /// 数量。
    pub qty: u32,
}

/// 到期检查 + 决策 + 意图转换（单 NPC）。
/// 返回 (ops, decided, cancel_skipped)：`decided=false` 表示本 tick 未到期。
#[allow(clippy::too_many_arguments)]
/// D5 #2：signal-based attention evaluate（旧管线 `evaluate_candidate_
/// with_signal` 同式——共享流续传（profile.seed=注意力 rng_state 同源）+
/// observe 门 + wait 采样；返回 (observe 判定, 排期前值)。每 due 恰一次。
pub fn evaluate_attention(
    profile: &mut NpcProfile,
    snapshot: &TickSnapshot,
    tick: u64,
) -> (bool, u64) {
    let market = build_market_view(snapshot);
    let signal = crate::session::market_attention_signal(&market);
    let kind = profile.strategy.kind;
    let base = profile.attention_base;
    let mut rng = crate::session::SplitMix64::new(profile.seed);
    let observes =
        crate::session::attention_candidate_is_observation(kind, base, signal, &mut rng);
    let wait = crate::session::sample_attention_wait(
        crate::session::maximum_observation_probability(kind, base),
        &mut rng,
    );
    let prev_due = profile.next_due;
    profile.seed = rng.state();
    profile.next_due = tick + wait;
    (observes, prev_due)
}

pub fn decide_account(
    profile: &mut NpcProfile,
    group_seed_mix: u64,
    tick: u64,
    local: LocalIdx,
    snapshot: &TickSnapshot,
    account: &GroupAccount,
    config: &GameConfig,
) -> (Vec<PendingOp>, usize) {
    // D5 #2：due/observe 门由调用方（run_round 的 evaluate 链）控制——
    /// 本函数纯决策（此前内嵌 due 判在 evaluate 排期后恒假）。
    // 决策 RNG：注意力流（evaluate 续传）经组混合因子派生——决策流与
    /// 调度流分离（旧管线策略流独立于 attention rng；组内经派生确定化）。
    let mut rng = SplitMix64::new(profile.seed ^ group_seed_mix ^ tick ^ local as u64);
    let market = build_market_view(snapshot);
    let own = build_self_view(account);
    let intents = decide_data(&profile.strategy, &market, &own, &mut rng, config);
    let mut ops = Vec::with_capacity(intents.len());
    let mut cancel_skipped = 0usize;
    for intent in intents {
        if let Some(op) = intent_to_pending(intent, &market) {
            ops.push(PendingOp { local, ..op });
        } else {
            cancel_skipped += 1;
        }
    }
    (ops, cancel_skipped)
}

/// 单条 Intent → PendingOp（None=Cancel 意图：OrderRef 账户侧跟踪随批 6 计划
/// 续行接线，显式计数不静默丢弃）。产出的 `local=0` 占位，调用方按名册覆盖。
pub fn intent_to_pending(intent: Intent, market: &MarketView) -> Option<PendingOp> {
    match intent {
        Intent::PlaceLimit {
            code,
            side,
            price,
            qty,
        } => {
            let (routed, purse_reference) = match price {
                LimitPrice::Fixed(p) => (RoutedPrice::Fixed(p.cents()), p.cents()),
                // 符号价 purse 最坏情况按日涨停（组端只有公共态，F §1.5）。
                LimitPrice::Highest | LimitPrice::Lowest => {
                    (symbol_routed(price), daily_upper(market, code))
                }
            };
            Some(PendingOp {
                local: 0,
                stock: code.0 as StockIdx,
                side,
                routed,
                purse_reference,
                qty,
            })
        }
        Intent::PlaceMarket { code, side, qty } => {
            // 市价路径：涨跌停保护、余量不进簿；purse 最坏情况按日涨停。
            Some(PendingOp {
                local: 0,
                stock: code.0 as StockIdx,
                side,
                routed: RoutedPrice::Market,
                purse_reference: daily_upper(market, code),
                qty,
            })
        }
        Intent::Cancel { .. } => None,
    }
}

fn symbol_routed(price: LimitPrice) -> RoutedPrice {
    match price {
        LimitPrice::Highest => RoutedPrice::Highest,
        LimitPrice::Lowest => RoutedPrice::Lowest,
        LimitPrice::Fixed(_) => unreachable!("仅符号价走此分支"),
    }
}

/// D3a-3：到期重排提取（retail 与 belief 链位同一公式——next_due 演进
/// 是组内调度权威 P1-10；Sol 二审修复集 5 的 undo 语义由调用方承接）。


fn daily_upper(market: &MarketView, code: StockId) -> i64 {
    market
        .stocks
        .get(&code)
        .map(|v| v.daily_upper_limit.cents())
        .unwrap_or(0)
}

/// 广播快照 → 策略 MarketView（只读转换；StockId 数值直映射）。
pub fn build_market_view(snapshot: &TickSnapshot) -> MarketView {
    let mut stocks = BTreeMap::new();
    for (idx, s) in snapshot.stocks.iter().enumerate() {
        stocks.insert(
            StockId(idx as u32),
            StockView {
                best_bid: s.best_bid.map(Money::from_cents),
                best_ask: s.best_ask.map(Money::from_cents),
                last_price: Money::from_cents(s.last_price),
                max_buy_price: Money::from_cents(s.max_buy_price),
                daily_upper_limit: Money::from_cents(s.daily_upper_limit),
                min_sell_price: Money::from_cents(s.min_sell_price),
                recent_prices: s
                    .recent_prices
                    .iter()
                    .map(|&c| Money::from_cents(c))
                    .collect(),
                recent_market_minute_prices: s
                    .minute_closes
                    .iter()
                    .map(|&(_, c)| Money::from_cents(c))
                    .collect(),
                relative_volume: s.relative_volume,
                order_book_imbalance: s.order_book_imbalance,
            },
        );
    }
    MarketView {
        stocks,
        tick: snapshot.tick,
        market_minute: snapshot.market_minute as u64,
    }
}

/// 组账户 → 策略 SelfView（严格所有权口径：余额即所见，在途已扣除）。
pub fn build_self_view(account: &GroupAccount) -> SelfView {
    let mut positions = BTreeMap::new();
    for (idx, (&qty, &frozen)) in account
        .shares
        .iter()
        .zip(account.frozen_today.iter())
        .enumerate()
    {
        positions.insert(
            StockId(idx as u32),
            PositionView {
                qty,
                sellable_qty: qty - frozen,
                cost_price: None,
            },
        );
    }
    SelfView {
        cash: Money::from_cents(account.cash),
        positions,
    }
}
