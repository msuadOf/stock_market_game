//! 具名技术指标计算内核（K5 行 134：SMA20/SMA60、RSI14、ATR14 按完整日 K 计算）。
//!
//! 本层是纯数学内核：只接收已完成日 K 的价格序列，不理解交易日历、宿主 tick
//! 或账户状态；窗口长度一律是完整交易日数。全部为整数分运算，除法一律半偶
//! 舍入（round-half-even）并在各函数文档写明。历史不足返回携带可用长度的
//! [`TechnicalError::InsufficientHistory`]，绝不零填充、绝不虚构趋势。
//!
//! 方法选择（文档化）：RSI 采用简单算术平均（Cutler 式），不采用 Wilder 递推
//! 平滑——本引擎日 K 历史有保留上限，Wilder 递推需要从序列首个样本起连续
//! 平滑，截断窗口下无法确定性重建；简单平均是无状态重算，存档恢复前后逐位
//! 一致，也可手算复核。

use crate::Money;

/// K5 固定短均线窗口（完整交易日数）。
pub const SMA_SHORT_WINDOW: usize = 20;
/// K5 固定长均线窗口（完整交易日数）。
pub const SMA_LONG_WINDOW: usize = 60;
/// RSI14 的涨跌样本数；需要 RSI_WINDOW + 1 个收盘价。
pub const RSI_WINDOW: usize = 14;
/// ATR14 的真实波幅样本数；每个真实波幅需要前一根收盘，故需 ATR_WINDOW + 1 根日 K。
pub const ATR_WINDOW: usize = 14;

/// 技术指标计算失败。历史不足时携带可用样本数与所需样本数。
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TechnicalError {
    #[error(
        "insufficient daily history: {available} samples available, {required} required; history is never zero-padded"
    )]
    InsufficientHistory { available: usize, required: usize },
    #[error("price at index {index} must be positive, got {cents} cents")]
    NonPositivePrice { index: usize, cents: i64 },
    #[error("daily bar at index {index} has high {high_cents} below low {low_cents}")]
    InvertedRange {
        index: usize,
        high_cents: i64,
        low_cents: i64,
    },
    #[error("moving-average window must be positive, got {window}")]
    ZeroWindow { window: usize },
}

/// ATR 内核输入：一根已完成日 K 的高/低/收。成交量与交易日历不属于内核语义，
/// 由观测层（`observation::technical`）先行过滤与校验。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TechnicalDailyBar {
    pub high: Money,
    pub low: Money,
    pub close: Money,
}

/// 简单移动平均。`average` = 最近 `window` 个收盘之和 / `window`，半偶舍入到分。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimpleMovingAverage {
    pub window: usize,
    pub average: Money,
}

/// RSI14。`value` 为 0..=100 整数；全平（14 个有效涨跌样本之和为零，即零涨跌
/// 分母）被明确定义为中性 50，并携带 `all_flat` 与 `valid_samples` 标记。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RelativeStrengthIndex {
    pub window: usize,
    pub valid_samples: usize,
    pub value: u16,
    pub all_flat: bool,
}

/// ATR14 = 最近 14 个真实波幅的简单平均，半偶舍入到分。
/// K5 约束：ATR 只供风险/执行参考，绝不充当方向信号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AverageTrueRange {
    pub window: usize,
    pub average: Money,
}

/// 最近 `window` 个收盘的简单移动平均。
///
/// 舍入：`round_half_even(Σclose_cents / window)`。输入中的全部价格先做正数
/// 校验（与 `observation` 模块对输入序列全量校验的惯例一致），再检查样本量。
pub fn sma(closes: &[Money], window: usize) -> Result<SimpleMovingAverage, TechnicalError> {
    if window == 0 {
        return Err(TechnicalError::ZeroWindow { window });
    }
    validate_positive_closes(closes)?;
    if closes.len() < window {
        return Err(TechnicalError::InsufficientHistory {
            available: closes.len(),
            required: window,
        });
    }
    let sum: i128 = closes[closes.len() - window..]
        .iter()
        .map(|close| i128::from(close.cents()))
        .sum();
    // usize → i128 是无损拓宽转换（usize 至多 64 位）。
    let average = div_round_half_even(sum, window as i128);
    let average = i64::try_from(average).expect("mean of positive i64 prices fits i64");
    Ok(SimpleMovingAverage {
        window,
        average: Money::from_cents(average),
    })
}

/// RSI14（简单平均式，见模块文档的方法选择）。
///
/// `RSI = round_half_even(100 × Σgain / (Σgain + Σloss))`，对最近 14 个日涨跌
/// 计算（需要 15 个收盘）；两端的 /14 平均分母约去。零涨跌分母定义：
/// `Σgain + Σloss == 0` 且已有 14 个有效样本时，值精确定义为中性 50 并置
/// `all_flat`；样本不足是类型化拒绝，不得伪装成 50。
pub fn rsi14(closes: &[Money]) -> Result<RelativeStrengthIndex, TechnicalError> {
    let required = RSI_WINDOW + 1;
    validate_positive_closes(closes)?;
    if closes.len() < required {
        return Err(TechnicalError::InsufficientHistory {
            available: closes.len(),
            required,
        });
    }
    let window = &closes[closes.len() - required..];
    let mut gain_sum: i128 = 0;
    let mut loss_sum: i128 = 0;
    for pair in window.windows(2) {
        let change = i128::from(pair[1].cents()) - i128::from(pair[0].cents());
        if change > 0 {
            gain_sum += change;
        } else {
            loss_sum -= change;
        }
    }
    let denominator = gain_sum + loss_sum;
    let all_flat = denominator == 0;
    // gain_sum ≤ denominator，故值域为 [0,100]，u16 转换安全。
    let value = if all_flat {
        50
    } else {
        u16::try_from(div_round_half_even(100 * gain_sum, denominator))
            .expect("RSI is mathematically bounded to 0..=100")
    };
    Ok(RelativeStrengthIndex {
        window: RSI_WINDOW,
        valid_samples: RSI_WINDOW,
        value,
        all_flat,
    })
}

/// ATR14：最近 14 个真实波幅的简单平均，半偶舍入到分。
///
/// 真实波幅 `TR_i = max(high_i − low_i, |high_i − close_{i−1}|, |low_i − close_{i−1}|)`；
/// 每个 TR 都需要前一根收盘，因此需要 15 根日 K，不虚构首根的前收盘。
pub fn atr14(bars: &[TechnicalDailyBar]) -> Result<AverageTrueRange, TechnicalError> {
    let required = ATR_WINDOW + 1;
    for (index, bar) in bars.iter().enumerate() {
        if bar.high.cents() <= 0 {
            return Err(TechnicalError::NonPositivePrice {
                index,
                cents: bar.high.cents(),
            });
        }
        if bar.low.cents() <= 0 {
            return Err(TechnicalError::NonPositivePrice {
                index,
                cents: bar.low.cents(),
            });
        }
        if bar.close.cents() <= 0 {
            return Err(TechnicalError::NonPositivePrice {
                index,
                cents: bar.close.cents(),
            });
        }
        if bar.high < bar.low {
            return Err(TechnicalError::InvertedRange {
                index,
                high_cents: bar.high.cents(),
                low_cents: bar.low.cents(),
            });
        }
    }
    if bars.len() < required {
        return Err(TechnicalError::InsufficientHistory {
            available: bars.len(),
            required,
        });
    }
    let window = &bars[bars.len() - required..];
    let mut tr_sum: i128 = 0;
    for pair in window.windows(2) {
        let previous_close = i128::from(pair[0].close.cents());
        let bar = pair[1];
        let high = i128::from(bar.high.cents());
        let low = i128::from(bar.low.cents());
        let range = high - low;
        let gap_up = (high - previous_close).abs();
        let gap_down = (low - previous_close).abs();
        tr_sum += range.max(gap_up).max(gap_down);
    }
    // 平均波幅不超过最大单根波幅，正 i64 输入下均值必在 i64 值域内；
    // usize → i128 是无损拓宽转换。
    let average = div_round_half_even(tr_sum, ATR_WINDOW as i128);
    let average = i64::try_from(average).expect("mean of positive true ranges fits i64");
    Ok(AverageTrueRange {
        window: ATR_WINDOW,
        average: Money::from_cents(average),
    })
}

fn validate_positive_closes(closes: &[Money]) -> Result<(), TechnicalError> {
    for (index, close) in closes.iter().enumerate() {
        if close.cents() <= 0 {
            return Err(TechnicalError::NonPositivePrice {
                index,
                cents: close.cents(),
            });
        }
    }
    Ok(())
}

/// 整数半偶舍入除法（denominator > 0）。
///
/// 与 `accounting::amount::div_round_half_even` 同算法的受控副本（该函数是
/// accounting 私有；`industrial::loans` 已有同一先例）。负分子按符号对称处理。
fn div_round_half_even(numerator: i128, denominator: i128) -> i128 {
    debug_assert!(denominator > 0, "all technical denominators are positive");
    let sign = if numerator < 0 { -1_i128 } else { 1_i128 };
    let numerator_abs = numerator.unsigned_abs();
    let denominator_abs = denominator.unsigned_abs();
    let quotient = numerator_abs / denominator_abs;
    let remainder = numerator_abs % denominator_abs;
    let twice = remainder * 2;
    let rounded = if twice > denominator_abs || (twice == denominator_abs && quotient % 2 == 1) {
        quotient + 1
    } else {
        quotient
    };
    sign * i128::try_from(rounded).expect("u128 quotient of i128 inputs fits i128")
}
