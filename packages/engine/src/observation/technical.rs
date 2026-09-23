//! 具名技术指标观测接缝（W3-Task 19）。
//!
//! 把权威已完成日 K 转换为 K5 具名技术指标观测。窗口长度一律按完整市场时间
//! （交易日）表达，输入是日 K 序列，结构上与宿主 tick 密度无关。原 30 分钟/
//! 5 日/区间/量能/失衡观测保持不变；本层只在其旁新增 SMA/RSI/ATR 观测，
//! 短期与长期信号独立携带、互不耦合，因此短期下跌与长期上行可同时表达。
//!
//! 无成交日（`volume == 0`）没有真实已发生行情，不构成指标样本：先对原始
//! 连续日序做结构校验（缺日/乱序/未来数据是类型化错误），再排除无成交日；
//! 排除后的样本数如实计入 [`TechnicalObservation::valid_sample_count`]，不足时
//! 逐指标携带 [`TechnicalError::InsufficientHistory`]，绝不零填充。

use super::{Money, ObservationError};
use crate::strategy::{
    atr14, rsi14, sma, AverageTrueRange, RelativeStrengthIndex, SimpleMovingAverage,
    TechnicalDailyBar, TechnicalError, SMA_LONG_WINDOW, SMA_SHORT_WINDOW,
};

/// 观测输入：一根已完成日 K（含成交量）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TechnicalDailyInput {
    /// 从 0 开始的绝对交易日序号。
    pub trading_day: u32,
    pub high: Money,
    pub low: Money,
    pub close: Money,
    /// 当日累计成交量；0 表示整日无成交（真实撮合与预置历史均 > 0）。
    pub volume: u64,
}

/// 单股具名技术指标观测。各指标独立计算、独立携带可用性（价格非法或历史
/// 不足时为 `Err`），互不掩盖。
#[derive(Clone, Debug, PartialEq)]
pub struct TechnicalObservation {
    /// 排除无成交日后的有效日 K 样本数。
    pub valid_sample_count: usize,
    pub sma20: Result<SimpleMovingAverage, TechnicalError>,
    pub sma60: Result<SimpleMovingAverage, TechnicalError>,
    pub rsi14: Result<RelativeStrengthIndex, TechnicalError>,
    /// K5 约束：ATR 只供风险/执行，绝不充当方向信号。
    pub atr14: Result<AverageTrueRange, TechnicalError>,
}

/// 由已完成日 K 构造单股技术指标观测。
///
/// `as_of_trading_day` 是观察所在的交易日：所有输入日 K 必须严格早于该日
/// （当日未完成 K 与未来 K 都是类型化拒绝——观察时点 T 不能包含 T 之后的数据）。
/// 日序必须严格递增且连续（缺日是缺数据，不是可用样本为零）。
pub fn build_technical_observation(
    bars: &[TechnicalDailyInput],
    as_of_trading_day: u32,
) -> Result<TechnicalObservation, ObservationError> {
    let mut previous_day: Option<u32> = None;
    for bar in bars {
        if bar.trading_day >= as_of_trading_day {
            return Err(ObservationError::DailyBarNotBeforeObservation {
                day: bar.trading_day,
                as_of: as_of_trading_day,
            });
        }
        if previous_day.is_some_and(|previous| bar.trading_day <= previous) {
            return Err(ObservationError::NonIncreasingDay {
                previous: previous_day.expect("checked Some above"),
                current: bar.trading_day,
            });
        }
        if let Some(previous) = previous_day {
            let expected = previous
                .checked_add(1)
                .ok_or(ObservationError::TradingDayGap {
                    expected: previous,
                    current: bar.trading_day,
                })?;
            if bar.trading_day != expected {
                return Err(ObservationError::TradingDayGap {
                    expected,
                    current: bar.trading_day,
                });
            }
        }
        previous_day = Some(bar.trading_day);
    }

    let traded: Vec<&TechnicalDailyInput> = bars.iter().filter(|bar| bar.volume > 0).collect();
    let valid_sample_count = traded.len();
    let closes: Vec<Money> = traded.iter().map(|bar| bar.close).collect();
    let kernel_bars: Vec<TechnicalDailyBar> = traded
        .iter()
        .map(|bar| TechnicalDailyBar {
            high: bar.high,
            low: bar.low,
            close: bar.close,
        })
        .collect();

    Ok(TechnicalObservation {
        valid_sample_count,
        sma20: sma(&closes, SMA_SHORT_WINDOW),
        sma60: sma(&closes, SMA_LONG_WINDOW),
        rsi14: rsi14(&closes),
        atr14: atr14(&kernel_bars),
    })
}
