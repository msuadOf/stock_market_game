//! 证监会公告〔2010〕2号规则9披露 ROE 计算。
//!
//! 本实现限于规则第4条一般公式；同一控制下企业合并及比较期间特殊处理明确返回
//! 不支持，不以一般公式替代。月份权重依据权益实际生效日期，采用变动次月起至
//! 报告期末的整月数。输入绑定公开报告期间、报告范围与普通股股东归属口径。

use crate::accounting::{AccountingAmount, ScopeId};
use crate::calendar::CivilDate;
use num_bigint::BigInt;
use thiserror::Error;

#[cfg(test)]
mod tests;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct DisclosureReportPeriod {
    pub start: CivilDate,
    pub end: CivilDate,
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub struct PublicReportIdentity {
    pub period: DisclosureReportPeriod,
    pub scope: ScopeId,
    pub attribution: DisclosureAttributionBasis,
    pub published_on: CivilDate,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum DisclosureAttributionBasis {
    AttributableToOrdinaryShareholders,
    ConsolidatedIncludingMinorityInterest,
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub struct ReportFact {
    pub period: DisclosureReportPeriod,
    pub scope: ScopeId,
    pub attribution: DisclosureAttributionBasis,
    pub value: AccountingAmount,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum EquityEventKind {
    Increase,
    Decrease,
    Other,
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub struct ActualEquityEvent {
    pub period: DisclosureReportPeriod,
    pub scope: ScopeId,
    pub attribution: DisclosureAttributionBasis,
    pub effective_on: CivilDate,
    pub kind: EquityEventKind,
    /// `Increase`/`Decrease` 使用正的绝对金额；`Other` 使用带方向的净变动。
    pub amount: AccountingAmount,
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub struct DisclosureRoeInput {
    pub report: PublicReportIdentity,
    pub opening_parent_equity: ReportFact,
    pub ordinary_parent_net_income: ReportFact,
    pub adjusted_parent_net_income: ReportFact,
    pub equity_events: Vec<ActualEquityEvent>,
    pub equity_event_history_complete: bool,
    pub same_control_combination: bool,
    pub comparative_period: bool,
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub struct ExactRational {
    pub numerator: BigInt,
    pub denominator: BigInt,
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub struct DisclosureRoeResult {
    pub ordinary_roe: DisclosureRoeValue,
    pub adjusted_roe: DisclosureRoeValue,
    pub weighted_average_parent_equity_cents: ExactRational,
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub enum DisclosureRoeOutcome {
    Available(DisclosureRoeResult),
    Unavailable(DisclosureRoeUnavailable),
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum DisclosureRoeUnavailable {
    NonPositiveAverageEquity,
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub enum DisclosureRoeValue {
    Available(ExactRational),
    Unavailable(DisclosureRoeUnavailable),
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum DisclosureRoeUnsupported {
    SameControlCombination,
    ComparativePeriodSpecialTreatment,
}

#[derive(Clone, Eq, PartialEq, Debug, Error)]
pub enum DisclosureRoeError {
    #[error("报告期间必须从月初至月末且最多覆盖12个月")]
    InvalidReportPeriod,
    #[error("报告尚未结束，不能计算披露 ROE")]
    ReportNotYetEnded,
    #[error("{fact} 的报告期间与报告身份不一致")]
    FactPeriodMismatch { fact: &'static str },
    #[error("{fact} 的披露范围与报告身份不一致")]
    FactScopeMismatch { fact: &'static str },
    #[error("{fact} 的权益归属口径与报告身份不一致")]
    FactAttributionMismatch { fact: &'static str },
    #[error("权益实际生效日期不在报告期间")]
    EventOutsidePeriod,
    #[error("权益事项类别与金额方向不一致")]
    InvalidEventDirection,
    #[error("报告期归母净资产变动事实不完整，不能计算披露 ROE")]
    IncompleteEquityEventHistory,
    #[error("不支持的规则9特殊处理：{0:?}")]
    Unsupported(DisclosureRoeUnsupported),
}

pub fn calculate_disclosure_roe(
    input: &DisclosureRoeInput,
) -> Result<DisclosureRoeOutcome, DisclosureRoeError> {
    let period = input.report.period;
    validate_period(period)?;
    if input.report.published_on < period.end {
        return Err(DisclosureRoeError::ReportNotYetEnded);
    }
    if input.same_control_combination {
        return Err(DisclosureRoeError::Unsupported(
            DisclosureRoeUnsupported::SameControlCombination,
        ));
    }
    if input.comparative_period {
        return Err(DisclosureRoeError::Unsupported(
            DisclosureRoeUnsupported::ComparativePeriodSpecialTreatment,
        ));
    }
    if !input.equity_event_history_complete {
        return Err(DisclosureRoeError::IncompleteEquityEventHistory);
    }
    if input.report.attribution != DisclosureAttributionBasis::AttributableToOrdinaryShareholders {
        return Err(DisclosureRoeError::FactAttributionMismatch { fact: "public report" });
    }
    for (name, fact) in [
        ("opening parent equity", &input.opening_parent_equity),
        ("ordinary parent net income", &input.ordinary_parent_net_income),
        ("adjusted parent net income", &input.adjusted_parent_net_income),
    ] {
        if fact.period != period {
            return Err(DisclosureRoeError::FactPeriodMismatch { fact: name });
        }
        if fact.scope != input.report.scope {
            return Err(DisclosureRoeError::FactScopeMismatch { fact: name });
        }
        if fact.attribution != input.report.attribution {
            return Err(DisclosureRoeError::FactAttributionMismatch { fact: name });
        }
    }

    let months = month_index(period.end) - month_index(period.start) + 1;
    let mut weighted_equity_months = BigInt::from(input.opening_parent_equity.value.cents())
        * BigInt::from(months);
    for event in &input.equity_events {
        if event.period != period {
            return Err(DisclosureRoeError::FactPeriodMismatch {
                fact: "equity event",
            });
        }
        if event.scope != input.report.scope {
            return Err(DisclosureRoeError::FactScopeMismatch {
                fact: "equity event",
            });
        }
        if event.attribution != input.report.attribution {
            return Err(DisclosureRoeError::FactAttributionMismatch {
                fact: "equity event",
            });
        }
        if event.effective_on < period.start || event.effective_on > period.end {
            return Err(DisclosureRoeError::EventOutsidePeriod);
        }
        let cents = event.amount.cents();
        match event.kind {
            EquityEventKind::Increase if cents <= 0 => {
                return Err(DisclosureRoeError::InvalidEventDirection)
            }
            EquityEventKind::Decrease if cents <= 0 => {
                return Err(DisclosureRoeError::InvalidEventDirection)
            }
            EquityEventKind::Other if cents == 0 => {
                return Err(DisclosureRoeError::InvalidEventDirection)
            }
            _ => {}
        }
        let months_after_effective_month = month_index(period.end) - month_index(event.effective_on);
        let signed_cents = match event.kind {
            EquityEventKind::Increase => cents,
            EquityEventKind::Decrease => -cents,
            EquityEventKind::Other => cents,
        };
        weighted_equity_months +=
            BigInt::from(signed_cents) * BigInt::from(months_after_effective_month);
    }

    let ordinary_profit = BigInt::from(input.ordinary_parent_net_income.value.cents());
    let adjusted_profit = BigInt::from(input.adjusted_parent_net_income.value.cents());
    let weighted_average_equity_numerator =
        weighted_equity_months * BigInt::from(2) + &ordinary_profit * BigInt::from(months);
    let weighted_average_equity_denominator = BigInt::from(2 * months);
    let average_equity = ExactRational {
        numerator: weighted_average_equity_numerator.clone(),
        denominator: weighted_average_equity_denominator,
    };
    let ordinary_roe = roe_ratio(
        ordinary_profit,
        months,
        weighted_average_equity_numerator.clone(),
    );
    let adjusted_roe = roe_ratio(adjusted_profit, months, weighted_average_equity_numerator);
    Ok(DisclosureRoeOutcome::Available(DisclosureRoeResult {
        ordinary_roe,
        adjusted_roe,
        weighted_average_parent_equity_cents: average_equity,
    }))
}

fn zero_ratio() -> ExactRational {
    ExactRational {
        numerator: BigInt::from(0),
        denominator: BigInt::from(1),
    }
}

fn roe_ratio(profit: BigInt, months: i32, weighted_equity: BigInt) -> DisclosureRoeValue {
    if weighted_equity <= BigInt::from(0) {
        DisclosureRoeValue::Unavailable(DisclosureRoeUnavailable::NonPositiveAverageEquity)
    } else if profit == BigInt::from(0) {
        DisclosureRoeValue::Available(zero_ratio())
    } else {
        DisclosureRoeValue::Available(ExactRational {
            numerator: profit * BigInt::from(2 * months),
            denominator: weighted_equity,
        })
    }
}

fn validate_period(period: DisclosureReportPeriod) -> Result<(), DisclosureRoeError> {
    if period.start.day() != 1
        || period.end.day() != days_in_month(period.end.year(), period.end.month())
        || period.start > period.end
    {
        return Err(DisclosureRoeError::InvalidReportPeriod);
    }
    let months = month_index(period.end) - month_index(period.start) + 1;
    if !(1..=12).contains(&months) {
        return Err(DisclosureRoeError::InvalidReportPeriod);
    }
    Ok(())
}

fn month_index(date: CivilDate) -> i32 {
    date.year() * 12 + i32::from(date.month())
}

fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 400 == 0 || (year % 4 == 0 && year % 100 != 0) => 29,
        2 => 28,
        _ => 0,
    }
}
