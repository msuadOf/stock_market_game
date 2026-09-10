//! 散户 ZiNoiseStrategy：trait 封壳，委托数据驱动内核并叠加个体行为判断。

use super::*;

use super::retail::decide_retail;
use crate::behavior::{decide_retail_position, decide_retail_position_with_experience};

/// 散户：噪音到达 + 追涨、下跌抄底与亏损止损。
///
/// 在个体观察 tick 上以 arrival_rate 概率「到达」；若到达，按 chase_prob 概率
/// 顺近期趋势。下跌时，未触发止损者可尝试抄底，达到动态止损线且可卖者主动止损；
/// 当日买入仓位遵守 T+1。非趋势分支仍随机选择方向，但无资产支持时不产生无效意图。
/// 纯逻辑：所有随机经注入 `&mut dyn Rng`（可重放、可单测）；价格用 Money，禁止 f64 存储权威状态。
pub struct ZiNoiseStrategy {
    pub(super) retail_style: RetailStyle,
    /// 每次观察时的到达概率，∈[0,1]。0 → 永不动作。
    pub(super) arrival_rate: f64,
    /// 个体每单股数；工厂以配置值为群体中心采样。
    order_size_mean: u32,
    /// 追势概率，∈[0,1]。
    pub(super) chase_prob: f64,
    /// 价格跨 tick 的「分」数（>0）。
    tick_cents: i64,
    pub(super) dip_threshold: f64,
    pub(super) stop_loss_threshold: f64,
    pub(super) take_profit_threshold: f64,
    pub(super) volume_confirmation: f64,
    pub(super) max_stock_fraction: f64,
    pub(super) base_observation_probability: f64,
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
