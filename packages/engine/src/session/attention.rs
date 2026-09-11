//! NPC 个体注意力/观察节奏接缝：从 session.rs 按责任抽出（W1-Task 3）。
//! 抽样函数、到期弹出与实时候选评估保持原行为；注意力 RNG 流与存档格式不变。
//! K4 个人发现权重与个体发现抽样（W4-Task 25）在本文件 additive 追加。

use super::*;
use crate::experience::PersonalWatchlist;

/// 单个 NPC 的随机注意力状态。市场越活跃，下一次观察的有效概率越高。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct NpcAttentionState {
    /// 平静市场下每 tick 至少观察一次的基础概率，∈(0,1]。
    pub base_probability: f64,
    /// 下一次评估实时观察概率的候选绝对游戏 tick。
    #[serde(with = "u64_decimal")]
    #[ts(type = "string")]
    pub next_attention_candidate_tick: u64,
    /// 该 NPC 独立注意力随机流的状态。
    #[serde(with = "u64_decimal")]
    #[ts(type = "string")]
    pub rng_state: u64,
}

fn market_attention_signal(market: &MarketView) -> f64 {
    market.stocks.values().fold(0.0, |strongest, stock| {
        let price_move = stock
            .recent_prices
            .first()
            .filter(|price| price.cents() > 0)
            .map_or(0.0, |first| {
                ((stock.last_price.cents() - first.cents()).unsigned_abs() as f64
                    / first.cents() as f64
                    / 0.02)
                    .min(3.0)
            });
        let volume = if stock.relative_volume.is_finite() {
            (stock.relative_volume - 1.0).clamp(0.0, 3.0)
        } else {
            0.0
        };
        let imbalance = if stock.order_book_imbalance.is_finite() {
            stock.order_book_imbalance.abs().min(1.0)
        } else {
            0.0
        };
        strongest.max(price_move.max(volume).max(imbalance))
    })
}

fn effective_observation_probability(
    kind: AccountKind,
    base_probability: f64,
    market: &MarketView,
) -> f64 {
    let sensitivity = match kind {
        AccountKind::Retail => 0.75,
        AccountKind::Inst => 0.40,
        AccountKind::Hot => 1.25,
        AccountKind::Player => 0.0,
    };
    (base_probability * (1.0 + sensitivity * market_attention_signal(market))).min(1.0)
}

pub(super) fn maximum_observation_probability(kind: AccountKind, base_probability: f64) -> f64 {
    let sensitivity = match kind {
        AccountKind::Retail => 0.75,
        AccountKind::Inst => 0.40,
        AccountKind::Hot => 1.25,
        AccountKind::Player => 0.0,
    };
    (base_probability * (1.0 + sensitivity * 3.0)).min(1.0)
}

fn attention_candidate_is_observation(
    kind: AccountKind,
    base_probability: f64,
    market: &MarketView,
    rng: &mut dyn Rng,
) -> bool {
    let maximum = maximum_observation_probability(kind, base_probability);
    let current = effective_observation_probability(kind, base_probability, market);
    rng.next_f64() < current / maximum
}

pub(super) fn sample_attention_wait(probability: f64, rng: &mut dyn Rng) -> u64 {
    debug_assert!(probability.is_finite() && probability > 0.0 && probability <= 1.0);
    if probability >= 1.0 {
        return 1;
    }
    let draw = rng.next_f64();
    let wait = ((1.0 - draw).ln() / (-probability).ln_1p()).floor() + 1.0;
    wait.clamp(1.0, u64::MAX as f64) as u64
}

// ---- K4 个人发现权重与个体发现抽样（W4-Task 25；会话接线归任务 26） ----
//
// 发现面输入只有两类**公共**数据：市场视图（行情/量能）与公告曝光集合
// （由任务 16 `information::discovery_candidates` 公开面派生）。私有经营
// 事实（未披露总账/CompanyState 内部）结构性不可达；新曝光只提高发现
// 机会，绝不自动获知（获知只能经 `record_acquisition`，任务 16 语义）。

/// 30 分钟窗口 |涨跌| 达到该比例视为异常（版本化待校准游戏假设）。
const DISCOVERY_ANOMALY_MOVE_RATIO: f64 = 0.02;
/// 相对量能达到该倍率视为异常。
const DISCOVERY_ANOMALY_RELATIVE_VOLUME: f64 = 2.0;
/// 三类公共信号的发现权重加成；基础权重恒为 1.0（全市场发现机会保留）。
const DISCOVERY_MOVE_BOOST: f64 = 1.0;
const DISCOVERY_VOLUME_BOOST: f64 = 1.0;
const DISCOVERY_ANNOUNCEMENT_BOOST: f64 = 2.0;
/// 继承 behavior 选股的持仓优先基础概率：60% 持仓优先、40% 全市场发现。
const HELD_PRIORITY_PROBABILITY: f64 = 0.60;
/// 继承 behavior 选股的关注列表偏好概率：发现分支先看关注列表。
const WATCHLIST_BIAS_PROBABILITY: f64 = 0.70;
/// 「30 分钟涨跌」信号最多回看的标准交易分钟数。
const DISCOVERY_MOVE_WINDOW_MINUTES: usize = 30;

/// 最近 ≤30 个已完成交易分钟内 |涨跌| 是否达到异常阈值。
///
/// 不足两个完整分钟或窗口首价非正时无信号——缺样本不伪装异常。
fn abnormal_thirty_minute_move(view: &StockView) -> bool {
    let closes = &view.recent_market_minute_prices;
    if closes.len() < 2 {
        return false;
    }
    let start = closes.len().saturating_sub(DISCOVERY_MOVE_WINDOW_MINUTES);
    let first = closes[start].cents();
    if first <= 0 {
        return false;
    }
    let last = closes[closes.len() - 1].cents();
    let change = (last - first).unsigned_abs() as f64 / first as f64;
    change >= DISCOVERY_ANOMALY_MOVE_RATIO
}

/// 相对量能是否达到异常阈值（非有限值 = 无同期预期量基准，无信号）。
fn abnormal_relative_volume(view: &StockView) -> bool {
    view.relative_volume.is_finite() && view.relative_volume >= DISCOVERY_ANOMALY_RELATIVE_VOLUME
}

/// 单股公共信号发现权重：1.0 基础 + 异常涨跌/异常量能/公告曝光加成。
fn stock_discovery_weight(view: &StockView, announcement_exposed: bool) -> f64 {
    let mut weight = 1.0;
    if abnormal_thirty_minute_move(view) {
        weight += DISCOVERY_MOVE_BOOST;
    }
    if abnormal_relative_volume(view) {
        weight += DISCOVERY_VOLUME_BOOST;
    }
    if announcement_exposed {
        weight += DISCOVERY_ANNOUNCEMENT_BOOST;
    }
    weight
}

/// 按权重从候选池抽样（BTreeMap 确定性顺序；恰好一次 `next_f64` 抽样）。
fn pick_weighted(
    market: &MarketView,
    candidates: &[&StockCode],
    announcement_exposed: &BTreeSet<StockCode>,
    rng: &mut dyn Rng,
) -> Option<StockCode> {
    if candidates.is_empty() {
        return None;
    }
    let total: f64 = candidates
        .iter()
        .map(|code| {
            stock_discovery_weight(&market.stocks[*code], announcement_exposed.contains(*code))
        })
        .sum();
    let draw = rng.next_f64() * total;
    let mut cumulative = 0.0;
    for code in candidates {
        cumulative +=
            stock_discovery_weight(&market.stocks[*code], announcement_exposed.contains(*code));
        if draw < cumulative {
            return Some((*code).clone());
        }
    }
    // 末位候选拥有 [Σ前缀, total) 区间；next_f64 < 1 使 draw < total，
    // 浮点边缘也必须落在末位区间内，绝不静默丢弃。
    Some((*candidates[candidates.len() - 1]).clone())
}

/// 个体发现选股内核：继承原候选结构（60% 持仓优先 → 70% 关注列表 →
/// 全市场），关注列表池与全市场池按公共信号发现权重加权。RNG 消费顺序
/// 与原 `behavior::select_observed_stock` 同构：各分支恰好一次门抽样 +
/// 一次选择抽样，仅选择抽样由 `next_range_u32` 换成一次加权 `next_f64`。
fn select_discovery_stock(
    market: &MarketView,
    held: &BTreeSet<StockCode>,
    watchlist: &PersonalWatchlist,
    announcement_exposed: &BTreeSet<StockCode>,
    rng: &mut dyn Rng,
) -> Option<StockCode> {
    let held_in_market: Vec<&StockCode> = held
        .iter()
        .filter(|code| market.stocks.contains_key(*code))
        .collect();
    if !held_in_market.is_empty() && rng.next_f64() < HELD_PRIORITY_PROBABILITY {
        let index = rng.next_range_u32(0, held_in_market.len() as u32) as usize;
        return Some(held_in_market[index].clone());
    }
    let watched_in_market: Vec<&StockCode> = watchlist
        .stocks
        .keys()
        .filter(|code| market.stocks.contains_key(*code))
        .collect();
    if !watched_in_market.is_empty() && rng.next_f64() < WATCHLIST_BIAS_PROBABILITY {
        return pick_weighted(market, &watched_in_market, announcement_exposed, rng);
    }
    if market.stocks.is_empty() {
        return None;
    }
    let all: Vec<&StockCode> = market.stocks.keys().collect();
    pick_weighted(market, &all, announcement_exposed, rng)
}

impl NpcAttentionState {
    /// 公共信号发现权重表（K4）：1.0 基础 + 异常30分钟涨跌/相对量能/公告
    /// 曝光加成。纯函数 of (公共市场视图, 公开曝光集合)——私有经营事实
    /// 结构性不可达；公告曝光集合由调用方从任务 16 `discovery_candidates`
    /// 面派生（公司↔股票映射与曝光新鲜度窗口归会话接线，任务 26）。
    pub fn discovery_weights(
        market: &MarketView,
        announcement_exposed: &BTreeSet<StockCode>,
    ) -> BTreeMap<StockCode, f64> {
        market
            .stocks
            .iter()
            .map(|(code, view)| {
                (
                    code.clone(),
                    stock_discovery_weight(view, announcement_exposed.contains(code)),
                )
            })
            .collect()
    }

    /// 个体发现抽样（K4/K6）：继承原候选结构——60% 持仓优先（均匀，异常
    /// 权重不进入持仓分支）→ 70% 关注列表（加权）→ 全市场（加权）。
    ///
    /// 消费本 NPC 的个体注意力 RNG 流（继承原候选个体 RNG；推进
    /// `rng_state`）。`held` 来自权威账户持仓；选中的候选由接线在观察被
    /// 接受时写入关注列表（`PersonalWatchlist::record_attention`），获知
    /// 内容只能另行经 `record_acquisition`——新曝光不等于已读。
    pub fn sample_discovery_stock(
        &mut self,
        market: &MarketView,
        held: &BTreeSet<StockCode>,
        watchlist: &PersonalWatchlist,
        announcement_exposed: &BTreeSet<StockCode>,
    ) -> Option<StockCode> {
        let mut rng = SplitMix64::new(self.rng_state);
        let picked =
            select_discovery_stock(market, held, watchlist, announcement_exposed, &mut rng);
        self.rng_state = rng.state;
        picked
    }
}

impl GameSession {
    pub(super) fn pop_due_npc_ids(&mut self, tick: u64) -> Vec<AccountId> {
        let mut due = Vec::new();
        while let Some(Reverse((scheduled_tick, _))) = self.attention_queue.peek() {
            if *scheduled_tick > tick {
                break;
            }
            let Reverse((scheduled_tick, id)) = self
                .attention_queue
                .pop()
                .expect("peeked attention entry must still exist");
            if self
                .npc_attention
                .get(&id)
                .is_some_and(|state| state.next_attention_candidate_tick == scheduled_tick)
            {
                due.push(id);
            }
        }
        due.sort_unstable();
        due.dedup();
        due
    }

    pub(super) fn evaluate_attention_candidate(
        &mut self,
        id: AccountId,
        market: &MarketView,
    ) -> bool {
        let kind = self
            .accounts
            .get(&id)
            .expect("attention queue may only contain existing NPC accounts")
            .kind;
        let state = self
            .npc_attention
            .get_mut(&id)
            .expect("every scheduled NPC must have attention state");
        let mut rng = SplitMix64::new(state.rng_state);
        let observes =
            attention_candidate_is_observation(kind, state.base_probability, market, &mut rng);
        let wait = sample_attention_wait(
            maximum_observation_probability(kind, state.base_probability),
            &mut rng,
        );
        state.rng_state = rng.state;
        state.next_attention_candidate_tick = self.tick.saturating_add(wait);
        self.attention_queue
            .push(Reverse((state.next_attention_candidate_tick, id)));
        observes
    }
}

#[cfg(test)]
mod attention_tests {
    use super::*;

    struct SequenceRng {
        values: Vec<f64>,
        index: usize,
    }

    impl Rng for SequenceRng {
        fn next_f64(&mut self) -> f64 {
            let value = self.values[self.index.min(self.values.len() - 1)];
            self.index += 1;
            value
        }

        fn next_range_u32(&mut self, lo: u32, _hi: u32) -> u32 {
            lo
        }
    }

    fn attention_view(last: i64, first: i64, relative_volume: f64, imbalance: f64) -> MarketView {
        MarketView {
            tick: 7,
            market_minute: 0,
            stocks: [(
                StockCode("600888".to_string()),
                StockView {
                    best_bid: Some(Money::from_cents(last - 1)),
                    best_ask: Some(Money::from_cents(last + 1)),
                    last_price: Money::from_cents(last),
                    fundamental_value: None,
                    recent_prices: vec![Money::from_cents(first), Money::from_cents(last)],
                    recent_market_minute_prices: vec![
                        Money::from_cents(first),
                        Money::from_cents(last),
                    ],
                    relative_volume,
                    order_book_imbalance: imbalance,
                },
            )]
            .into(),
        }
    }

    #[test]
    fn market_activity_monotonically_increases_attention_probability() {
        let quiet = attention_view(1_000, 1_000, 1.0, 0.0);
        let active = attention_view(940, 1_000, 2.5, -0.8);

        for kind in [AccountKind::Retail, AccountKind::Inst, AccountKind::Hot] {
            let quiet_probability = effective_observation_probability(kind, 0.10, &quiet);
            let active_probability = effective_observation_probability(kind, 0.10, &active);
            assert_eq!(quiet_probability, 0.10);
            assert!(active_probability > quiet_probability, "{kind:?}");
            assert!(active_probability <= 1.0, "{kind:?}");
        }
    }

    #[test]
    fn thinning_uses_market_state_at_the_candidate_tick() {
        let quiet = attention_view(1_000, 1_000, 1.0, 0.0);
        let active = attention_view(940, 1_000, 2.5, -0.8);
        let mut quiet_rng = SequenceRng {
            values: vec![0.5],
            index: 0,
        };
        let mut active_rng = SequenceRng {
            values: vec![0.5],
            index: 0,
        };

        assert!(!attention_candidate_is_observation(
            AccountKind::Retail,
            0.10,
            &quiet,
            &mut quiet_rng,
        ));
        assert!(attention_candidate_is_observation(
            AccountKind::Retail,
            0.10,
            &active,
            &mut active_rng,
        ));
    }

    #[test]
    fn attention_wait_samples_the_exact_geometric_distribution_without_a_short_tail_cap() {
        let mut succeeds_on_third = SequenceRng {
            values: vec![0.75],
            index: 0,
        };
        assert_eq!(sample_attention_wait(0.5, &mut succeeds_on_third), 3);

        let mut long_wait = SequenceRng {
            values: vec![0.999_999],
            index: 0,
        };
        assert!(sample_attention_wait(0.000_001, &mut long_wait) > 10_000);
    }
}
