//! NPC 个体注意力/观察节奏接缝：从 session.rs 按责任抽出（W1-Task 3）。
//! 抽样函数、到期弹出与实时候选评估保持原行为；注意力 RNG 流与存档格式不变。

use super::*;

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
