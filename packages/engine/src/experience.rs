//! 独立散户账户的最小、可恢复经历状态。
//!
//! 只有真实成交和真实观察可以修改这里的状态；委托意图本身不构成经历。
//! K5 个人价格记忆（本人所见锚点与公开历史读取事件）在 [`price_memory`]
//! 子模块；K5 经历反馈（受挫日期/生命周期/冷静期历史与衰减读取）在
//! [`feedback`] 子模块；K5 个人关注列表（发现、保留与淡出）在
//! [`watchlist`] 子模块；`experience.rs` 保持模块入口（Rust 2018 布局），
//! 全部原有公共路径不变。

mod feedback;
mod price_memory;
mod watchlist;

pub use feedback::{
    ExitRecord, ExperienceFeedback, ExperienceMoment, FAILURE_DECAY_TRADING_DAYS,
    FailureEventRecord, HoldingEpoch, LONG_STUCK_TRADING_DAYS, OwnObservation,
};
pub use price_memory::{PersonalPriceMemory, PriceMemoryError, StockPriceMemory};
pub use watchlist::{PersonalWatchlist, WatchedStock, WatchlistError};

use std::collections::{BTreeMap, BTreeSet};

use crate::calendar::CivilDate;
use crate::{Money, Side, StockCode};

pub const POST_EXIT_COOLDOWN_MINUTES: u64 = 120;
pub const MAX_UNHELD_WATCHLIST_STOCKS: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ExperienceError {
    #[error("{field} must be positive, got {cents} cents")]
    NonPositiveMoney { field: &'static str, cents: i64 },
    #[error("{side:?} fill has invalid position transition {before_qty} -> {after_qty}")]
    InvalidPositionTransition {
        side: Side,
        before_qty: u32,
        after_qty: u32,
    },
    #[error("retail experience counter overflow")]
    CounterOverflow,
    #[error("market-minute overflow from {minute} + {increment}")]
    MarketMinuteOverflow { minute: u64, increment: u64 },
    #[error("civil clock cannot go backwards: attempted {attempted} after last {last}")]
    CivilTimeWentBackwards {
        attempted: CivilDate,
        last: CivilDate,
    },
    #[error("market-minute clock cannot go backwards: attempted {attempted} after last {last}")]
    MarketMinuteWentBackwards { attempted: u64, last: u64 },
    #[error("trading-day clock cannot go backwards: attempted {attempted} after last {last}")]
    TradingDayWentBackwards { attempted: u64, last: u64 },
    #[error(
        "as-of {as_of:?} precedes recorded experience {latest:?}; the state holds future-dated experience"
    )]
    AsOfBeforeLatestEvent {
        as_of: feedback::ExperienceMoment,
        latest: feedback::ExperienceMoment,
    },
    #[error("stock {code} already has an active holding epoch; its exit lifecycle is missing")]
    ActiveEntryAlreadyExists { code: String },
    #[error("stock {code} has no active holding epoch; its entry lifecycle is missing")]
    NoActiveEntry { code: String },
    #[error("inconsistent experience feedback state: {detail}")]
    InconsistentFeedback { detail: String },
}

/// 一只股票的成交与观察经历。退出后仍暂存，用于冷静期和关注列表。
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct RetailStockExperience {
    pub entry_reference_price: Option<Money>,
    pub peak_price_since_entry: Option<Money>,
    pub last_buy_price: Option<Money>,
    pub adverse_move_recorded: bool,
    #[serde(with = "optional_u64_decimal")]
    #[ts(type = "string | null")]
    pub last_buy_order_id: Option<u64>,
    #[serde(with = "optional_u64_decimal")]
    #[ts(type = "string | null")]
    pub last_sell_order_id: Option<u64>,
    #[serde(with = "u64_decimal")]
    #[ts(type = "string")]
    pub last_trade_market_minute: u64,
    #[serde(with = "u64_decimal")]
    #[ts(type = "string")]
    pub last_observed_market_minute: u64,
    #[serde(with = "optional_u64_decimal")]
    #[ts(type = "string | null")]
    pub cooldown_until_market_minute: Option<u64>,
}

/// 每个自然人散户独立持有的最小经历；不与其他账户共享。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct RetailExperienceState {
    pub reference_equity: Option<Money>,
    pub peak_equity: Option<Money>,
    pub consecutive_failed_buys: u16,
    pub stocks: BTreeMap<StockCode, RetailStockExperience>,
    /// K5 经历反馈事实（任务 20）：受挫事件日期、持仓生命周期、退出/冷静期
    /// 历史。默认空 = 新账户或尚未接双时钟事件（序列化时省略，旧档字节与
    /// 读取语义不变）；衰减只作用于读取档位，不删除这里登记的任何事实。
    #[serde(default, skip_serializing_if = "ExperienceFeedback::is_empty")]
    pub feedback: ExperienceFeedback,
}

impl RetailExperienceState {
    pub fn new(reference_equity: Money) -> Result<Self, ExperienceError> {
        require_positive("reference equity", reference_equity)?;
        Ok(Self {
            reference_equity: Some(reference_equity),
            peak_equity: Some(reference_equity),
            consecutive_failed_buys: 0,
            stocks: BTreeMap::new(),
            feedback: ExperienceFeedback::default(),
        })
    }

    pub fn without_equity_reference() -> Self {
        Self {
            reference_equity: None,
            peak_equity: None,
            consecutive_failed_buys: 0,
            stocks: BTreeMap::new(),
            feedback: ExperienceFeedback::default(),
        }
    }

    pub fn observe_equity(&mut self, equity: Money) -> Result<(), ExperienceError> {
        require_positive("observed equity", equity)?;
        if self.reference_equity.is_none() {
            self.reference_equity = Some(equity);
        }
        self.peak_equity = Some(self.peak_equity.map_or(equity, |peak| peak.max(equity)));
        Ok(())
    }

    /// 为开局已分配的真实持仓建立参照；它不是一笔游戏内成交，不产生买入成败。
    pub fn initialize_holding(
        &mut self,
        code: &StockCode,
        entry_reference_price: Option<Money>,
        current_price: Money,
        market_minute: u64,
    ) -> Result<(), ExperienceError> {
        require_positive("initial holding price", current_price)?;
        if let Some(reference) = entry_reference_price {
            require_positive("initial entry reference", reference)?;
        }
        self.stocks.insert(
            code.clone(),
            RetailStockExperience {
                entry_reference_price,
                peak_price_since_entry: Some(current_price),
                last_observed_market_minute: market_minute,
                ..RetailStockExperience::default()
            },
        );
        Ok(())
    }

    pub fn observe_position(
        &mut self,
        code: &StockCode,
        price: Money,
        market_minute: u64,
    ) -> Result<(), ExperienceError> {
        require_positive("observed position price", price)?;
        let stock = self.stocks.entry(code.clone()).or_default();
        stock.last_observed_market_minute = market_minute;
        if stock.entry_reference_price.is_some() {
            stock.peak_price_since_entry = Some(
                stock
                    .peak_price_since_entry
                    .map_or(price, |peak| peak.max(price)),
            );
        }
        let adverse = stock.last_buy_price.is_some_and(|buy_price| {
            i128::from(price.cents()) * 100 <= i128::from(buy_price.cents()) * 95
        });
        if adverse && !stock.adverse_move_recorded {
            self.consecutive_failed_buys = self
                .consecutive_failed_buys
                .checked_add(1)
                .ok_or(ExperienceError::CounterOverflow)?;
            stock.adverse_move_recorded = true;
        }
        Ok(())
    }

    /// 记录一次真实的个股查看。未持仓时只更新关注列表，不凭空生成成交经历。
    pub fn observe_stock(&mut self, code: &StockCode, market_minute: u64) {
        self.stocks
            .entry(code.clone())
            .or_default()
            .last_observed_market_minute = market_minute;
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_fill(
        &mut self,
        code: &StockCode,
        side: Side,
        price: Money,
        before_qty: u32,
        after_qty: u32,
        cost_before: Option<Money>,
        market_minute: u64,
    ) -> Result<(), ExperienceError> {
        self.record_fill_with_order(
            code,
            side,
            price,
            before_qty,
            after_qty,
            cost_before,
            market_minute,
            None,
        )
    }

    /// 记录一次已结算订单的成交；同一订单的跨 tick 部分成交只构成一次心理决策。
    #[allow(clippy::too_many_arguments)]
    pub fn record_fill_with_order(
        &mut self,
        code: &StockCode,
        side: Side,
        price: Money,
        before_qty: u32,
        after_qty: u32,
        cost_before: Option<Money>,
        market_minute: u64,
        order_id: Option<u64>,
    ) -> Result<(), ExperienceError> {
        require_positive("fill price", price)?;
        if let Some(cost) = cost_before {
            require_positive("cost before fill", cost)?;
        }
        let valid_transition = match side {
            Side::Buy => after_qty > before_qty,
            Side::Sell => after_qty < before_qty,
        };
        if !valid_transition {
            return Err(ExperienceError::InvalidPositionTransition {
                side,
                before_qty,
                after_qty,
            });
        }

        let stock = self.stocks.entry(code.clone()).or_default();
        stock.last_trade_market_minute = market_minute;
        stock.last_observed_market_minute = market_minute;
        match side {
            Side::Buy => {
                stock.last_buy_price = Some(price);
                if order_id.is_none_or(|id| stock.last_buy_order_id != Some(id)) {
                    stock.adverse_move_recorded = false;
                }
                stock.last_buy_order_id = order_id;
                stock.cooldown_until_market_minute = None;
                if before_qty == 0 {
                    stock.entry_reference_price = Some(price);
                    stock.peak_price_since_entry = Some(price);
                } else {
                    stock.peak_price_since_entry = Some(
                        stock
                            .peak_price_since_entry
                            .map_or(price, |peak| peak.max(price)),
                    );
                }
            }
            Side::Sell => {
                if order_id.is_none_or(|id| stock.last_sell_order_id != Some(id)) {
                    if let Some(cost) = cost_before {
                        if price > cost {
                            self.consecutive_failed_buys =
                                self.consecutive_failed_buys.saturating_sub(1);
                        } else if price < cost && !stock.adverse_move_recorded {
                            self.consecutive_failed_buys = self
                                .consecutive_failed_buys
                                .checked_add(1)
                                .ok_or(ExperienceError::CounterOverflow)?;
                            stock.adverse_move_recorded = true;
                        }
                    }
                }
                stock.last_sell_order_id = order_id;
                if after_qty == 0 {
                    stock.entry_reference_price = None;
                    stock.peak_price_since_entry = None;
                    stock.last_buy_price = None;
                    stock.last_buy_order_id = None;
                    stock.adverse_move_recorded = false;
                    stock.cooldown_until_market_minute = Some(
                        market_minute
                            .checked_add(POST_EXIT_COOLDOWN_MINUTES)
                            .ok_or(ExperienceError::MarketMinuteOverflow {
                                minute: market_minute,
                                increment: POST_EXIT_COOLDOWN_MINUTES,
                            })?,
                    );
                }
            }
        }
        Ok(())
    }

    pub fn is_in_post_exit_cooldown(&self, code: &StockCode, market_minute: u64) -> bool {
        self.stocks
            .get(code)
            .and_then(|stock| stock.cooldown_until_market_minute)
            .is_some_and(|until| market_minute < until)
    }

    pub fn prune_watchlist(&mut self, held: &BTreeSet<StockCode>) {
        let mut unheld: Vec<_> = self
            .stocks
            .iter()
            .filter(|(code, _)| !held.contains(*code))
            .map(|(code, stock)| {
                (
                    stock
                        .last_trade_market_minute
                        .max(stock.last_observed_market_minute),
                    code.clone(),
                )
            })
            .collect();
        unheld.sort_by(|left, right| right.cmp(left));
        for (_, code) in unheld.into_iter().skip(MAX_UNHELD_WATCHLIST_STOCKS) {
            self.stocks.remove(&code);
        }
    }
}

fn require_positive(field: &'static str, value: Money) -> Result<(), ExperienceError> {
    if value.cents() <= 0 {
        return Err(ExperienceError::NonPositiveMoney {
            field,
            cents: value.cents(),
        });
    }
    Ok(())
}

mod u64_decimal {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &u64, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<u64, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse::<u64>()
            .map_err(serde::de::Error::custom)
    }
}

mod optional_u64_decimal {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(value: &Option<u64>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        value.map(|minute| minute.to_string()).serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Option::<String>::deserialize(deserializer)?
            .map(|value| value.parse::<u64>().map_err(serde::de::Error::custom))
            .transpose()
    }
}
