//! 日内均价（VWAP）纯函数域（B2 补面——按 18725d07 基线恢复 wasm 纯函数面）。
//!
//! B2 重建时只保留了会话查询路径（`GameSession::query_intraday_average`，
//! `DailyTradeStats` u64 域），漏掉了 wasm 宿主需要的独立纯函数面
//! （`IntradayAverageInput` 反序列化、`parse_turnover_cents` 字符串解析、
//! `calculate_intraday_average_curve` 批量曲线）与 `IntradayAverage` 的
//! Serialize/TS 导出——W3 web 接线（host/intraday-average.ts 归一层与
//! PriceChart 的 Rust 指标源路径）依赖此面，按旧基线语义恢复：
//! - wire 上 `turnover_cents` 为 u128 十进制字符串（合法成交额可超 u64）；
//! - `volume_shares`/`trade_count` 为 js 安全 u64 数值；
//! - 一致性规则（额/量/笔数互检）单点实现于 `intraday_average_from_parts`，
//!   会话查询路径（u64）经加宽复用同一规则，不复制判定。

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

/// 一致性规则单点（额/量/笔数互检+None 语义）——两条入口共用。
fn intraday_average_from_parts(
    turnover_cents: u128,
    trade_count: u64,
    volume_shares: u64,
) -> Result<Option<IntradayAverage>, IntradayAverageError> {
    if volume_shares == 0 {
        if turnover_cents != 0 || trade_count != 0 {
            return Err(IntradayAverageError::InconsistentDailyTradeStats {
                turnover_cents,
                volume_shares,
                trade_count,
            });
        }
        return Ok(None);
    }

    if turnover_cents == 0 || trade_count == 0 || volume_shares < trade_count {
        return Err(IntradayAverageError::InconsistentDailyTradeStats {
            turnover_cents,
            volume_shares,
            trade_count,
        });
    }

    Ok(Some(IntradayAverage {
        turnover_cents,
        volume_shares,
    }))
}

/// 从真实日累计成交统计中生成精确分子和分母，不在 engine 内提前舍入均价。
pub fn calculate_intraday_average(
    stats: Option<&DailyTradeStats>,
    volume_shares: u64,
) -> Result<Option<IntradayAverage>, IntradayAverageError> {
    match stats {
        None => Ok(None),
        Some(stats) => intraday_average_from_parts(
            u128::from(stats.turnover_cents),
            stats.trade_count,
            volume_shares,
        ),
    }
}

pub fn calculate_intraday_average_curve(
    samples: &[IntradayAverageInput],
) -> Result<Vec<Option<IntradayAverage>>, IntradayAverageError> {
    samples
        .iter()
        .map(|sample| {
            intraday_average_from_parts(
                sample.turnover_cents,
                sample.trade_count,
                sample.volume_shares,
            )
        })
        .collect()
}

/// 单点 wire 输入直算（wasm 宿主用）——u128 wire 域不经 u64 会话域
/// （`DailyTradeStats`）转一道，杜绝静默截断（铁律二）。
pub fn calculate_intraday_average_from_input(
    input: &IntradayAverageInput,
) -> Result<Option<IntradayAverage>, IntradayAverageError> {
    intraday_average_from_parts(input.turnover_cents, input.trade_count, input.volume_shares)
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
                    volume_shares: 100,
                },
                IntradayAverageInput {
                    turnover_cents: 1,
                    trade_count: 1,
                    volume_shares: 0,
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
