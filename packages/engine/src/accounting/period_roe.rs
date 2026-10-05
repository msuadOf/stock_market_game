//! 期间时间加权平均权益分析指标。
//!
//! 该指标按指定范围的期间净利润与有日期权益变动计算，不是法定披露“加权平均净资产收益率”。
//! 期间为 `(start, end]` 的自然日序列：期初权益是 start 时点余额；变动自其日期当日生效，
//! start 当日变动属于期初余额事实，不应再重复提供。end 当日变动计入最后一个期间日。
//! 只按输入的期初权益和带日期权益变动计算平均权益；期间净利润用于 ROE 分子，不臆测
//! 利润何时形成或另加利润权重。调用方若有利润已转入权益的具体生效事实，应作为带日期变动输入。
//! 因此是明确的时间加权分析指标基础，不冒充法定披露 ROE。单位均为元，内部金额沿用
//! `AccountingAmount` 的分；比率保留为精确有理数，调用方自行格式化展示。

use crate::accounting::{AccountingAmount, ScopeId};
use crate::calendar::CivilDate;
use num_bigint::BigInt;

/// 一项按日期生效且属于特定报告范围的权益变动（单位：元）。
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct DatedEquityChange {
    pub scope: ScopeId,
    pub basis: EquityBasis,
    pub date: CivilDate,
    pub delta: AccountingAmount,
}

/// 权益归属范围；利润和平均权益必须绑定同一归属口径。
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum EquityBasis {
    Total,
    AttributableToParent,
}

/// 属于明确报告范围的会计金额。
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct ScopedAccountingAmount {
    pub scope: ScopeId,
    pub basis: EquityBasis,
    pub amount: AccountingAmount,
}

/// 比率 `numerator / denominator`；分子、分母来自精确分值，不舍入成浮点数。
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct ExactRatio {
    pub numerator: i128,
    pub denominator: i128,
}

/// 可用的期间时间加权权益 ROE 分析结果。
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct PeriodRoe {
    pub scope: ScopeId,
    pub basis: EquityBasis,
    pub start_exclusive: CivilDate,
    pub end_inclusive: CivilDate,
    pub period_net_income: AccountingAmount,
    /// 权益日总和 / 天数，以分表示并精确保留不足一分。
    pub average_equity_cents: ExactRatio,
    /// 净利润 / 平均权益，精确有理数；这不是披露标准 ROE。
    pub rate: ExactRatio,
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub enum PeriodRoeOutcome {
    Available(PeriodRoe),
    Unavailable(PeriodRoeUnavailable),
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub enum PeriodRoeUnavailable {
    NonPositiveAverageEquity,
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub enum PeriodRoeError {
    InvalidWindow {
        start: CivilDate,
        end: CivilDate,
    },
    ScopeMismatch {
        expected: ScopeId,
        actual: ScopeId,
    },
    EquityBasisMismatch {
        expected: EquityBasis,
        actual: EquityBasis,
    },
    ChangeOutsideWindow {
        date: CivilDate,
        start: CivilDate,
        end: CivilDate,
    },
    ArithmeticOverflow {
        operation: &'static str,
    },
}

/// 计算分析用期间时间加权平均权益 ROE。
/// `changes` 必须全部属于同一 `scope` 且日期位于 `(start_exclusive, end_inclusive]`。
/// 同日多项权益变动在该日开始前后合并，不假定日内顺序；调用方必须与利润采用相同合并或
/// 单体报告范围。平均权益非正时返回明确不可用；任何中间乘加溢出均返回错误。此指标基础
/// 不等同于监管披露 ROE 公式。
pub fn period_time_weighted_equity_roe(
    start_exclusive: CivilDate,
    end_inclusive: CivilDate,
    opening_equity: ScopedAccountingAmount,
    period_net_income: ScopedAccountingAmount,
    changes: &[DatedEquityChange],
) -> Result<PeriodRoeOutcome, PeriodRoeError> {
    let scope = &opening_equity.scope;
    if period_net_income.scope != *scope {
        return Err(PeriodRoeError::ScopeMismatch {
            expected: scope.clone(),
            actual: period_net_income.scope,
        });
    }
    if period_net_income.basis != opening_equity.basis {
        return Err(PeriodRoeError::EquityBasisMismatch {
            expected: opening_equity.basis,
            actual: period_net_income.basis,
        });
    }
    let days = end_inclusive.days_since(start_exclusive);
    if days <= 0 {
        return Err(PeriodRoeError::InvalidWindow {
            start: start_exclusive,
            end: end_inclusive,
        });
    }
    let mut ordered = changes.to_vec();
    for change in &ordered {
        if &change.scope != scope {
            return Err(PeriodRoeError::ScopeMismatch {
                expected: scope.clone(),
                actual: change.scope.clone(),
            });
        }
        if change.basis != opening_equity.basis {
            return Err(PeriodRoeError::EquityBasisMismatch {
                expected: opening_equity.basis,
                actual: change.basis,
            });
        }
        if change.date <= start_exclusive || change.date > end_inclusive {
            return Err(PeriodRoeError::ChangeOutsideWindow {
                date: change.date,
                start: start_exclusive,
                end: end_inclusive,
            });
        }
    }
    ordered.sort_by_key(|change| change.date);

    let mut weighted_equity_days = BigInt::from(0);
    let mut current_equity = BigInt::from(opening_equity.amount.cents());
    let mut cursor = start_exclusive;
    let mut index = 0;
    while index < ordered.len() {
        let date = ordered[index].date;
        let segment_days = i128::from(date.days_since(cursor) - 1);
        weighted_equity_days += &current_equity * segment_days;
        let mut same_day_delta = BigInt::from(0);
        while index < ordered.len() && ordered[index].date == date {
            same_day_delta += ordered[index].delta.cents();
            index += 1;
        }
        current_equity += same_day_delta;
        if i128::try_from(current_equity.clone()).is_err() {
            return Err(PeriodRoeError::ArithmeticOverflow {
                operation: "equity after same-day changes",
            });
        }
        cursor = date;
    }
    let tail_days = i128::from(end_inclusive.days_since(cursor) + 1);
    weighted_equity_days += &current_equity * tail_days;
    let weighted_equity_days =
        i128::try_from(weighted_equity_days).map_err(|_| PeriodRoeError::ArithmeticOverflow {
            operation: "weighted equity days sum",
        })?;
    if weighted_equity_days <= 0 {
        return Ok(PeriodRoeOutcome::Unavailable(
            PeriodRoeUnavailable::NonPositiveAverageEquity,
        ));
    }
    let roe_numerator = period_net_income
        .amount
        .cents()
        .checked_mul(i128::from(days))
        .ok_or(PeriodRoeError::ArithmeticOverflow {
            operation: "net income cents × period days",
        })?;
    Ok(PeriodRoeOutcome::Available(PeriodRoe {
        scope: scope.clone(),
        basis: opening_equity.basis,
        start_exclusive,
        end_inclusive,
        period_net_income: period_net_income.amount,
        average_equity_cents: ExactRatio {
            numerator: weighted_equity_days,
            denominator: i128::from(days),
        },
        rate: ExactRatio {
            numerator: roe_numerator,
            denominator: weighted_equity_days,
        },
    }))
}

#[cfg(test)]
mod tests;
