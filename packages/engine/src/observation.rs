//! 由权威行情与账户状态派生的只读策略观测。
//!
//! 窗口只使用游戏内交易分钟和已完成交易日，不读取墙钟、宿主刷新节奏或 UI 采样。

use std::collections::BTreeMap;

use thiserror::Error;

use crate::{Money, MoneyError, StockCode};

/// 当前游戏一天的盘中交易分钟桶数。
///
/// 这是包含 M01“尚未实现收盘集合竞价”简化的游戏时间轴，不等同于现行 A 股连续竞价时长。
pub const GAME_INTRADAY_MINUTES_PER_DAY: u16 = 240;

/// 连续竞价内一个权威 tick 的最新价输入。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarketTickPrice {
    /// 当日连续竞价 tick，从 0 开始，必须小于该日连续竞价总 tick 数。
    pub continuous_tick: u64,
    pub price: Money,
}

/// 一个标准交易分钟结束时可见的权威最新成交价；无成交分钟沿用当时最新价。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct MarketMinuteClose {
    /// 从游戏第 0 日第 0 个连续竞价分钟开始的绝对交易分钟。
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub absolute_trading_minute: u64,
    pub close: Money,
}

/// 一个已经完成的游戏交易日收盘价。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompletedDayClose {
    pub trading_day: u32,
    pub close: Money,
}

/// 固定市场时间窗口的收益。历史不足时收益为 `None`，并保留实际可用跨度。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HorizonReturn {
    pub requested_span: u16,
    pub available_span: u16,
    pub return_ratio: Option<f64>,
}

/// 当前价格相对于此前 30 个完整交易分钟范围的位置。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PriorRangeObservation {
    pub high: Money,
    pub low: Money,
    pub distance_from_high: f64,
    pub distance_from_low: f64,
    pub rebound_from_low: f64,
    pub broke_above: bool,
    pub broke_below: bool,
}

/// 单股公共价格路径观测。
#[derive(Clone, Debug, PartialEq)]
pub struct PricePathObservation {
    pub one_minute: HorizonReturn,
    pub thirty_minute: HorizonReturn,
    pub intraday: HorizonReturn,
    pub five_day: HorizonReturn,
    pub twenty_day: HorizonReturn,
    pub one_hundred_twenty_day: HorizonReturn,
    pub two_hundred_fifty_day: HorizonReturn,
    pub prior_thirty_minute_range: Option<PriorRangeObservation>,
}

/// 游戏内等权市场背景。覆盖不足不会被补成零收益。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EqualWeightMarketObservation {
    pub total_stock_count: usize,
    pub observed_stock_count: usize,
    pub equal_weight_return: Option<f64>,
    pub advance_fraction: Option<f64>,
    pub decline_fraction: Option<f64>,
    pub unchanged_fraction: Option<f64>,
}

/// 账户中一个真实持仓的风险计算输入。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RiskPositionInput {
    pub qty: u32,
    pub cost_price: Option<Money>,
    pub last_price: Money,
    /// 当前持仓生命周期内已经观察到的最高价；尚未维护该状态时明确为 `None`。
    pub peak_price_since_entry: Option<Money>,
}

/// 单个持仓相对于该账户自身成本与净值的观测。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PositionRiskObservation {
    pub market_value: Money,
    pub unrealized_return: Option<f64>,
    pub equity_weight: Option<f64>,
    pub drawdown_from_position_peak: Option<f64>,
}

/// 一个独立账户的风险观测。
#[derive(Clone, Debug, PartialEq)]
pub struct AccountRiskObservation {
    pub equity: Money,
    pub return_from_reference: Option<f64>,
    pub drawdown_from_peak: Option<f64>,
    pub positions: BTreeMap<StockCode, PositionRiskObservation>,
}

#[derive(Debug, Error)]
pub enum ObservationError {
    #[error("continuous_ticks_per_day must be > 0")]
    ZeroTicksPerDay,
    #[error("market tick {tick} must be less than continuous_ticks_per_day {total}")]
    TickOutsideDay { tick: u64, total: u64 },
    #[error("completed market ticks {completed} must not exceed ticks per day {total}")]
    CompletedTicksOutsideDay { completed: u64, total: u64 },
    #[error("market ticks must be strictly increasing; found {current} after {previous}")]
    NonIncreasingTick { previous: u64, current: u64 },
    #[error("market tick history has a gap: expected {expected}, found {current}")]
    TickGap { expected: u64, current: u64 },
    #[error("minute closes must be strictly increasing; found {current} after {previous}")]
    NonIncreasingMinute { previous: u64, current: u64 },
    #[error(
        "completed trading days must be strictly increasing; found {current} after {previous}"
    )]
    NonIncreasingDay { previous: u32, current: u32 },
    #[error("completed trading-day history has a gap: expected {expected}, found {current}")]
    TradingDayGap { expected: u32, current: u32 },
    #[error("{field} must be positive, got {cents} cents")]
    NonPositivePrice { field: &'static str, cents: i64 },
    #[error("total cash must be non-negative, got {cents} cents")]
    NegativeCash { cents: i64 },
    #[error("position {code} must have positive quantity")]
    ZeroPositionQuantity { code: String },
    #[error("position {code} peak {peak_cents} is below current price {last_cents}")]
    PeakBelowCurrent {
        code: String,
        peak_cents: i64,
        last_cents: i64,
    },
    #[error("account reference equity must be positive, got {cents} cents")]
    NonPositiveReferenceEquity { cents: i64 },
    #[error("account peak equity must be positive, got {cents} cents")]
    NonPositivePeakEquity { cents: i64 },
    #[error("account peak equity {peak_cents} is below current equity {equity_cents}")]
    PeakEquityBelowCurrent { peak_cents: i64, equity_cents: i64 },
    #[error("market return for {code} must be finite and greater than -1, got {value}")]
    InvalidMarketReturn { code: String, value: f64 },
    #[error("market return input must contain at least one stock")]
    EmptyMarket,
    #[error("equal-weight market return sum became non-finite after stock {code}")]
    MarketReturnSumNonFinite { code: String },
    #[error("absolute trading minute overflow for day {day}")]
    TradingMinuteOverflow { day: u32 },
    #[error(transparent)]
    Money(#[from] MoneyError),
}

/// 把任意密度的连续竞价 tick 归并为标准 240 个已完成交易分钟。
///
/// 一个 tick 跨过多个分钟边界时，那些分钟都沿用该 tick 完成时的权威最新价；这表示
/// 期间没有新的价格观测，不产生或伪造成交。一个分钟内有多个 tick 时取跨越该分钟末端的
/// 最后一个 tick。宿主速度不参与计算。
pub fn build_market_minute_closes(
    trading_day: u32,
    continuous_ticks_per_day: u64,
    samples: &[MarketTickPrice],
) -> Result<Vec<MarketMinuteClose>, ObservationError> {
    if continuous_ticks_per_day == 0 {
        return Err(ObservationError::ZeroTicksPerDay);
    }
    let day_start = u64::from(trading_day)
        .checked_mul(u64::from(GAME_INTRADAY_MINUTES_PER_DAY))
        .ok_or(ObservationError::TradingMinuteOverflow { day: trading_day })?;
    let mut output: Vec<MarketMinuteClose> = Vec::new();
    let mut completed_minutes = 0_u64;
    let mut previous_tick = None;
    for sample in samples {
        if sample.price.cents() <= 0 {
            return Err(ObservationError::NonPositivePrice {
                field: "market tick price",
                cents: sample.price.cents(),
            });
        }
        if sample.continuous_tick >= continuous_ticks_per_day {
            return Err(ObservationError::TickOutsideDay {
                tick: sample.continuous_tick,
                total: continuous_ticks_per_day,
            });
        }
        if previous_tick.is_some_and(|previous| sample.continuous_tick <= previous) {
            return Err(ObservationError::NonIncreasingTick {
                previous: previous_tick.expect("checked Some above"),
                current: sample.continuous_tick,
            });
        }
        let expected_tick = previous_tick.map_or(0, |previous| previous + 1);
        if sample.continuous_tick != expected_tick {
            return Err(ObservationError::TickGap {
                expected: expected_tick,
                current: sample.continuous_tick,
            });
        }
        previous_tick = Some(sample.continuous_tick);
        let completed_after_tick = u64::from(completed_market_minute_count(
            sample.continuous_tick + 1,
            continuous_ticks_per_day,
        )?);
        while completed_minutes < completed_after_tick {
            let absolute_trading_minute = day_start
                .checked_add(completed_minutes)
                .ok_or(ObservationError::TradingMinuteOverflow { day: trading_day })?;
            output.push(MarketMinuteClose {
                absolute_trading_minute,
                close: sample.price,
            });
            completed_minutes += 1;
        }
    }
    Ok(output)
}

/// 把已经完成的连续撮合 tick 数映射为已经完成的游戏盘中分钟数。
///
/// 计算为常数时间，不随 tick 数量循环；因此压缩日和高精度日都不会放大存档校验成本。
pub fn completed_market_minute_count(
    completed_ticks: u64,
    continuous_ticks_per_day: u64,
) -> Result<u16, ObservationError> {
    if continuous_ticks_per_day == 0 {
        return Err(ObservationError::ZeroTicksPerDay);
    }
    if completed_ticks > continuous_ticks_per_day {
        return Err(ObservationError::CompletedTicksOutsideDay {
            completed: completed_ticks,
            total: continuous_ticks_per_day,
        });
    }
    let completed = u128::from(completed_ticks) * u128::from(GAME_INTRADAY_MINUTES_PER_DAY)
        / u128::from(continuous_ticks_per_day);
    Ok(u16::try_from(completed).expect("ratio is bounded by the 240-minute game day"))
}

/// 从标准分钟收盘和已完成日收盘构造公共价格路径观测。
pub fn build_price_path_observation(
    minute_closes: &[MarketMinuteClose],
    completed_days: &[CompletedDayClose],
    current_day_open: Option<Money>,
) -> Result<PricePathObservation, ObservationError> {
    validate_minute_closes(minute_closes)?;
    validate_completed_days(completed_days)?;
    if let Some(open) = current_day_open {
        validate_positive_price("current day open", open)?;
    }

    let Some(current) = minute_closes.last().copied() else {
        return Ok(PricePathObservation {
            one_minute: unavailable_return(1, 0),
            thirty_minute: unavailable_return(30, 0),
            intraday: unavailable_return(0, 0),
            five_day: unavailable_return(5, available_days(completed_days)),
            twenty_day: unavailable_return(20, available_days(completed_days)),
            one_hundred_twenty_day: unavailable_return(120, available_days(completed_days)),
            two_hundred_fifty_day: unavailable_return(250, available_days(completed_days)),
            prior_thirty_minute_range: None,
        });
    };
    let day_start = current.absolute_trading_minute / u64::from(GAME_INTRADAY_MINUTES_PER_DAY)
        * u64::from(GAME_INTRADAY_MINUTES_PER_DAY);
    let first_current_day = minute_closes
        .iter()
        .find(|sample| sample.absolute_trading_minute >= day_start)
        .expect("current sample establishes at least one sample in its day");
    let available_minutes_u64 = current
        .absolute_trading_minute
        .saturating_sub(first_current_day.absolute_trading_minute);
    let available_minutes = u16::try_from(available_minutes_u64).unwrap_or(u16::MAX);

    let one_minute = minute_return(minute_closes, current, day_start, 1, available_minutes);
    let thirty_minute = minute_return(minute_closes, current, day_start, 30, available_minutes);
    let current_minute_in_day = u16::try_from(current.absolute_trading_minute - day_start)
        .expect("minute in a 240-minute day fits u16");
    let intraday_span = current_minute_in_day + 1;
    let intraday = HorizonReturn {
        requested_span: intraday_span,
        available_span: intraday_span,
        return_ratio: current_day_open.map(|open| return_between(current.close, open)),
    };

    Ok(PricePathObservation {
        one_minute,
        thirty_minute,
        intraday,
        five_day: daily_return(current.close, completed_days, 5),
        twenty_day: daily_return(current.close, completed_days, 20),
        one_hundred_twenty_day: daily_return(current.close, completed_days, 120),
        two_hundred_fifty_day: daily_return(current.close, completed_days, 250),
        prior_thirty_minute_range: prior_range(minute_closes, current, day_start, 30),
    })
}

/// 用具备完整窗口的股票构造游戏内等权市场收益与涨跌家数。
pub fn build_equal_weight_market_observation(
    stock_returns: &BTreeMap<StockCode, Option<f64>>,
) -> Result<EqualWeightMarketObservation, ObservationError> {
    if stock_returns.is_empty() {
        return Err(ObservationError::EmptyMarket);
    }
    let mut sum = 0.0;
    let mut observed = 0_usize;
    let mut advances = 0_usize;
    let mut declines = 0_usize;
    let mut unchanged = 0_usize;
    for (code, value) in stock_returns {
        let Some(value) = value else { continue };
        if !value.is_finite() || *value <= -1.0 {
            return Err(ObservationError::InvalidMarketReturn {
                code: code.0.clone(),
                value: *value,
            });
        }
        observed += 1;
        sum += value;
        if !sum.is_finite() {
            return Err(ObservationError::MarketReturnSumNonFinite {
                code: code.0.clone(),
            });
        }
        if *value > 0.0 {
            advances += 1;
        } else if *value < 0.0 {
            declines += 1;
        } else {
            unchanged += 1;
        }
    }
    let denominator = observed as f64;
    Ok(EqualWeightMarketObservation {
        total_stock_count: stock_returns.len(),
        observed_stock_count: observed,
        equal_weight_return: (observed > 0).then_some(sum / denominator),
        advance_fraction: (observed > 0).then_some(advances as f64 / denominator),
        decline_fraction: (observed > 0).then_some(declines as f64 / denominator),
        unchanged_fraction: (observed > 0).then_some(unchanged as f64 / denominator),
    })
}

/// 按每个独立账户自己的成本、持仓与净值构造风险观测。
pub fn build_account_risk_observation(
    total_cash: Money,
    positions: &BTreeMap<StockCode, RiskPositionInput>,
    reference_equity: Option<Money>,
    peak_equity: Option<Money>,
) -> Result<AccountRiskObservation, ObservationError> {
    if total_cash.cents() < 0 {
        return Err(ObservationError::NegativeCash {
            cents: total_cash.cents(),
        });
    }
    validate_reference_equity(reference_equity, "reference")?;
    validate_reference_equity(peak_equity, "peak")?;

    let mut equity = total_cash;
    let mut market_values = BTreeMap::new();
    for (code, position) in positions {
        validate_position(code, position)?;
        let market_value = position.last_price.mul_shares(position.qty)?;
        equity = equity.add(market_value)?;
        market_values.insert(code.clone(), market_value);
    }
    if let Some(peak) = peak_equity {
        if peak < equity {
            return Err(ObservationError::PeakEquityBelowCurrent {
                peak_cents: peak.cents(),
                equity_cents: equity.cents(),
            });
        }
    }

    let mut observations = BTreeMap::new();
    for (code, position) in positions {
        let market_value = market_values
            .get(code)
            .copied()
            .expect("market value was built from the same position map");
        let unrealized_return = position
            .cost_price
            .map(|cost| return_between(position.last_price, cost));
        let drawdown_from_position_peak = position
            .peak_price_since_entry
            .map(|peak| return_between(position.last_price, peak));
        observations.insert(
            code.clone(),
            PositionRiskObservation {
                market_value,
                unrealized_return,
                equity_weight: (equity.cents() > 0)
                    .then_some(market_value.cents() as f64 / equity.cents() as f64),
                drawdown_from_position_peak,
            },
        );
    }

    Ok(AccountRiskObservation {
        equity,
        return_from_reference: reference_equity.map(|reference| return_between(equity, reference)),
        drawdown_from_peak: peak_equity.map(|peak| return_between(equity, peak)),
        positions: observations,
    })
}

fn validate_minute_closes(samples: &[MarketMinuteClose]) -> Result<(), ObservationError> {
    let mut previous = None;
    for sample in samples {
        if sample.close.cents() <= 0 {
            return Err(ObservationError::NonPositivePrice {
                field: "market minute close",
                cents: sample.close.cents(),
            });
        }
        if previous.is_some_and(|value| sample.absolute_trading_minute <= value) {
            return Err(ObservationError::NonIncreasingMinute {
                previous: previous.expect("checked Some above"),
                current: sample.absolute_trading_minute,
            });
        }
        previous = Some(sample.absolute_trading_minute);
    }
    Ok(())
}

fn validate_completed_days(samples: &[CompletedDayClose]) -> Result<(), ObservationError> {
    let mut previous = None;
    for sample in samples {
        if sample.close.cents() <= 0 {
            return Err(ObservationError::NonPositivePrice {
                field: "completed day close",
                cents: sample.close.cents(),
            });
        }
        if previous.is_some_and(|value| sample.trading_day <= value) {
            return Err(ObservationError::NonIncreasingDay {
                previous: previous.expect("checked Some above"),
                current: sample.trading_day,
            });
        }
        if let Some(previous) = previous {
            let expected = previous
                .checked_add(1)
                .ok_or(ObservationError::TradingDayGap {
                    expected: previous,
                    current: sample.trading_day,
                })?;
            if sample.trading_day != expected {
                return Err(ObservationError::TradingDayGap {
                    expected,
                    current: sample.trading_day,
                });
            }
        }
        previous = Some(sample.trading_day);
    }
    Ok(())
}

fn validate_reference_equity(
    value: Option<Money>,
    kind: &'static str,
) -> Result<(), ObservationError> {
    let Some(value) = value else { return Ok(()) };
    if value.cents() > 0 {
        return Ok(());
    }
    match kind {
        "reference" => Err(ObservationError::NonPositiveReferenceEquity {
            cents: value.cents(),
        }),
        "peak" => Err(ObservationError::NonPositivePeakEquity {
            cents: value.cents(),
        }),
        _ => unreachable!("reference kind is internal and exhaustive"),
    }
}

fn validate_position(
    code: &StockCode,
    position: &RiskPositionInput,
) -> Result<(), ObservationError> {
    if position.qty == 0 {
        return Err(ObservationError::ZeroPositionQuantity {
            code: code.0.clone(),
        });
    }
    validate_positive_price("position last price", position.last_price)?;
    if let Some(cost) = position.cost_price {
        validate_positive_price("position cost price", cost)?;
    }
    if let Some(peak) = position.peak_price_since_entry {
        validate_positive_price("position peak price", peak)?;
        if peak < position.last_price {
            return Err(ObservationError::PeakBelowCurrent {
                code: code.0.clone(),
                peak_cents: peak.cents(),
                last_cents: position.last_price.cents(),
            });
        }
    }
    Ok(())
}

fn validate_positive_price(field: &'static str, value: Money) -> Result<(), ObservationError> {
    if value.cents() <= 0 {
        return Err(ObservationError::NonPositivePrice {
            field,
            cents: value.cents(),
        });
    }
    Ok(())
}

fn unavailable_return(requested_span: u16, available_span: u16) -> HorizonReturn {
    HorizonReturn {
        requested_span,
        available_span,
        return_ratio: None,
    }
}

fn minute_return(
    samples: &[MarketMinuteClose],
    current: MarketMinuteClose,
    day_start: u64,
    horizon: u16,
    available_span: u16,
) -> HorizonReturn {
    let target = current
        .absolute_trading_minute
        .checked_sub(u64::from(horizon))
        .filter(|target| *target >= day_start);
    let reference = target.and_then(|target| {
        samples
            .binary_search_by_key(&target, |sample| sample.absolute_trading_minute)
            .ok()
            .map(|index| samples[index].close)
    });
    HorizonReturn {
        requested_span: horizon,
        available_span: available_span.min(horizon),
        return_ratio: reference.map(|reference| return_between(current.close, reference)),
    }
}

fn daily_return(
    current: Money,
    completed_days: &[CompletedDayClose],
    horizon: u16,
) -> HorizonReturn {
    let available_span = available_days(completed_days).min(horizon);
    let reference = completed_days
        .len()
        .checked_sub(usize::from(horizon))
        .map(|index| completed_days[index].close);
    HorizonReturn {
        requested_span: horizon,
        available_span,
        return_ratio: reference.map(|reference| return_between(current, reference)),
    }
}

fn available_days(completed_days: &[CompletedDayClose]) -> u16 {
    u16::try_from(completed_days.len()).unwrap_or(u16::MAX)
}

fn prior_range(
    samples: &[MarketMinuteClose],
    current: MarketMinuteClose,
    day_start: u64,
    horizon: u16,
) -> Option<PriorRangeObservation> {
    let start = current
        .absolute_trading_minute
        .checked_sub(u64::from(horizon))
        .filter(|start| *start >= day_start)?;
    let mut high: Option<Money> = None;
    let mut low: Option<Money> = None;
    for minute in start..current.absolute_trading_minute {
        let index = samples
            .binary_search_by_key(&minute, |sample| sample.absolute_trading_minute)
            .ok()?;
        let close = samples[index].close;
        high = Some(high.map_or(close, |value| value.max(close)));
        low = Some(low.map_or(close, |value| value.min(close)));
    }
    let high = high?;
    let low = low?;
    Some(PriorRangeObservation {
        high,
        low,
        distance_from_high: return_between(current.close, high),
        distance_from_low: return_between(current.close, low),
        rebound_from_low: return_between(current.close, low),
        broke_above: current.close > high,
        broke_below: current.close < low,
    })
}

fn return_between(current: Money, reference: Money) -> f64 {
    (current.cents() - reference.cents()) as f64 / reference.cents() as f64
}
