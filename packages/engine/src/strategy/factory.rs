//! 策略工厂：按账户种类与序号采样个体策略实例。

use super::*;

use super::sampling::{
    daily_observations_to_tick_probability, sample_between, sample_individual_order_size,
};
use crate::account::AccountKind;

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
    ) -> Result<Option<Box<dyn ProductionStrategy>>, StrategyError> {
        Self::build_for_market_day(kind, params, 15_300, rng)
    }

    /// 使用权威交易日 tick 数把“每天观察次数”换算成逐 tick 概率。
    pub fn build_for_market_day(
        kind: AccountKind,
        params: &StrategyParams,
        ticks_per_day: u64,
        rng: &mut dyn Rng,
    ) -> Result<Option<Box<dyn ProductionStrategy>>, StrategyError> {
        Self::build_for_market_day_with_ordinal(kind, params, ticks_per_day, 0, rng)
    }

    /// 按账户在同类 NPC 中的序号分配稳定风格；序号只决定风格，不合并账户状态。
    pub fn build_for_market_day_with_ordinal(
        kind: AccountKind,
        params: &StrategyParams,
        ticks_per_day: u64,
        ordinal: u32,
        rng: &mut dyn Rng,
    ) -> Result<Option<Box<dyn ProductionStrategy>>, StrategyError> {
        if ticks_per_day == 0 {
            return Err(StrategyError::InvalidParam {
                param: "ticks_per_day",
                reason: "must be > 0".to_owned(),
            });
        }
        match kind {
            AccountKind::Retail => {
                let r = &params.retail;
                let mut strategy = ZiNoiseStrategy::new(
                    r.arrival_rate,
                    sample_individual_order_size(rng, r.order_size_mean, "order_size_mean")?,
                    r.chase_prob,
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
                strategy.position_step_bp = match strategy.retail_style {
                    RetailStyle::Dormant => 2_000,
                    RetailStyle::LongTerm => 1_625,
                    _ => rng.next_range_u32(375, 1_001),
                };
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
                    }
                    RetailStyle::LongTerm => {
                        strategy.arrival_rate *= 0.50;
                        strategy.chase_prob *= 0.40;
                        strategy.take_profit_threshold *= 1.50;
                        strategy.stop_loss_threshold *= 1.50;
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
                if !(0.0..1.0).contains(&i.margin) {
                    return Err(StrategyError::InvalidParam {
                        param: "margin",
                        reason: "must be finite and in [0, 1)".to_owned(),
                    });
                }
                let style = match ordinal % 5 {
                    0 => InstitutionStyle::DeepValue,
                    1 => InstitutionStyle::Growth,
                    2 => InstitutionStyle::Balanced,
                    3 => InstitutionStyle::Defensive,
                    _ => InstitutionStyle::ActiveTrader,
                };
                let (margin_factor, position_steps, observations) = match style {
                    InstitutionStyle::DeepValue => (1.35, (1_125, 1_750), (0.1, 0.4)),
                    InstitutionStyle::Growth => (1.00, (1_125, 1_875), (20.0, 55.0)),
                    InstitutionStyle::Balanced => (1.00, (875, 1_500), (20.0, 50.0)),
                    InstitutionStyle::Defensive => (1.60, (625, 1_125), (0.25, 0.75)),
                    InstitutionStyle::ActiveTrader => (0.65, (625, 1_250), (50.0, 100.0)),
                };
                // 同风格内部仍保留独立估值误差，避免机构同步行动。
                let individual_order_size =
                    sample_individual_order_size(rng, i.order_size, "order_size")?;
                let base_observation_probability = daily_observations_to_tick_probability(
                    sample_between(rng, observations.0, observations.1),
                    ticks_per_day,
                );
                // 所有机构风格走计划链；ActiveTrader 不依赖基本面，计划当日到期。
                // 工厂只采样个体规模、容忍带和观察节奏。
                let mut strategy = BeliefInstitutionStrategy::new(
                    (i.margin * margin_factor).min(0.95),
                    individual_order_size,
                )?;
                strategy.style = style;
                strategy.position_step_bp =
                    rng.next_range_u32(position_steps.0, position_steps.1 + 1);
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
                strategy.base_observation_probability = daily_observations_to_tick_probability(
                    sample_between(rng, 80.0, 300.0),
                    ticks_per_day,
                );
                Ok(Some(Box::new(strategy)))
            }
            // 玩家与回购专户都不产生 NPC 策略（专户委托由回购执行器产生，B4d）。
            AccountKind::Player | AccountKind::IssuerRepurchase => Ok(None),
        }
    }
}

#[cfg(test)]
mod r22b_tests {
    //! R22b 计划分类观察节奏（trading-rules.md「计划策略（游戏规则）」）：
    //! 深度价值/防御型机构不因普通开盘自动复核、随机观察频率低于每天一次；
    //! 成长/均衡型每日观察（可跨日）。手算 oracle：逐 tick 观察概率
    //! p(x)=1−e^(−x/240)（`daily_observations_to_tick_probability`，泊松
    //! 到达近似），采样区间端点取自工厂风格表（factory.rs 观察次数区间）：
    //! DeepValue (0.1,0.4)、Growth (20,55)、Balanced (20,50)、
    //! Defensive (0.25,0.75)、ActiveTrader (50,100)。低频/高频的判别线
    //! =p(1.0/日)=0.004157998155。
    use super::*;
    use crate::account::AccountKind;

    /// 固定序列 RNG（确定性、非极端值：f64∈{0.3,0.7}，区间取中点）。
    struct FixedRng(bool);
    impl Rng for FixedRng {
        fn next_f64(&mut self) -> f64 {
            self.0 = !self.0;
            if self.0 { 0.7 } else { 0.3 }
        }
        fn next_range_u32(&mut self, lo: u32, hi: u32) -> u32 {
            lo + (hi - lo) / 2
        }
    }

    fn build_institution(ordinal: u32) -> Box<dyn ProductionStrategy> {
        let params = StrategyParams {
            retail: crate::strategy::RetailParams {
                arrival_rate: 1.0,
                order_size_mean: 100,
                chase_prob: 0.0,
            },
            inst: crate::strategy::InstParams {
                margin: 0.01,
                order_size: 100,
            },
            hot: crate::strategy::HotParams {
                lookback: 2,
                trend_threshold: 0.01,
                order_size: 100,
            },
        };
        StrategyFactory::build_for_market_day_with_ordinal(
            AccountKind::Inst,
            &params,
            240,
            ordinal,
            &mut FixedRng(false),
        )
        .expect("参数合法")
        .expect("机构必产策略")
    }

    /// 风格身份 + 观察节奏 + 复核旗标逐风格锚定（ordinal%5 风格映射）。
    #[test]
    fn r22b_classification_cadence_matches_registered_policy() {
        // (ordinal, 期望风格, daily_plan_review, 观察概率区间 [lo, hi))
        // 数值手算：p(x)=1−e^(−x/240)（python: 1-math.exp(-x/240)）。
        let cases: [(u32, InstitutionStyle, bool, f64, f64); 5] = [
            // p(0.1)=0.000416579873、p(0.4)=0.001665278549
            (0, InstitutionStyle::DeepValue, false, 0.000416579873, 0.001665278549),
            // p(20)=0.079955585371、p(55)=0.204804010205
            (1, InstitutionStyle::Growth, true, 0.079955585371, 0.204804010205),
            // p(20)=0.079955585371、p(50)=0.188063653849
            (2, InstitutionStyle::Balanced, true, 0.079955585371, 0.188063653849),
            // p(0.25)=0.001041124320、p(0.75)=0.003120122270
            (3, InstitutionStyle::Defensive, false, 0.001041124320, 0.003120122270),
            // p(50)=0.188063653849、p(100)=0.340759369800
            (4, InstitutionStyle::ActiveTrader, true, 0.188063653849, 0.340759369800),
        ];
        const ONE_PER_DAY: f64 = 0.004157998155; // p(1.0/日)——低/高频判别线
        for (ordinal, want_style, want_review, lo, hi) in cases {
            let s = build_institution(ordinal);
            assert_eq!(s.profile(), StrategyProfile::Institution(want_style), "ordinal {ordinal}");
            let params = s.belief_chain_params().expect("机构必有链参数");
            assert_eq!(
                params.daily_plan_review, want_review,
                "{want_style:?}：开盘自动复核旗标（深值/防御=false）"
            );
            let state = s.state().expect("状态合法");
            let StrategyState::BeliefInstitution(bis) = state else {
                panic!("机构策略状态形态：{state:?}")
            };
            assert!(
                bis.base_observation_probability >= lo && bis.base_observation_probability < hi,
                "{want_style:?} 观察概率 {:?} 不在 [{lo}, {hi})",
                bis.base_observation_probability
            );
            // 判别线：深值/防御恒 < 1 次/日；成长/均衡/积极恒 > 1 次/日。
            if !want_review {
                assert!(
                    bis.base_observation_probability < ONE_PER_DAY,
                    "{want_style:?}：随机观察频率必须低于每天一次"
                );
            } else {
                assert!(
                    bis.base_observation_probability > ONE_PER_DAY,
                    "{want_style:?}：每日多次观察"
                );
            }
        }
    }

    /// 估值期限（跨日计划的上界来源）：深值/防御 60、均衡/成长 20、其他 5
    ///（ActiveTrader 的计划 horizon 在 lifecycle 开仓时强制 1=当日到期，
    /// 由 session 域测试承载——R22a 面）。
    #[test]
    fn r22b_belief_horizon_days_by_classification() {
        let cases = [
            (0, InstitutionStyle::DeepValue, 60u16),
            (1, InstitutionStyle::Growth, 20),
            (2, InstitutionStyle::Balanced, 20),
            (3, InstitutionStyle::Defensive, 60),
            (4, InstitutionStyle::ActiveTrader, 5),
        ];
        for (ordinal, style, want) in cases {
            let s = build_institution(ordinal);
            assert_eq!(
                super::super::fundamental::belief_horizon_days(&s.profile()),
                want,
                "{style:?} 估值期限（计划可跨日的上界）"
            );
        }
    }
}
