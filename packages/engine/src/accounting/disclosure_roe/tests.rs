use super::*;
use crate::accounting::{AccountingAmount, MemberId, ScopeId};
use crate::calendar::CivilDate;

fn date(year: i32, month: u8, day: u8) -> CivilDate {
    CivilDate::from_ymd(year, month, day).unwrap()
}

fn scope() -> ScopeId {
    ScopeId::Standalone(MemberId("issuer".into()))
}

fn attribution() -> DisclosureAttributionBasis {
    DisclosureAttributionBasis::AttributableToOrdinaryShareholders
}

fn amount(cents: i128) -> AccountingAmount {
    AccountingAmount::from_cents(cents)
}

fn ratio(numerator: i128, denominator: i128) -> ExactRational {
    ExactRational {
        numerator: numerator.into(),
        denominator: denominator.into(),
    }
}

fn ratio_big(numerator: BigInt, denominator: BigInt) -> ExactRational {
    ExactRational {
        numerator,
        denominator,
    }
}

fn report(period: DisclosureReportPeriod) -> DisclosureRoeInput {
    let report_scope = scope();
    DisclosureRoeInput {
        report: PublicReportIdentity {
            period,
            scope: report_scope.clone(),
            attribution: attribution(),
            published_on: date(2030, 12, 31),
        },
        opening_parent_equity: ReportFact {
            period,
            scope: report_scope.clone(),
            attribution: attribution(),
            value: amount(100_000),
        },
        ordinary_parent_net_income: ReportFact {
            period,
            scope: report_scope.clone(),
            attribution: attribution(),
            value: amount(24_000),
        },
        adjusted_parent_net_income: ReportFact {
            period,
            scope: report_scope.clone(),
            attribution: attribution(),
            value: amount(12_000),
        },
        equity_events: vec![],
        equity_event_history_complete: true,
        same_control_combination: false,
        comparative_period: false,
    }
}

fn full_year() -> DisclosureReportPeriod {
    DisclosureReportPeriod {
        start: date(2030, 1, 1),
        end: date(2030, 12, 31),
    }
}

#[test]
fn computes_both_disclosed_numerators_over_np_adjusted_weighted_equity() {
    let DisclosureRoeOutcome::Available(result) = calculate_disclosure_roe(&report(full_year())).unwrap() else {
        panic!("positive average equity is available")
    };

    assert_eq!(result.ordinary_roe, DisclosureRoeValue::Available(ratio(576_000, 2_688_000)));
    assert_eq!(result.adjusted_roe, DisclosureRoeValue::Available(ratio(288_000, 2_688_000)));
    assert_eq!(result.weighted_average_parent_equity_cents, ratio(2_688_000, 24));
}

#[test]
fn weights_equity_change_from_month_after_its_actual_effective_date() {
    let mut input = report(full_year());
    input.equity_events.push(ActualEquityEvent {
        period: full_year(),
        scope: scope(),
        attribution: attribution(),
        effective_on: date(2030, 4, 1),
        kind: EquityEventKind::Increase,
        amount: amount(12_000),
    });

    let DisclosureRoeOutcome::Available(result) = calculate_disclosure_roe(&input).unwrap() else {
        panic!("positive average equity is available")
    };
    assert_eq!(result.weighted_average_parent_equity_cents, ratio(2_880_000, 24));
}

#[test]
fn validates_fact_scope_period_and_future_publication_boundary() {
    let mut input = report(full_year());
    input.adjusted_parent_net_income.period = DisclosureReportPeriod {
        start: date(2030, 1, 1),
        end: date(2030, 6, 30),
    };
    assert!(matches!(calculate_disclosure_roe(&input), Err(DisclosureRoeError::FactPeriodMismatch { .. })));

    let mut input = report(full_year());
    input.adjusted_parent_net_income.scope =
        ScopeId::Standalone(MemberId("other".into()));
    assert!(matches!(calculate_disclosure_roe(&input), Err(DisclosureRoeError::FactScopeMismatch { .. })));

    let mut future_input = report(full_year());
    future_input.report.published_on = date(2030, 6, 30);
    assert!(matches!(calculate_disclosure_roe(&future_input), Err(DisclosureRoeError::ReportNotYetEnded { .. })));
}

#[test]
fn rejects_total_consolidated_income_mixed_with_parent_equity_basis() {
    let mut input = report(full_year());
    input.ordinary_parent_net_income.attribution =
        DisclosureAttributionBasis::ConsolidatedIncludingMinorityInterest;

    assert!(matches!(
        calculate_disclosure_roe(&input),
        Err(DisclosureRoeError::FactAttributionMismatch {
            fact: "ordinary parent net income"
        })
    ));

    let mut input = report(full_year());
    input.report.attribution = DisclosureAttributionBasis::ConsolidatedIncludingMinorityInterest;
    assert!(matches!(
        calculate_disclosure_roe(&input),
        Err(DisclosureRoeError::FactAttributionMismatch { fact: "public report" })
    ));
}

#[test]
fn refuses_same_control_and_comparative_special_rules_explicitly() {
    let mut input = report(full_year());
    input.same_control_combination = true;
    assert_eq!(calculate_disclosure_roe(&input), Err(DisclosureRoeError::Unsupported(DisclosureRoeUnsupported::SameControlCombination)));

    let mut input = report(full_year());
    input.comparative_period = true;
    assert_eq!(calculate_disclosure_roe(&input), Err(DisclosureRoeError::Unsupported(DisclosureRoeUnsupported::ComparativePeriodSpecialTreatment)));
}

#[test]
fn zero_profit_with_nonpositive_average_equity_is_unavailable() {
    let mut input = report(full_year());
    input.ordinary_parent_net_income.value = amount(0);
    input.adjusted_parent_net_income.value = amount(0);
    input.opening_parent_equity.value = amount(-100_000);

    assert_eq!(calculate_disclosure_roe(&input).unwrap(), DisclosureRoeOutcome::Available(DisclosureRoeResult {
        ordinary_roe: DisclosureRoeValue::Unavailable(DisclosureRoeUnavailable::NonPositiveAverageEquity),
        adjusted_roe: DisclosureRoeValue::Unavailable(DisclosureRoeUnavailable::NonPositiveAverageEquity),
        weighted_average_parent_equity_cents: ratio(-2_400_000, 24),
    }));
}

#[test]
fn zero_profit_with_positive_average_equity_has_zero_roe() {
    let mut input = report(full_year());
    input.ordinary_parent_net_income.value = amount(0);
    input.adjusted_parent_net_income.value = amount(0);

    let DisclosureRoeOutcome::Available(result) = calculate_disclosure_roe(&input).unwrap() else {
        panic!("positive average equity is available")
    };
    assert_eq!(result.ordinary_roe, DisclosureRoeValue::Available(ratio(0, 1)));
    assert_eq!(result.adjusted_roe, DisclosureRoeValue::Available(ratio(0, 1)));
    assert_eq!(result.weighted_average_parent_equity_cents, ratio(2_400_000, 24));
}

#[test]
fn positive_profit_with_nonpositive_average_equity_is_unavailable() {
    let mut input = report(full_year());
    input.opening_parent_equity.value = amount(-200_000);

    assert_eq!(calculate_disclosure_roe(&input).unwrap(), DisclosureRoeOutcome::Available(DisclosureRoeResult {
        ordinary_roe: DisclosureRoeValue::Unavailable(DisclosureRoeUnavailable::NonPositiveAverageEquity),
        adjusted_roe: DisclosureRoeValue::Unavailable(DisclosureRoeUnavailable::NonPositiveAverageEquity),
        weighted_average_parent_equity_cents: ratio(-4_512_000, 24),
    }));
}

#[test]
fn a_zero_profit_line_is_unavailable_when_average_equity_is_nonpositive() {
    let mut input = report(full_year());
    input.opening_parent_equity.value = amount(-200_000);
    input.adjusted_parent_net_income.value = amount(0);

    let DisclosureRoeOutcome::Available(result) = calculate_disclosure_roe(&input).unwrap() else {
        panic!("ROE lines have separate availability")
    };
    assert_eq!(result.ordinary_roe, DisclosureRoeValue::Unavailable(DisclosureRoeUnavailable::NonPositiveAverageEquity));
    assert_eq!(result.adjusted_roe, DisclosureRoeValue::Unavailable(DisclosureRoeUnavailable::NonPositiveAverageEquity));
}

#[test]
fn rejects_incomplete_equity_history_instead_of_assuming_no_events() {
    let mut input = report(full_year());
    input.equity_event_history_complete = false;

    assert_eq!(calculate_disclosure_roe(&input), Err(DisclosureRoeError::IncompleteEquityEventHistory));
}

#[test]
fn rejects_invalid_month_aligned_period_and_effective_date_not_in_period() {
    let mut input = report(full_year());
    input.equity_events.push(ActualEquityEvent {
        period: full_year(),
        scope: scope(),
        attribution: attribution(),
        effective_on: date(2030, 12, 31),
        kind: EquityEventKind::Decrease,
        amount: amount(1_000),
    });
    let DisclosureRoeOutcome::Available(result) = calculate_disclosure_roe(&input).unwrap() else {
        panic!("positive average equity is available")
    };
    assert_eq!(result.weighted_average_parent_equity_cents, ratio(2_688_000, 24));

    let mut input = report(full_year());
    input.equity_events.push(ActualEquityEvent {
        period: full_year(), scope: scope(), attribution: attribution(), effective_on: date(2029, 12, 31),
        kind: EquityEventKind::Increase, amount: amount(1_000),
    });
    assert!(matches!(calculate_disclosure_roe(&input), Err(DisclosureRoeError::EventOutsidePeriod { .. })));
}

#[test]
fn three_month_period_uses_its_actual_month_count() {
    let period = DisclosureReportPeriod {
        start: date(2030, 4, 1),
        end: date(2030, 6, 30),
    };
    let mut input = report(period);
    input.report.published_on = period.end;
    input.ordinary_parent_net_income.value = amount(30_000);
    input.adjusted_parent_net_income.value = amount(15_000);

    let DisclosureRoeOutcome::Available(result) = calculate_disclosure_roe(&input).unwrap() else {
        panic!("positive average equity is available")
    };
    assert_eq!(result.weighted_average_parent_equity_cents, ratio(690_000, 6));
    assert_eq!(result.ordinary_roe, DisclosureRoeValue::Available(ratio(180_000, 690_000)));
}

#[test]
fn arithmetic_keeps_i128_sized_facts_exact_without_intermediate_overflow() {
    let mut input = report(full_year());
    input.opening_parent_equity.value = amount(i128::MAX);
    input.ordinary_parent_net_income.value = amount(i128::MAX);
    input.adjusted_parent_net_income.value = amount(i128::MAX);

    let DisclosureRoeOutcome::Available(result) = calculate_disclosure_roe(&input).unwrap() else {
        panic!("positive average equity is available")
    };
    let maximum = BigInt::from(i128::MAX);
    assert_eq!(
        result.weighted_average_parent_equity_cents,
        ratio_big(&maximum * 36, BigInt::from(24)),
    );
    assert_eq!(
        result.ordinary_roe,
        DisclosureRoeValue::Available(ratio_big(&maximum * 24, &maximum * 36)),
    );
}
