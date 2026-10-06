use crate::accounting::disclosure_roe::{
    ActualEquityEvent, DisclosureAttributionBasis, DisclosureReportPeriod, EquityEventKind,
    OrdinaryDisclosureRoeInput, calculate_ordinary_disclosure_roe,
};
use crate::accounting::journal::BusinessKind;
use crate::accounting::ledger::AccountElement;
use crate::accounting::{AccountingAmount, Books, ScopeId};
use crate::calendar::CivilDate;

use super::{
    ReportRational, ReportRoe, ReportRoeBasis, ReportRoeUnavailable, ReportRoeValue,
};

#[cfg(test)]
mod tests;

pub(super) fn calculate_report_roe(
    books: Option<&Books>,
    scope: &ScopeId,
    period: DisclosureReportPeriod,
    opening_parent_equity: AccountingAmount,
    ordinary_parent_net_income: AccountingAmount,
    restated: bool,
) -> Result<ReportRoe, super::ReportError> {
    let Some(books) = books else {
        return Ok(unsupported(
            ReportRoeUnavailable::ConsolidatedAttributionFactsUnavailable,
        ));
    };
    if restated {
        return Ok(unsupported(
            ReportRoeUnavailable::IncompleteEquityEventHistory,
        ));
    }
    let equity_events = match actual_equity_events(books, period, scope) {
        Ok(Some(events)) => events,
        Ok(None) => return Ok(unsupported(ReportRoeUnavailable::UnclassifiedEquityEvent)),
        Err(error) => return Err(error),
    };
    let result = calculate_ordinary_disclosure_roe(&OrdinaryDisclosureRoeInput {
        period,
        scope: scope.clone(),
        attribution: DisclosureAttributionBasis::AttributableToOrdinaryShareholders,
        opening_parent_equity,
        ordinary_parent_net_income,
        equity_events,
        equity_event_history_complete: true,
        same_control_combination: false,
        comparative_period: false,
    })
    .map_err(|error| super::ReportError::InternalWindowInconsistent {
        detail: format!("无法从已验证报告事实计算规则9一般公式 ROE：{error}"),
    })?;
    let average_equity = ReportRoeValue::Available(ReportRational {
        numerator: result
            .weighted_average_parent_equity_cents
            .numerator
            .to_string(),
        denominator: result
            .weighted_average_parent_equity_cents
            .denominator
            .to_string(),
    });
    let adjusted_roe = ReportRoeValue::Unavailable {
        reason: ReportRoeUnavailable::MissingNonRecurringIncomeFacts,
    };
    Ok(ReportRoe {
        basis: ReportRoeBasis::AttributableToOrdinaryShareholders,
        ordinary_roe: map_value(result.ordinary_roe),
        adjusted_roe,
        weighted_average_parent_equity_cents: average_equity,
    })
}

fn actual_equity_events(
    books: &Books,
    period: DisclosureReportPeriod,
    scope: &ScopeId,
) -> Result<Option<Vec<ActualEquityEvent>>, super::ReportError> {
    let definitions = books.ledger().chart();
    let mut events = Vec::new();
    for entry in books.journal().entries() {
        if entry.date < period.start || entry.date > period.end {
            continue;
        }
        let mut delta = AccountingAmount::ZERO;
        for line in &entry.lines {
            if definitions
                .get(&line.account)
                .is_some_and(|definition| definition.element == AccountElement::Equity)
            {
                let signed = match line.side {
                    crate::accounting::PostingSide::Debit => line.amount.neg()?,
                    crate::accounting::PostingSide::Credit => line.amount,
                };
                delta = delta.add(signed)?;
            }
        }
        if !delta.is_positive() && !delta.is_negative() {
            continue;
        }
        let kind = match entry.kind {
            BusinessKind::CompanyDividendDeclaration if delta.is_negative() => {
                EquityEventKind::Decrease
            }
            BusinessKind::CompanyDividendDeclaration => {
                return Ok(None)
            }
            _ => return Ok(None),
        };
        events.push(ActualEquityEvent {
            period,
            scope: scope.clone(),
            attribution: DisclosureAttributionBasis::AttributableToOrdinaryShareholders,
            effective_on: entry.date,
            kind,
            amount: delta.neg()?,
        });
    }
    Ok(Some(events))
}

fn map_value(value: crate::accounting::disclosure_roe::DisclosureRoeValue) -> ReportRoeValue {
    match value {
        crate::accounting::disclosure_roe::DisclosureRoeValue::Available(value) => {
            ReportRoeValue::Available(ReportRational {
                numerator: value.numerator.to_string(),
                denominator: value.denominator.to_string(),
            })
        }
        crate::accounting::disclosure_roe::DisclosureRoeValue::Unavailable(_) => {
            ReportRoeValue::Unavailable {
                reason: ReportRoeUnavailable::NonPositiveAverageEquity,
            }
        }
    }
}

fn unsupported(reason: ReportRoeUnavailable) -> ReportRoe {
    let value = ReportRoeValue::Unavailable { reason };
    ReportRoe {
        basis: ReportRoeBasis::Unsupported { reason },
        ordinary_roe: value.clone(),
        adjusted_roe: value.clone(),
        weighted_average_parent_equity_cents: value,
    }
}

pub(super) fn period_dates(
    first: crate::accounting::AccountingPeriod,
    last: crate::accounting::AccountingPeriod,
) -> Result<DisclosureReportPeriod, super::ReportError> {
    let start = CivilDate::from_ymd(first.year(), first.month(), 1)
        .map_err(|error| super::ReportError::InternalWindowInconsistent {
            detail: error.to_string(),
        })?;
    let end = CivilDate::from_ymd(
        last.year(),
        last.month(),
        crate::calendar::days_in_month(last.year(), last.month()),
    )
    .map_err(|error| super::ReportError::InternalWindowInconsistent {
        detail: error.to_string(),
    })?;
    Ok(DisclosureReportPeriod { start, end })
}
