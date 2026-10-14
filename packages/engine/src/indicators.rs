use crate::compute::parallel::*;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IndicatorInput {
    pub prices: Vec<f64>,
    #[serde(default)]
    pub candles: Vec<OhlcBar>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Macd {
    pub dif: Vec<f64>,
    pub dea: Vec<f64>,
    pub histogram: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Kdj {
    pub k: Vec<f64>,
    pub d: Vec<f64>,
    pub j: Vec<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct OhlcBar {
    pub high: f64,
    pub low: f64,
    pub close: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IndicatorResults {
    pub macd: Macd,
    #[serde(rename = "priceKdj")]
    pub price_kdj: Kdj,
    #[serde(rename = "candleKdj")]
    pub candle_kdj: Kdj,
}

#[derive(Clone, Debug, PartialEq)]
pub enum IndicatorInputError {
    NonFinitePrice { index: usize },
    NonFiniteOhlc { index: usize },
    InvalidOhlcRange { index: usize },
    NonFiniteResult { series: &'static str, index: usize },
}

impl Display for IndicatorInputError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonFinitePrice { index } => {
                write!(formatter, "指标 prices[{index}] 必须是有限数值")
            }
            Self::NonFiniteOhlc { index } => {
                write!(
                    formatter,
                    "指标 candles[{index}] 的 high/low/close 必须是有限数值"
                )
            }
            Self::InvalidOhlcRange { index } => write!(
                formatter,
                "指标 candles[{index}] 必须满足 low <= close <= high 且 low <= high"
            ),
            Self::NonFiniteResult { series, index } => {
                write!(formatter, "指标计算结果 {series}[{index}] 非有限数值")
            }
        }
    }
}

impl std::error::Error for IndicatorInputError {}

fn validate_input(prices: &[f64], candles: &[OhlcBar]) -> Result<(), IndicatorInputError> {
    for (index, price) in prices.iter().enumerate() {
        if !price.is_finite() {
            return Err(IndicatorInputError::NonFinitePrice { index });
        }
    }
    for (index, candle) in candles.iter().enumerate() {
        if !candle.high.is_finite() || !candle.low.is_finite() || !candle.close.is_finite() {
            return Err(IndicatorInputError::NonFiniteOhlc { index });
        }
        if candle.low > candle.high || candle.close < candle.low || candle.close > candle.high {
            return Err(IndicatorInputError::InvalidOhlcRange { index });
        }
    }
    Ok(())
}

fn validate_results(results: &IndicatorResults) -> Result<(), IndicatorInputError> {
    let series = [
        ("MACD DIF", &results.macd.dif),
        ("MACD DEA", &results.macd.dea),
        ("MACD histogram", &results.macd.histogram),
        ("price KDJ K", &results.price_kdj.k),
        ("price KDJ D", &results.price_kdj.d),
        ("price KDJ J", &results.price_kdj.j),
        ("candle KDJ K", &results.candle_kdj.k),
        ("candle KDJ D", &results.candle_kdj.d),
        ("candle KDJ J", &results.candle_kdj.j),
    ];
    for (name, values) in series {
        if let Some(index) = values.iter().position(|value| !value.is_finite()) {
            return Err(IndicatorInputError::NonFiniteResult {
                series: name,
                index,
            });
        }
    }
    Ok(())
}

/// 单条 EMA 的短暂递推状态；各序列显式传入自己的两个系数。
struct EmaSmoother {
    previous: Option<f64>,
    current_weight: f64,
    previous_weight: f64,
}

impl EmaSmoother {
    fn new(current_weight: f64, previous_weight: f64) -> Self {
        Self {
            previous: None,
            current_weight,
            previous_weight,
        }
    }

    fn advance(&mut self, value: f64) -> f64 {
        let current = match self.previous {
            None => value,
            Some(previous) => value * self.current_weight + previous * self.previous_weight,
        };
        self.previous = Some(current);
        current
    }
}

fn macd(prices: &[f64]) -> Macd {
    let fast_weight = 2.0 / (12.0 + 1.0);
    let slow_weight = 2.0 / (26.0 + 1.0);
    let mut fast_smoother = EmaSmoother::new(fast_weight, 1.0 - fast_weight);
    let mut slow_smoother = EmaSmoother::new(slow_weight, 1.0 - slow_weight);
    let fast: Vec<_> = prices
        .iter()
        .copied()
        .map(|price| fast_smoother.advance(price))
        .collect();
    let slow: Vec<_> = prices
        .iter()
        .copied()
        .map(|price| slow_smoother.advance(price))
        .collect();
    let dif: Vec<f64> = fast
        .iter()
        .zip(slow)
        .map(|(fast, slow)| fast - slow)
        .collect();
    let mut signal_smoother = EmaSmoother::new(0.2, 0.8);
    let dea: Vec<_> = dif
        .iter()
        .copied()
        .map(|value| signal_smoother.advance(value))
        .collect();
    let histogram = dif
        .iter()
        .zip(&dea)
        .map(|(dif, dea)| (dif - dea) * 2.0)
        .collect();
    Macd {
        dif,
        dea,
        histogram,
    }
}

/// K/D 递推与输出序列共同推进，窗口 RSV 仍由各输入路径计算。
struct KdjAccumulator {
    previous_k: f64,
    previous_d: f64,
    series: Kdj,
}

impl KdjAccumulator {
    fn new(capacity: usize) -> Self {
        Self {
            previous_k: 50.0,
            previous_d: 50.0,
            series: Kdj {
                k: Vec::with_capacity(capacity),
                d: Vec::with_capacity(capacity),
                j: Vec::with_capacity(capacity),
            },
        }
    }

    fn push_rsv(&mut self, rsv: f64) {
        self.previous_k = self.previous_k * 2.0 / 3.0 + rsv / 3.0;
        self.previous_d = self.previous_d * 2.0 / 3.0 + self.previous_k / 3.0;
        self.series.k.push(self.previous_k);
        self.series.d.push(self.previous_d);
        self.series
            .j
            .push(3.0 * self.previous_k - 2.0 * self.previous_d);
    }

    fn finish(self) -> Kdj {
        self.series
    }
}

fn kdj(prices: &[f64]) -> Kdj {
    kdj_from_values(prices, prices, prices)
}

fn kdj_ohlc(bars: &[OhlcBar]) -> Kdj {
    let mut accumulator = KdjAccumulator::new(bars.len());
    for (index, bar) in bars.iter().enumerate() {
        let start = index.saturating_sub(8);
        let window = &bars[start..=index];
        let high = window
            .iter()
            .map(|item| item.high)
            .fold(f64::NEG_INFINITY, f64::max);
        let low = window
            .iter()
            .map(|item| item.low)
            .fold(f64::INFINITY, f64::min);
        let rsv = rsv(bar.close, high, low);
        accumulator.push_rsv(rsv);
    }
    accumulator.finish()
}

/// `compute`：会话计算上下文；`None` 时串行计算（消灭全局 rayon 池惰性初始化
/// 入口——HTTP 线程等非池环境曾实测 +128 常驻线程，#67 缺口①修1，
/// 先例：ledger_conservation::map_rows 的 Option<&ComputeContext> 口径）。
pub fn calculate_indicators(
    prices: &[f64],
    candles: &[OhlcBar],
    compute: Option<&std::sync::Arc<crate::compute::ComputeContext>>,
) -> Result<IndicatorResults, IndicatorInputError> {
    validate_input(prices, candles)?;
    let results = if prices.len().max(candles.len()) >= 2048 {
        match compute {
            Some(context) => {
                let (macd, (price_kdj, candle_kdj)) = context.join(
                    || macd(prices),
                    || context.join(|| kdj(prices), || kdj_ohlc(candles)),
                );
                IndicatorResults {
                    macd,
                    price_kdj,
                    candle_kdj,
                }
            }
            None => IndicatorResults {
                macd: macd(prices),
                price_kdj: kdj(prices),
                candle_kdj: kdj_ohlc(candles),
            },
        }
    } else {
        IndicatorResults {
            macd: macd(prices),
            price_kdj: kdj(prices),
            candle_kdj: kdj_ohlc(candles),
        }
    };
    validate_results(&results)?;
    Ok(results)
}

fn kdj_from_values(highs: &[f64], lows: &[f64], closes: &[f64]) -> Kdj {
    let mut accumulator = KdjAccumulator::new(closes.len());
    for (index, close) in closes.iter().copied().enumerate() {
        let start = index.saturating_sub(8);
        let high = highs[start..=index]
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        let low = lows[start..=index]
            .iter()
            .copied()
            .fold(f64::INFINITY, f64::min);
        accumulator.push_rsv(rsv(close, high, low));
    }
    accumulator.finish()
}

fn rsv(close: f64, high: f64, low: f64) -> f64 {
    if high == low {
        50.0
    } else {
        (close - low) / (high - low) * 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::{kdj, kdj_ohlc, macd, IndicatorInput, IndicatorInputError, OhlcBar};

    // 修2（增量十四）：batch 死入口删除时本混合测试被整块误删，现恢复其全部
    // 单项入口断言（NaN/Infinity/非法 OHLC/NonFiniteResult），去掉 batch 分支并
    // 适配三参签名——覆盖不降。
    #[test]
    fn indicator_calculation_rejects_non_finite_prices_and_invalid_ohlc() {
        assert_eq!(
            super::calculate_indicators(
                &[],
                &[OhlcBar {
                    high: f64::MAX,
                    low: -f64::MAX,
                    close: f64::MAX,
                }],
                None
            ),
            Err(IndicatorInputError::NonFiniteResult {
                series: "candle KDJ K",
                index: 0,
            })
        );
        assert_eq!(
            super::calculate_indicators(&[f64::NAN], &[], None),
            Err(IndicatorInputError::NonFinitePrice { index: 0 })
        );
        assert_eq!(
            super::calculate_indicators(&[f64::INFINITY], &[], None),
            Err(IndicatorInputError::NonFinitePrice { index: 0 })
        );
        assert_eq!(
            super::calculate_indicators(
                &[],
                &[OhlcBar {
                    high: f64::INFINITY,
                    low: 1.0,
                    close: 1.0,
                }],
                None
            ),
            Err(IndicatorInputError::NonFiniteOhlc { index: 0 })
        );
        assert_eq!(
            super::calculate_indicators(
                &[],
                &[OhlcBar {
                    high: 2.0,
                    low: f64::NAN,
                    close: 1.5,
                }],
                None
            ),
            Err(IndicatorInputError::NonFiniteOhlc { index: 0 })
        );
        assert_eq!(
            super::calculate_indicators(
                &[],
                &[OhlcBar {
                    high: 2.0,
                    low: 1.0,
                    close: f64::NEG_INFINITY,
                }],
                None
            ),
            Err(IndicatorInputError::NonFiniteOhlc { index: 0 })
        );
        assert_eq!(
            super::calculate_indicators(
                &[],
                &[OhlcBar {
                    high: 1.0,
                    low: 2.0,
                    close: 1.5,
                }],
                None
            ),
            Err(IndicatorInputError::InvalidOhlcRange { index: 0 })
        );
        // 修1（增量十五）：十三删除记录（incremental-vs-runnerfinal.patch:229 区）
        // 共 9 条单项断言，十四复原 7 条，此处补回遗漏两条——close 越出 [low,high]
        // 上侧与下侧各自成断言；补齐后与删除记录逐条对照 9/9 全复原。
        assert_eq!(
            super::calculate_indicators(
                &[],
                &[OhlcBar {
                    high: 2.0,
                    low: 1.0,
                    close: 2.5,
                }],
                None
            ),
            Err(IndicatorInputError::InvalidOhlcRange { index: 0 })
        );
        assert_eq!(
            super::calculate_indicators(
                &[],
                &[OhlcBar {
                    high: 2.0,
                    low: 1.0,
                    close: 0.5,
                }],
                None
            ),
            Err(IndicatorInputError::InvalidOhlcRange { index: 0 })
        );
    }

    #[test]
    fn recurrence_protection_spans_ema_period_and_kdj_window_rollover() {
        let prices: Vec<_> = (0..32)
            .map(|index| 100.0 + ((index * 7) % 19) as f64)
            .collect();
        let actual = macd(&prices);
        let (mut fast, mut slow, mut signal) = (prices[0], prices[0], 0.0);
        for (index, price) in prices.iter().copied().enumerate() {
            if index > 0 {
                fast = price * (2.0 / 13.0) + fast * (1.0 - 2.0 / 13.0);
                slow = price * (2.0 / 27.0) + slow * (1.0 - 2.0 / 27.0);
                signal = (fast - slow) * 0.2 + signal * 0.8;
            }
            assert_eq!(actual.dif[index].to_bits(), (fast - slow).to_bits());
            assert_eq!(actual.dea[index].to_bits(), signal.to_bits());
            assert_eq!(
                actual.histogram[index].to_bits(),
                ((fast - slow - signal) * 2.0).to_bits()
            );
        }
        let closes = [1.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 17.0, 10.0];
        let close_result = kdj(&closes);
        let bars: Vec<_> = closes
            .iter()
            .map(|&close| OhlcBar {
                high: close,
                low: close,
                close,
            })
            .collect();
        let candle_result = kdj_ohlc(&bars);
        assert_eq!(close_result, candle_result);
        // 第十个样本使首次低点滚出九样本窗口，RSV 回到零。
        let expected_k = close_result.k[8] * 2.0 / 3.0;
        assert_eq!(close_result.k[9].to_bits(), expected_k.to_bits());
        assert_eq!(
            macd(&[5.0]),
            super::Macd {
                dif: vec![0.0],
                dea: vec![0.0],
                histogram: vec![0.0]
            }
        );
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-10, "{actual} != {expected}");
    }

    #[test]
    fn macd_preserves_ema_seeds_and_12_26_9_parameters() {
        let result = macd(&[10.0, 12.0]);

        assert_eq!(result.dif.len(), 2);
        assert_eq!(result.dea.len(), 2);
        assert_eq!(result.histogram.len(), 2);
        assert_close(result.dif[0], 0.0);
        assert_close(result.dea[0], 0.0);
        assert_close(result.dif[1], 2.0 / 13.0 * 2.0 - 2.0 / 27.0 * 2.0);
        assert_close(result.dea[1], result.dif[1] * 0.2);
        assert_close(result.histogram[1], (result.dif[1] - result.dea[1]) * 2.0);
    }

    #[test]
    fn kdj_uses_nine_close_samples_and_seeds_smoothing_at_fifty() {
        let result = kdj(&[10.0, 12.0, 11.0]);

        assert_eq!(result.k.len(), 3);
        assert_eq!(result.d.len(), 3);
        assert_eq!(result.j.len(), 3);
        assert_close(result.k[0], 50.0);
        assert_close(result.d[0], 50.0);
        assert_close(result.j[0], 50.0);
        assert_close(result.k[1], 200.0 / 3.0);
        assert_close(result.d[1], 500.0 / 9.0);
        assert_close(result.j[1], 800.0 / 9.0);
    }

    #[test]
    fn kdj_ohlc_uses_bar_high_low_close_values() {
        let result = kdj_ohlc(&[
            OhlcBar {
                high: 12.0,
                low: 8.0,
                close: 10.0,
            },
            OhlcBar {
                high: 14.0,
                low: 8.0,
                close: 14.0,
            },
        ]);

        assert_close(result.k[0], 50.0);
        assert_close(result.d[0], 50.0);
        assert_close(result.k[1], 200.0 / 3.0);
        assert_close(result.d[1], 500.0 / 9.0);
        assert_close(result.j[1], 800.0 / 9.0);
    }

    #[test]
    fn flat_ohlc_history_keeps_all_kdj_values_at_fifty() {
        let bars = vec![
            OhlcBar {
                high: 10.0,
                low: 10.0,
                close: 10.0,
            };
            12
        ];
        let result = kdj_ohlc(&bars);

        assert_eq!(result.k, vec![50.0; bars.len()]);
        assert_eq!(result.d, vec![50.0; bars.len()]);
        assert_eq!(result.j, vec![50.0; bars.len()]);
    }

    #[test]
    fn indicators_return_empty_series_for_empty_prices() {
        assert_eq!(macd(&[]).dif, Vec::<f64>::new());
        assert_eq!(kdj(&[]).k, Vec::<f64>::new());
    }

    #[test]
    fn indicator_result_serializes_with_transport_field_names() {
        let result =
            super::calculate_indicators(&[10.0], &[], None).expect("valid input should calculate");
        let value = serde_json::to_value(result).expect("indicator result should serialize");

        assert!(value.get("priceKdj").is_some());
        assert!(value.get("candleKdj").is_some());
        assert_eq!(value["macd"]["histogram"].as_array().unwrap().len(), 1);
        assert!(value["candleKdj"]["k"].as_array().unwrap().is_empty());
    }

    #[test]
    fn production_indicator_entry_matches_sequential_components_in_a_worker_pool() {
        let prices: Vec<_> = (0..2048).map(|index| 10.0 + (index % 17) as f64).collect();
        let candles: Vec<_> = prices
            .iter()
            .map(|price| super::OhlcBar {
                high: price + 1.0,
                low: price - 1.0,
                close: *price,
            })
            .collect();
        let expected = super::IndicatorResults {
            macd: macd(&prices),
            price_kdj: kdj(&prices),
            candle_kdj: kdj_ohlc(&candles),
        };
        // 修1 后并行分支只经会话 ComputeContext（消灭全局池入口）；本测试随之
        // 用真实 8-worker 上下文覆盖 Some 路径，None 串行路径由
        // indicators_over_2048_points_do_not_spawn_threads 护栏覆盖。
        let context = crate::compute::ComputeContext::with_workers(8).unwrap();
        let actual = super::calculate_indicators(&prices, &candles, Some(&context)).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(
            super::calculate_indicators(&[], &[], None).unwrap().macd.dif,
            Vec::<f64>::new()
        );
    }
}

/// #67 缺口①·修1/修3 护栏（增量十五观测可靠性收口）：≥2048 点指标计算
/// （None 上下文）期间与之后的进程线程峰值均不得增长。修复前该分支走自由函数
/// crate::compute::join（全局 rayon 池惰性初始化；HTTP 线程等非池环境实测常驻
/// +128 线程，R5 257→385）。
///
/// 覆盖主张（如实收窄）：本护栏主张限定为「独立首触语义下，常驻型线程泄漏
/// （+128 级全局池初始化）必被抓」。周期采样（200µs）存在调度间隙盲区，
/// **不主张**瞬时峰的可靠捕获——低于采样周期的瞬时线程创建/销毁可能漏采，
/// 该边界为已知限制。池已被其他代码初始化时护栏空转（rayon 无查询接口，
/// 该状态不可检）；口径依赖独立进程首触。
///
/// 口径（完整命令，勿用子串过滤以免空跑；--exact 需含模块全路径）：
///   cargo test -p engine --lib indicators::indicators_over_2048_points_do_not_spawn_threads -- --ignored --exact --test-threads 1
#[test]
#[ignore = "口径=独立进程首触：cargo test -p engine --lib indicators::indicators_over_2048_points_do_not_spawn_threads -- --ignored --exact --test-threads 1；并行套件中兄弟测试线程活动噪声化进程计数，且池已初始化时护栏空转（状态不可检）——均为口径限制非缺陷"]
#[cfg(target_os = "linux")]
fn indicators_over_2048_points_do_not_spawn_threads() {
    fn task_count() -> usize {
        // /proc/self/task 条目数=线程数（与 /proc/self/status Threads 等效）。
        // 修2b：读取/遍历失败显式 panic（防御式原则，不吞错不用默认值）。
        let entries = std::fs::read_dir("/proc/self/task")
            .expect("linux 必须可读 /proc/self/task");
        let mut count = 0;
        for entry in entries {
            entry.expect("task 条目读取不得失败");
            count += 1;
        }
        count
    }
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::sync::Arc;
    let prices: Vec<f64> = (0..4096).map(|index| index as f64).collect();
    let before = task_count();
    // 修2a：采样启动握手——计算开始前等采样线程完成首采（peak=0 空过不可能），
    // 并在断言中核验 samples_count>0。
    let peak = Arc::new(AtomicUsize::new(0));
    let samples = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let (first_tx, first_rx) = mpsc::channel::<usize>();
    let sampler_peak = Arc::clone(&peak);
    let sampler_samples = Arc::clone(&samples);
    let sampler_stop = Arc::clone(&stop);
    let sampler = std::thread::spawn(move || {
        let mut first_sent = false;
        while !sampler_stop.load(Ordering::Relaxed) {
            let count = task_count();
            sampler_peak.fetch_max(count, Ordering::Relaxed);
            sampler_samples.fetch_add(1, Ordering::Relaxed);
            if !first_sent {
                first_sent = true;
                first_tx.send(count).expect("首采握手通道必须可达");
            }
            std::thread::sleep(std::time::Duration::from_micros(200));
        }
    });
    let first_sample = first_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("采样线程必须在计算开始前完成首采");
    assert!(first_sample >= before, "首采线程数不得低于计算前基线");
    let results = calculate_indicators(&prices, &[], None);
    stop.store(true, Ordering::Relaxed);
    sampler.join().expect("采样线程必须可 join");
    let after = task_count();
    let max_observed = peak.load(Ordering::Relaxed);
    let samples_count = samples.load(Ordering::Relaxed);
    assert!(results.is_ok());
    assert!(samples_count > 0, "无样本即无观测：peak=0 空过不允许");
    // 期望：串行路径零线程增长；容差 2 = 采样线程自身 +1 与调度边界 +1（旁注）。
    assert!(
        max_observed <= before + 2 && after <= before + 2,
        "≥2048 点指标计算不得触发线程增长（before={before} max_observed={max_observed} after={after} samples={samples_count}；期望零增长，容差 2）"
    );
}


