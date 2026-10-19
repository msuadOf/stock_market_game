use crate::accounting::disclosure_roe::{
    ActualEquityEvent, DisclosureAttributionBasis, DisclosureRoeInput, DisclosureReportPeriod,
    EquityEventKind, PublicReportIdentity, ReportFact, calculate_disclosure_roe,
};
use crate::accounting::journal::BusinessKind;
use crate::accounting::ledger::AccountElement;
use crate::accounting::{AccountingAmount, Books, ScopeId};
use crate::calendar::CivilDate;

use super::{
    ReportError, ReportRational, ReportRoe, ReportRoeBasis, ReportRoeEquityEvent,
    ReportRoeEquityEventBreakdown, ReportRoeEquityEventKind, ReportRoeEquityEventListing,
    ReportRoeNonRecurring, ReportRoeNonRecurringItem, ReportRoeUnavailable, ReportRoeValue,
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
) -> Result<ReportRoe, ReportError> {
    let Some(books) = books else {
        // 合并来源没有可用的逐项归母权益事实 → 整体显式不可用（不填零）。
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
    let non_recurring_items = non_recurring_income_items(books, period)?;
    let mut deducted_parent_net_income = ordinary_parent_net_income;
    for item in &non_recurring_items {
        // 归母扣非 = 归母净利润 − Σ非经常项目（税后归母口径）。
        deducted_parent_net_income = deducted_parent_net_income.sub(item.amount)?;
    }
    let result = calculate_disclosure_roe(&DisclosureRoeInput {
        report: PublicReportIdentity {
            period,
            scope: scope.clone(),
            attribution: DisclosureAttributionBasis::AttributableToOrdinaryShareholders,
            // 报表按期末及以后的事实生成；以期末作为可公布下界即通过校验。
            published_on: period.end,
        },
        opening_parent_equity: ReportFact {
            period,
            scope: scope.clone(),
            attribution: DisclosureAttributionBasis::AttributableToOrdinaryShareholders,
            value: opening_parent_equity,
        },
        ordinary_parent_net_income: ReportFact {
            period,
            scope: scope.clone(),
            attribution: DisclosureAttributionBasis::AttributableToOrdinaryShareholders,
            value: ordinary_parent_net_income,
        },
        adjusted_parent_net_income: ReportFact {
            period,
            scope: scope.clone(),
            attribution: DisclosureAttributionBasis::AttributableToOrdinaryShareholders,
            value: deducted_parent_net_income,
        },
        equity_events: equity_events.clone(),
        equity_event_history_complete: true,
        // Simple 单体账套不存在同一控制企业合并业务；该特殊路径由公式层
        // 承载并测试，接线侧不产生同类事件。
        same_control_combinations: Vec::new(),
        comparative_period: false,
    })
    .map_err(|error| ReportError::InternalWindowInconsistent {
        detail: format!("无法从已验证报告事实计算规则9 ROE（普通+扣非）：{error}"),
    })?;
    // 公式层无同一控制事项时普通/扣非分母恒等；adjusted_roe 已按扣非分子
    // 与同一分母计算（研究记录：分母含 NP/2，扣非分子不代入 NP 项）。
    let crate::accounting::disclosure_roe::DisclosureRoeOutcome::Available(result) = result else {
        return Err(ReportError::InternalWindowInconsistent {
            detail: "规则9计算在通过前置校验后不应整体不可用".to_string(),
        });
    };
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
    Ok(ReportRoe {
        basis: ReportRoeBasis::AttributableToOrdinaryShareholders,
        ordinary_roe: map_value(result.ordinary_roe),
        adjusted_roe: map_value(result.adjusted_roe),
        weighted_average_parent_equity_cents: average_equity,
        non_recurring: ReportRoeNonRecurring::Listed {
            items: non_recurring_items,
            parent_net_income_after_deduction: deducted_parent_net_income,
        },
        equity_events: standalone_equity_event_breakdown(&equity_events),
    })
}

/// 单体账套的逐项归属列示：全部权益事件归属母公司；少数股东权益事件在
/// 单体口径下不可表示（显式 unavailable，不填零）。
fn standalone_equity_event_breakdown(
    events: &[ActualEquityEvent],
) -> ReportRoeEquityEventBreakdown {
    ReportRoeEquityEventBreakdown {
        attributable_to_parent: ReportRoeEquityEventListing::Items(
            events.iter().map(listing_event).collect(),
        ),
        minority_interest: ReportRoeEquityEventListing::Unavailable {
            reason: ReportRoeUnavailable::StandaloneHasNoMinorityInterest,
        },
    }
}

fn listing_event(event: &ActualEquityEvent) -> ReportRoeEquityEvent {
    ReportRoeEquityEvent {
        effective_on: event.effective_on.to_iso(),
        kind: match event.kind {
            EquityEventKind::Increase => ReportRoeEquityEventKind::Increase,
            EquityEventKind::Decrease => ReportRoeEquityEventKind::Decrease,
            EquityEventKind::Other => ReportRoeEquityEventKind::Other,
        },
        amount: event.amount,
    }
}

/// 非经常性损益项目分类器：扫描报告窗口内损益类科目发生变动的分录，按
/// 业务类别穷举分类。已登记简化（issues.md 2026-09-11 W3-Task18）：本游戏
/// 报表不产出非经常项目行——当前全部业务均属正常经营损益，列示为空集。
/// 穷举 match 不留通配：新增 BusinessKind 必须在此显式登记证监会口径归属，
/// 未登记前编译失败（不静默归为经常性）。
fn non_recurring_income_items(
    books: &Books,
    period: DisclosureReportPeriod,
) -> Result<Vec<ReportRoeNonRecurringItem>, ReportError> {
    let definitions = books.ledger().chart();
    for entry in books.journal().entries() {
        if entry.date < period.start || entry.date > period.end {
            continue;
        }
        let mut touches_income = false;
        for line in &entry.lines {
            if definitions
                .get(&line.account)
                .is_some_and(|definition| {
                    matches!(
                        definition.element,
                        AccountElement::Revenue | AccountElement::Expense
                    )
                })
            {
                touches_income = true;
            }
        }
        if !touches_income {
            continue;
        }
        // 证监会《信息披露编报规则》口径下本游戏业务均为经常性损益；新业务
        // 若落入非经常口径（如资产处置、政府补助、债务重组），必须在此分支
        // 登记项目 code/name 与税后归母金额，未登记时编译期即暴露。
        match entry.kind {
            BusinessKind::SimpleRevenueAccrual
            | BusinessKind::SimpleFixedExpenseAccrual
            | BusinessKind::SimpleVariableExpenseAccrual
            | BusinessKind::CompanyDividendDeclaration
            | BusinessKind::CompanyDividendPayment
            | BusinessKind::CompanyStockDistributionCredit
            | BusinessKind::CompanyRightsOfferingSettlement
            | BusinessKind::CompanyRepurchaseCancellation
            | BusinessKind::CompanyShareReDenomination
            | BusinessKind::OpeningBalance
            | BusinessKind::LoanDisbursement
            | BusinessKind::LoanRepayment
            | BusinessKind::CashRevenue
            | BusinessKind::CreditSale
            | BusinessKind::ReceivableCollection
            | BusinessKind::CashExpense
            | BusinessKind::InterestAccrual
            | BusinessKind::InterestPayment
            | BusinessKind::TaxAccrual
            | BusinessKind::TaxPayment
            | BusinessKind::Depreciation
            | BusinessKind::CustomerDeposit
            | BusinessKind::CustomerWithdrawal
            | BusinessKind::LoanIssued
            | BusinessKind::LoanPrincipalCollected
            | BusinessKind::LoanInterestAccrued
            | BusinessKind::LoanInterestCollected
            | BusinessKind::DepositInterestAccrued
            | BusinessKind::DepositInterestPaid
            | BusinessKind::FeeAndCommissionEarned
            | BusinessKind::CreditImpairment
            | BusinessKind::LoanWriteOff
            | BusinessKind::WriteOffRecovery
            | BusinessKind::LandAcquisition
            | BusinessKind::DevelopmentCostIncurred
            | BusinessKind::PresaleCollection
            | BusinessKind::RealEstateDelivery
            | BusinessKind::FinalPaymentCollected
            | BusinessKind::BorrowingCostCapitalized
            | BusinessKind::DevelopmentImpairment
            | BusinessKind::InsurancePremiumAccrued
            | BusinessKind::InsurancePremiumCollected
            | BusinessKind::InsuranceServiceRevenue
            | BusinessKind::InsuranceFinance
            | BusinessKind::InsuranceLossComponent
            | BusinessKind::InsuranceClaimIncurred
            | BusinessKind::InsuranceClaimPaid => {}
        }
    }
    Ok(Vec::new())
}

fn actual_equity_events(
    books: &Books,
    period: DisclosureReportPeriod,
    scope: &ScopeId,
) -> Result<Option<Vec<ActualEquityEvent>>, ReportError> {
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
        // Increase/Decrease 使用正的绝对金额；净权益变动为零的行为分录
        //（送转、回购注销、缩股的权益内部结转）在上方 continue 跳过。
        let (kind, amount) = match entry.kind {
            BusinessKind::CompanyDividendDeclaration if delta.is_negative() => {
                (EquityEventKind::Decrease, delta.neg()?)
            }
            BusinessKind::CompanyDividendDeclaration => {
                return Ok(None)
            }
            // 配股／增发结算使归母权益增加（募集资金按面值+溢价计入权益），
            // 属规则9口径的权益增加事项（按实际生效日加权）。
            BusinessKind::CompanyRightsOfferingSettlement if delta.is_positive() => {
                (EquityEventKind::Increase, delta)
            }
            BusinessKind::CompanyRightsOfferingSettlement => {
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
            amount,
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
        non_recurring: ReportRoeNonRecurring::Unavailable { reason },
        equity_events: ReportRoeEquityEventBreakdown {
            attributable_to_parent: ReportRoeEquityEventListing::Unavailable { reason },
            minority_interest: ReportRoeEquityEventListing::Unavailable { reason },
        },
    }
}

pub(super) fn period_dates(
    first: crate::accounting::AccountingPeriod,
    last: crate::accounting::AccountingPeriod,
) -> Result<DisclosureReportPeriod, ReportError> {
    let start = CivilDate::from_ymd(first.year(), first.month(), 1)
        .map_err(|error| ReportError::InternalWindowInconsistent {
            detail: error.to_string(),
        })?;
    let end = CivilDate::from_ymd(
        last.year(),
        last.month(),
        crate::calendar::days_in_month(last.year(), last.month()),
    )
    .map_err(|error| ReportError::InternalWindowInconsistent {
        detail: error.to_string(),
    })?;
    Ok(DisclosureReportPeriod { start, end })
}
