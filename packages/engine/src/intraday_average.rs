use crate::session::DailyTradeStats;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntradayAverage {
    pub turnover_cents: u64,
    pub volume_shares: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum IntradayAverageError {
    #[error("成交额、股数与成交笔数不一致：成交额 {turnover_cents} 分，股数 {volume_shares} 股，成交笔数 {trade_count}")]
    InconsistentDailyTradeStats {
        turnover_cents: u64,
        volume_shares: u64,
        trade_count: u64,
    },
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

#[cfg(test)]
mod tests {
    use super::{calculate_intraday_average, IntradayAverage, IntradayAverageError};
    use crate::session::DailyTradeStats;

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
}
