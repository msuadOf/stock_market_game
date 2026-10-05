use crate::session::DailyTradeStats;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct IntradayAverage {
    #[serde(with = "crate::orderbook::canonical_u128_decimal")]
    #[ts(type = "string")]
    pub turnover_cents: u128,
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub volume_shares: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct IntradayAverageInput {
    #[serde(with = "crate::orderbook::canonical_u128_decimal")]
    #[ts(type = "string")]
    pub turnover_cents: u128,
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub trade_count: u64,
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub volume_shares: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum IntradayAverageError {
    #[error("成交额、股数与成交笔数不一致：成交额 {turnover_cents} 分，股数 {volume_shares} 股，成交笔数 {trade_count}")]
    InconsistentDailyTradeStats {
        turnover_cents: u128,
        volume_shares: u64,
        trade_count: u64,
    },
}

pub fn parse_turnover_cents(value: &str) -> Result<u128, String> {
    crate::orderbook::canonical_u128_decimal::parse(value)
}

/// 从真实日累计成交统计中生成精确分子和分母，不在 engine 内提前舍入均价。
pub fn calculate_intraday_average(
    stats: Option<&DailyTradeStats>,
    volume_shares: u64,
) -> Result<Option<IntradayAverage>, IntradayAverageError> {
    let Some(stats) = stats else {
        return Ok(None);
    };

    if volume_shares == 0 {
        if stats.turnover_cents != 0 || stats.trade_count != 0 {
            return Err(IntradayAverageError::InconsistentDailyTradeStats {
                turnover_cents: stats.turnover_cents,
                volume_shares,
                trade_count: stats.trade_count,
            });
        }
        return Ok(None);
    }

    if stats.turnover_cents == 0 || stats.trade_count == 0 || volume_shares < stats.trade_count {
        return Err(IntradayAverageError::InconsistentDailyTradeStats {
            turnover_cents: stats.turnover_cents,
            volume_shares,
            trade_count: stats.trade_count,
        });
    }

    Ok(Some(IntradayAverage {
        turnover_cents: stats.turnover_cents,
        volume_shares,
    }))
}

pub fn calculate_intraday_average_curve(
    samples: &[IntradayAverageInput],
) -> Result<Vec<Option<IntradayAverage>>, IntradayAverageError> {
    samples
        .iter()
        .map(|sample| {
            let stats = DailyTradeStats {
                turnover_cents: sample.turnover_cents,
                trade_count: sample.trade_count,
            };
            calculate_intraday_average(Some(&stats), sample.volume_shares)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        calculate_intraday_average, calculate_intraday_average_curve, IntradayAverage,
        IntradayAverageError, IntradayAverageInput,
    };
    use crate::session::DailyTradeStats;

    #[test]
    fn average_retains_wide_aggregate_and_strict_wire() {
        let json = r#"{"turnover_cents":"20000000000000000000","trade_count":4,"volume_shares":4}"#;
        let input: IntradayAverageInput = serde_json::from_str(json).unwrap();
        let result = calculate_intraday_average_curve(&[input]).unwrap();
        assert_eq!(
            result[0].unwrap().turnover_cents,
            20_000_000_000_000_000_000
        );
        assert_eq!(
            serde_json::to_value(result[0].unwrap()).unwrap()["turnover_cents"],
            "20000000000000000000"
        );
        for invalid in ["00", "+1", "-1", "340282366920938463463374607431768211456"] {
            assert!(super::parse_turnover_cents(invalid).is_err());
        }
        assert_eq!(
            super::parse_turnover_cents(&u128::MAX.to_string()).unwrap(),
            u128::MAX
        );
    }

    #[test]
    fn keeps_exact_turnover_and_share_ratio_without_rounding() {
        let stats = DailyTradeStats {
            turnover_cents: 260_000,
            trade_count: 2,
        };
        assert_eq!(
            calculate_intraday_average(Some(&stats), 300),
            Ok(Some(IntradayAverage {
                turnover_cents: 260_000,
                volume_shares: 300,
            })),
        );
    }

    #[test]
    fn missing_statistics_and_consistent_zero_trades_are_unavailable() {
        assert_eq!(calculate_intraday_average(None, 0), Ok(None));
        assert_eq!(calculate_intraday_average(None, 100), Ok(None));
        assert_eq!(
            calculate_intraday_average(Some(&DailyTradeStats::default()), 0),
            Ok(None)
        );
    }

    #[test]
    fn inconsistent_statistics_fail_explicitly() {
        let stats = DailyTradeStats {
            turnover_cents: 1,
            trade_count: 1,
        };
        assert_eq!(
            calculate_intraday_average(Some(&stats), 0),
            Err(IntradayAverageError::InconsistentDailyTradeStats {
                turnover_cents: 1,
                volume_shares: 0,
                trade_count: 1,
            }),
        );
        let stats = DailyTradeStats {
            turnover_cents: 0,
            trade_count: 1,
        };
        assert!(calculate_intraday_average(Some(&stats), 1).is_err());
        let stats = DailyTradeStats {
            turnover_cents: 1,
            trade_count: 2,
        };
        assert!(calculate_intraday_average(Some(&stats), 1).is_err());
    }

    #[test]
    fn calculates_a_curve_in_one_pure_rust_call() {
        let result = calculate_intraday_average_curve(&[
            IntradayAverageInput {
                turnover_cents: 100_000,
                trade_count: 1,
                volume_shares: 100,
            },
            IntradayAverageInput {
                turnover_cents: 260_000,
                trade_count: 2,
                volume_shares: 300,
            },
        ])
        .unwrap();
        assert_eq!(
            result,
            vec![
                Some(IntradayAverage {
                    turnover_cents: 100_000,
                    volume_shares: 100
                }),
                Some(IntradayAverage {
                    turnover_cents: 260_000,
                    volume_shares: 300
                }),
            ]
        );
        assert_eq!(
            calculate_intraday_average_curve(&[
                IntradayAverageInput {
                    turnover_cents: 100_000,
                    trade_count: 1,
                    volume_shares: 100
                },
                IntradayAverageInput {
                    turnover_cents: 1,
                    trade_count: 1,
                    volume_shares: 0
                },
            ]),
            Err(IntradayAverageError::InconsistentDailyTradeStats {
                turnover_cents: 1,
                volume_shares: 0,
                trade_count: 1,
            })
        );
    }
}
