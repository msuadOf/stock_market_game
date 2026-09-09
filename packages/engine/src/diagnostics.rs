//! 可重放的量价统计基线。
//!
//! 该模块只观察 [`GameSession`] 的权威事件，不参与撮合或 NPC 决策。诊断运行使用
//! 与正式游戏相同的 setup 和引擎路径，以便在改策略前后对比多 seed 分布。

use std::collections::BTreeMap;

use serde::Serializer;

use crate::{DailyCandle, Event, GameSession, Money, SessionError, SessionSetup, StockCode};

#[derive(Debug, thiserror::Error)]
pub enum BaselineError {
    #[error("量价基线至少需要一个 seed")]
    EmptySeeds,
    #[error("量价基线的交易日数必须大于 0")]
    ZeroTradingDays,
    #[error("量价基线总 tick 数溢出：ticks_per_day={ticks_per_day}, trading_days={trading_days}")]
    TickCountOverflow {
        ticks_per_day: u64,
        trading_days: u32,
    },
    #[error("seed {seed} 创建游戏会话失败：{source}")]
    Session {
        seed: u64,
        #[source]
        source: SessionError,
    },
    #[error("seed {seed} 的股票 {code:?} 仅归档 {actual} 个交易日，期望 {expected} 个")]
    MissingDailyCandles {
        seed: u64,
        code: StockCode,
        expected: u32,
        actual: usize,
    },
    #[error("seed {seed} 的股票 {code:?} 成交量统计溢出")]
    VolumeOverflow { seed: u64, code: StockCode },
    #[error("seed {seed} 的诊断计数器 {counter} 溢出")]
    CounterOverflow { seed: u64, counter: &'static str },
}

/// 一组 setup 在多个随机种子下的可比量价报告。
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct PriceVolumeBaselineReport {
    pub trading_days: u32,
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub ticks_per_day: u64,
    pub runs: Vec<PriceVolumeRunReport>,
}

/// 单个 seed 的完整报告。所有 u64 序列化为十进制字符串，避免 JSON/JavaScript 精度损失。
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct PriceVolumeRunReport {
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub seed: u64,
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub final_tick: u64,
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub trade_events: u64,
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub rejection_events: u64,
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub engine_error_events: u64,
    pub stocks: BTreeMap<StockCode, StockPriceVolumeReport>,
}

/// 单只股票在一个 seed 下的日级统计。浮点值仅用于诊断，不写回权威游戏状态。
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct StockPriceVolumeReport {
    pub completed_days: u32,
    pub traded_days: u32,
    pub zero_volume_days: u32,
    pub max_zero_volume_streak: u32,
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub trade_event_volume: u64,
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub total_daily_volume: u64,
    pub mean_daily_volume: f64,
    pub first_open: Money,
    pub final_close: Money,
    pub min_close: Money,
    pub max_close: Money,
    pub terminal_return_bps: f64,
    pub mean_daily_return_bps: f64,
    pub daily_return_stddev_bps: f64,
    pub absolute_return_lag1_correlation: Option<f64>,
    pub daily_returns_bps: Vec<f64>,
}

/// 用相同 setup 依次运行多个 seed，并统计权威成交事件和每日收盘 K 线。
///
/// 此函数不读取墙钟、Publisher 或 UI 状态，所以相同输入必须得到完全相同的报告。
pub fn run_price_volume_baseline(
    setup: &SessionSetup,
    seeds: &[u64],
    trading_days: u32,
) -> Result<PriceVolumeBaselineReport, BaselineError> {
    if seeds.is_empty() {
        return Err(BaselineError::EmptySeeds);
    }
    if trading_days == 0 {
        return Err(BaselineError::ZeroTradingDays);
    }
    let total_ticks = setup
        .ticks_per_day
        .checked_mul(u64::from(trading_days))
        .ok_or(BaselineError::TickCountOverflow {
            ticks_per_day: setup.ticks_per_day,
            trading_days,
        })?;

    let mut runs = Vec::with_capacity(seeds.len());
    for &seed in seeds {
        runs.push(run_one_seed(setup, seed, trading_days, total_ticks)?);
    }
    Ok(PriceVolumeBaselineReport {
        trading_days,
        ticks_per_day: setup.ticks_per_day,
        runs,
    })
}

fn run_one_seed(
    setup: &SessionSetup,
    seed: u64,
    trading_days: u32,
    total_ticks: u64,
) -> Result<PriceVolumeRunReport, BaselineError> {
    let mut session = GameSession::new(setup.clone(), seed)
        .map_err(|source| BaselineError::Session { seed, source })?;
    let mut daily_candles: BTreeMap<StockCode, Vec<DailyCandle>> = setup
        .stocks
        .iter()
        .map(|stock| {
            (
                stock.code.clone(),
                Vec::with_capacity(trading_days as usize),
            )
        })
        .collect();
    let mut trade_event_volume: BTreeMap<StockCode, u64> = setup
        .stocks
        .iter()
        .map(|stock| (stock.code.clone(), 0))
        .collect();
    let mut trade_events = 0_u64;
    let mut rejection_events = 0_u64;
    let mut engine_error_events = 0_u64;

    for _ in 0..total_ticks {
        for event in session.step() {
            match event {
                Event::Trade { code, qty, .. } => {
                    trade_events = checked_increment(trade_events, seed, "trade_events")?;
                    let volume = trade_event_volume.get_mut(&code).ok_or_else(|| {
                        BaselineError::MissingDailyCandles {
                            seed,
                            code: code.clone(),
                            expected: trading_days,
                            actual: 0,
                        }
                    })?;
                    *volume = volume.checked_add(u64::from(qty)).ok_or_else(|| {
                        BaselineError::VolumeOverflow {
                            seed,
                            code: code.clone(),
                        }
                    })?;
                }
                Event::DayBoundary {
                    closed_daily_candles,
                    ..
                } => {
                    for (code, candle) in closed_daily_candles {
                        let candles = daily_candles.get_mut(&code).ok_or_else(|| {
                            BaselineError::MissingDailyCandles {
                                seed,
                                code: code.clone(),
                                expected: trading_days,
                                actual: 0,
                            }
                        })?;
                        candles.push(candle);
                    }
                }
                Event::IntentRejected { .. } => {
                    rejection_events =
                        checked_increment(rejection_events, seed, "rejection_events")?;
                }
                Event::SettlementError { .. } | Event::VError { .. } => {
                    engine_error_events =
                        checked_increment(engine_error_events, seed, "engine_error_events")?;
                }
                Event::AuctionTick { .. }
                | Event::AuctionCompleted { .. }
                | Event::PriceTick { .. }
                | Event::OrderCanceled { .. }
                | Event::OrderAccepted { .. } => {}
            }
        }
    }

    let mut stocks = BTreeMap::new();
    for stock in &setup.stocks {
        let candles = daily_candles
            .remove(&stock.code)
            .expect("setup stocks initialized the diagnostics candle map");
        if candles.len() != trading_days as usize {
            return Err(BaselineError::MissingDailyCandles {
                seed,
                code: stock.code.clone(),
                expected: trading_days,
                actual: candles.len(),
            });
        }
        let event_volume = trade_event_volume
            .remove(&stock.code)
            .expect("setup stocks initialized the diagnostics volume map");
        stocks.insert(
            stock.code.clone(),
            summarize_stock(
                stock.initial_price,
                candles,
                event_volume,
                seed,
                &stock.code,
            )?,
        );
    }

    Ok(PriceVolumeRunReport {
        seed,
        final_tick: total_ticks,
        trade_events,
        rejection_events,
        engine_error_events,
        stocks,
    })
}

fn summarize_stock(
    initial_price: Money,
    candles: Vec<DailyCandle>,
    trade_event_volume: u64,
    seed: u64,
    code: &StockCode,
) -> Result<StockPriceVolumeReport, BaselineError> {
    let completed_days = candles.len() as u32;
    let traded_days = candles.iter().filter(|candle| candle.volume > 0).count() as u32;
    let zero_volume_days = completed_days - traded_days;
    let mut max_zero_volume_streak = 0_u32;
    let mut current_zero_volume_streak = 0_u32;
    let mut total_daily_volume = 0_u64;
    for candle in &candles {
        total_daily_volume = total_daily_volume
            .checked_add(candle.volume)
            .ok_or_else(|| BaselineError::VolumeOverflow {
                seed,
                code: code.clone(),
            })?;
        if candle.volume == 0 {
            current_zero_volume_streak += 1;
            max_zero_volume_streak = max_zero_volume_streak.max(current_zero_volume_streak);
        } else {
            current_zero_volume_streak = 0;
        }
    }

    let mut previous_close = initial_price;
    let mut daily_returns_bps = Vec::with_capacity(candles.len());
    for candle in &candles {
        daily_returns_bps.push(return_bps(previous_close, candle.close));
        previous_close = candle.close;
    }
    let mean_daily_return_bps = mean(&daily_returns_bps);
    let daily_return_stddev_bps = population_stddev(&daily_returns_bps, mean_daily_return_bps);
    let absolute_returns: Vec<f64> = daily_returns_bps.iter().map(|value| value.abs()).collect();
    let first = candles
        .first()
        .expect("zero trading days was rejected before simulation");
    let final_candle = candles
        .last()
        .expect("zero trading days was rejected before simulation");
    let min_close = candles
        .iter()
        .map(|candle| candle.close)
        .min()
        .expect("non-empty candles");
    let max_close = candles
        .iter()
        .map(|candle| candle.close)
        .max()
        .expect("non-empty candles");

    Ok(StockPriceVolumeReport {
        completed_days,
        traded_days,
        zero_volume_days,
        max_zero_volume_streak,
        trade_event_volume,
        total_daily_volume,
        mean_daily_volume: total_daily_volume as f64 / f64::from(completed_days),
        first_open: first.open,
        final_close: final_candle.close,
        min_close,
        max_close,
        terminal_return_bps: return_bps(initial_price, final_candle.close),
        mean_daily_return_bps,
        daily_return_stddev_bps,
        absolute_return_lag1_correlation: lag_one_correlation(&absolute_returns),
        daily_returns_bps,
    })
}

fn return_bps(from: Money, to: Money) -> f64 {
    (to.cents() as f64 / from.cents() as f64 - 1.0) * 10_000.0
}

fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len() as f64
}

fn population_stddev(values: &[f64], values_mean: f64) -> f64 {
    let variance = values
        .iter()
        .map(|value| (value - values_mean).powi(2))
        .sum::<f64>()
        / values.len() as f64;
    variance.sqrt()
}

fn lag_one_correlation(values: &[f64]) -> Option<f64> {
    if values.len() < 3 {
        return None;
    }
    let left = &values[..values.len() - 1];
    let right = &values[1..];
    let left_mean = mean(left);
    let right_mean = mean(right);
    let covariance = left
        .iter()
        .zip(right)
        .map(|(x, y)| (x - left_mean) * (y - right_mean))
        .sum::<f64>();
    let left_scale = left
        .iter()
        .map(|value| (value - left_mean).powi(2))
        .sum::<f64>();
    let right_scale = right
        .iter()
        .map(|value| (value - right_mean).powi(2))
        .sum::<f64>();
    let denominator = (left_scale * right_scale).sqrt();
    (denominator > 0.0).then_some(covariance / denominator)
}

fn serialize_u64_decimal<S>(value: &u64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&value.to_string())
}

fn checked_increment(value: u64, seed: u64, counter: &'static str) -> Result<u64, BaselineError> {
    value
        .checked_add(1)
        .ok_or(BaselineError::CounterOverflow { seed, counter })
}

#[cfg(test)]
mod tests {
    use super::{lag_one_correlation, summarize_stock};
    use crate::{DailyCandle, Money, StockCode};

    fn candle(open: i64, close: i64, volume: u64) -> DailyCandle {
        DailyCandle {
            time: 0,
            open: Money::from_cents(open),
            high: Money::from_cents(open.max(close)),
            low: Money::from_cents(open.min(close)),
            close: Money::from_cents(close),
            volume,
        }
    }

    #[test]
    fn known_candles_produce_expected_returns_and_population_stddev() {
        let code = StockCode("600101".to_string());
        let report = summarize_stock(
            Money::from_cents(10_000),
            vec![candle(10_000, 11_000, 100), candle(11_000, 9_900, 200)],
            300,
            42,
            &code,
        )
        .unwrap();

        assert!((report.daily_returns_bps[0] - 1_000.0).abs() < 1e-9);
        assert!((report.daily_returns_bps[1] - -1_000.0).abs() < 1e-9);
        assert!(report.mean_daily_return_bps.abs() < 1e-9);
        assert!((report.daily_return_stddev_bps - 1_000.0).abs() < 1e-9);
        assert!((report.terminal_return_bps - -100.0).abs() < 1e-9);
        assert_eq!(report.total_daily_volume, 300);
        assert_eq!(report.absolute_return_lag1_correlation, None);
    }

    #[test]
    fn lag_one_correlation_matches_a_known_perfect_series() {
        let correlation = lag_one_correlation(&[1.0, 2.0, 3.0, 4.0]).unwrap();
        assert!((correlation - 1.0).abs() < 1e-12);
        assert_eq!(lag_one_correlation(&[1.0, 1.0, 1.0]), None);
    }
}
