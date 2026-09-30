use rayon::prelude::*;
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

fn macd(prices: &[f64]) -> Macd {
    let ema = |period: usize| {
        let smoothing = 2.0 / (period as f64 + 1.0);
        let mut values = Vec::with_capacity(prices.len());
        let mut previous = prices.first().copied().unwrap_or(0.0);
        for (index, price) in prices.iter().copied().enumerate() {
            previous = if index == 0 {
                price
            } else {
                price * smoothing + previous * (1.0 - smoothing)
            };
            values.push(previous);
        }
        values
    };
    let fast = ema(12);
    let slow = ema(26);
    let dif: Vec<f64> = fast
        .iter()
        .zip(slow)
        .map(|(fast, slow)| fast - slow)
        .collect();
    let mut dea = Vec::with_capacity(dif.len());
    let mut previous = dif.first().copied().unwrap_or(0.0);
    for (index, value) in dif.iter().copied().enumerate() {
        previous = if index == 0 {
            value
        } else {
            value * 0.2 + previous * 0.8
        };
        dea.push(previous);
    }
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

fn kdj(prices: &[f64]) -> Kdj {
    kdj_from_values(prices, prices, prices)
}

fn kdj_ohlc(bars: &[OhlcBar]) -> Kdj {
    let mut k = Vec::with_capacity(bars.len());
    let mut d = Vec::with_capacity(bars.len());
    let mut j = Vec::with_capacity(bars.len());
    let mut previous_k = 50.0;
    let mut previous_d = 50.0;
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
        push_kdj(
            rsv,
            &mut previous_k,
            &mut previous_d,
            &mut k,
            &mut d,
            &mut j,
        );
    }
    Kdj { k, d, j }
}

pub fn calculate_indicators(
    prices: &[f64],
    candles: &[OhlcBar],
) -> Result<IndicatorResults, IndicatorInputError> {
    validate_input(prices, candles)?;
    let results = IndicatorResults {
        macd: macd(prices),
        price_kdj: kdj(prices),
        candle_kdj: kdj_ohlc(candles),
    };
    validate_results(&results)?;
    Ok(results)
}

pub fn calculate_indicators_batch(
    inputs: &[IndicatorInput],
) -> Result<Vec<IndicatorResults>, IndicatorInputError> {
    for input in inputs {
        validate_input(&input.prices, &input.candles)?;
    }
    let results: Vec<_> = inputs
        .par_iter()
        .map(|input| IndicatorResults {
            macd: macd(&input.prices),
            price_kdj: kdj(&input.prices),
            candle_kdj: kdj_ohlc(&input.candles),
        })
        .collect();
    for result in &results {
        validate_results(result)?;
    }
    Ok(results)
}

fn kdj_from_values(highs: &[f64], lows: &[f64], closes: &[f64]) -> Kdj {
    let mut k = Vec::with_capacity(closes.len());
    let mut d = Vec::with_capacity(closes.len());
    let mut j = Vec::with_capacity(closes.len());
    let mut previous_k = 50.0;
    let mut previous_d = 50.0;
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
        push_kdj(
            rsv(close, high, low),
            &mut previous_k,
            &mut previous_d,
            &mut k,
            &mut d,
            &mut j,
        );
    }
    Kdj { k, d, j }
}

fn rsv(close: f64, high: f64, low: f64) -> f64 {
    if high == low {
        50.0
    } else {
        (close - low) / (high - low) * 100.0
    }
}

fn push_kdj(
    rsv: f64,
    previous_k: &mut f64,
    previous_d: &mut f64,
    k: &mut Vec<f64>,
    d: &mut Vec<f64>,
    j: &mut Vec<f64>,
) {
    *previous_k = *previous_k * 2.0 / 3.0 + rsv / 3.0;
    *previous_d = *previous_d * 2.0 / 3.0 + *previous_k / 3.0;
    k.push(*previous_k);
    d.push(*previous_d);
    j.push(3.0 * *previous_k - 2.0 * *previous_d);
}

#[cfg(test)]
mod tests {
    use super::{kdj, kdj_ohlc, macd, IndicatorInput, IndicatorInputError, OhlcBar};

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
            super::calculate_indicators(&[10.0], &[]).expect("valid input should calculate");
        let value = serde_json::to_value(result).expect("indicator result should serialize");

        assert!(value.get("priceKdj").is_some());
        assert!(value.get("candleKdj").is_some());
        assert_eq!(value["macd"]["histogram"].as_array().unwrap().len(), 1);
        assert!(value["candleKdj"]["k"].as_array().unwrap().is_empty());
    }

    #[test]
    fn indicator_batch_keeps_input_order_and_accepts_optional_candles() {
        let inputs: Vec<super::IndicatorInput> = serde_json::from_value(serde_json::json!([
            { "prices": [10.0, 12.0] },
            { "prices": [20.0, 21.0], "candles": [] }
        ]))
        .expect("candles may be omitted for close-only indicators");

        let results =
            super::calculate_indicators_batch(&inputs).expect("valid batch should calculate");

        assert_eq!(results.len(), 2);
        assert_close(results[0].macd.dif[1], macd(&inputs[0].prices).dif[1]);
        assert_close(results[1].macd.dif[1], macd(&inputs[1].prices).dif[1]);
        assert!(super::calculate_indicators_batch(&[])
            .expect("empty batch should calculate")
            .is_empty());
    }

    #[test]
    fn indicator_calculation_rejects_non_finite_prices_and_invalid_ohlc() {
        assert_eq!(
            super::calculate_indicators(
                &[],
                &[OhlcBar {
                    high: f64::MAX,
                    low: -f64::MAX,
                    close: f64::MAX,
                }]
            ),
            Err(IndicatorInputError::NonFiniteResult {
                series: "candle KDJ K",
                index: 0,
            })
        );
        assert_eq!(
            super::calculate_indicators_batch(&[IndicatorInput {
                prices: vec![],
                candles: vec![OhlcBar {
                    high: f64::MAX,
                    low: -f64::MAX,
                    close: f64::MAX,
                }],
            }]),
            Err(IndicatorInputError::NonFiniteResult {
                series: "candle KDJ K",
                index: 0,
            })
        );
        assert_eq!(
            super::calculate_indicators(&[f64::NAN], &[]),
            Err(IndicatorInputError::NonFinitePrice { index: 0 })
        );
        assert_eq!(
            super::calculate_indicators(&[f64::INFINITY], &[]),
            Err(IndicatorInputError::NonFinitePrice { index: 0 })
        );
        assert_eq!(
            super::calculate_indicators(
                &[],
                &[OhlcBar {
                    high: f64::INFINITY,
                    low: 1.0,
                    close: 1.0,
                }]
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
                }]
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
                }]
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
                }]
            ),
            Err(IndicatorInputError::InvalidOhlcRange { index: 0 })
        );
        assert_eq!(
            super::calculate_indicators(
                &[],
                &[OhlcBar {
                    high: 2.0,
                    low: 1.0,
                    close: 2.5,
                }]
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
                }]
            ),
            Err(IndicatorInputError::InvalidOhlcRange { index: 0 })
        );
        assert_eq!(
            super::calculate_indicators_batch(&[IndicatorInput {
                prices: vec![1.0, f64::NAN],
                candles: vec![],
            }]),
            Err(IndicatorInputError::NonFinitePrice { index: 1 })
        );
    }
}
