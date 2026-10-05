use super::*;
use crate::accounting::MemberId;

fn date(day: u8) -> CivilDate {
    CivilDate::from_ymd(2030, 1, day).unwrap()
}

fn scope() -> ScopeId {
    ScopeId::Standalone(MemberId("issuer".to_string()))
}

fn amount(cents: i128) -> AccountingAmount {
    AccountingAmount::from_cents(cents)
}

fn scoped(scope: &ScopeId, cents: i128) -> ScopedAccountingAmount {
    ScopedAccountingAmount {
        scope: scope.clone(),
        basis: EquityBasis::Total,
        amount: amount(cents),
    }
}

fn scoped_basis(scope: &ScopeId, basis: EquityBasis, cents: i128) -> ScopedAccountingAmount {
    ScopedAccountingAmount {
        scope: scope.clone(),
        basis,
        amount: amount(cents),
    }
}

fn change(scope: &ScopeId, date: CivilDate, delta: i128) -> DatedEquityChange {
    DatedEquityChange {
        scope: scope.clone(),
        basis: EquityBasis::Total,
        date,
        delta: amount(delta),
    }
}

#[test]
fn time_weights_equity_change_from_its_effective_date() {
    let scope = scope();
    let PeriodRoeOutcome::Available(result) = period_time_weighted_equity_roe(
        date(1),
        date(11),
        scoped(&scope, 10_000),
        scoped(&scope, 1_500),
        &[change(&scope, date(6), 10_000)],
    )
    .unwrap() else {
        panic!("valid period is available")
    };

    assert_eq!(
        result.average_equity_cents,
        ExactRatio {
            numerator: 160_000,
            denominator: 10
        }
    );
    assert_eq!(
        result.rate,
        ExactRatio {
            numerator: 15_000,
            denominator: 160_000
        }
    );
    assert_eq!(result.start_exclusive, date(1));
    assert_eq!(result.end_inclusive, date(11));
}

#[test]
fn first_inclusive_day_change_weights_full_period_and_end_date_weights_one_day() {
    let scope = scope();
    let PeriodRoeOutcome::Available(first_day) = period_time_weighted_equity_roe(
        date(1),
        date(11),
        scoped(&scope, 10_000),
        scoped(&scope, 0),
        &[change(&scope, date(2), 10_000)],
    )
    .unwrap() else {
        panic!("valid period is available")
    };
    let PeriodRoeOutcome::Available(last_day) = period_time_weighted_equity_roe(
        date(1),
        date(11),
        scoped(&scope, 10_000),
        scoped(&scope, 0),
        &[change(&scope, date(11), 10_000)],
    )
    .unwrap() else {
        panic!("valid period is available")
    };

    assert_eq!(
        first_day.average_equity_cents,
        ExactRatio {
            numerator: 200_000,
            denominator: 10
        }
    );
    assert_eq!(
        last_day.average_equity_cents,
        ExactRatio {
            numerator: 110_000,
            denominator: 10
        }
    );
}

#[test]
fn zero_or_negative_average_equity_is_unavailable() {
    let scope = scope();
    let result = period_time_weighted_equity_roe(
        date(1),
        date(11),
        scoped(&scope, 0),
        scoped(&scope, 100),
        &[],
    );
    assert_eq!(
        result.unwrap(),
        PeriodRoeOutcome::Unavailable(PeriodRoeUnavailable::NonPositiveAverageEquity)
    );
    assert_eq!(
        period_time_weighted_equity_roe(
            date(1),
            date(11),
            scoped(&scope, -100),
            scoped(&scope, 100),
            &[]
        )
        .unwrap(),
        PeriodRoeOutcome::Unavailable(PeriodRoeUnavailable::NonPositiveAverageEquity)
    );
}

#[test]
fn changes_must_match_scope_and_lie_inside_open_closed_window() {
    let scope = scope();
    let foreign = ScopeId::Standalone(MemberId("other".to_string()));
    let wrong_scope = DatedEquityChange {
        scope: foreign.clone(),
        basis: EquityBasis::Total,
        date: date(5),
        delta: amount(1),
    };
    assert!(matches!(
        period_time_weighted_equity_roe(
            date(1),
            date(11),
            scoped(&scope, 100),
            scoped(&scope, 1),
            &[wrong_scope]
        ),
        Err(PeriodRoeError::ScopeMismatch { .. })
    ));
    let boundary_change = DatedEquityChange {
        scope: scope.clone(),
        basis: EquityBasis::Total,
        date: date(1),
        delta: amount(1),
    };
    assert!(matches!(
        period_time_weighted_equity_roe(
            date(1),
            date(11),
            scoped(&scope, 100),
            scoped(&scope, 1),
            &[boundary_change]
        ),
        Err(PeriodRoeError::ChangeOutsideWindow { .. })
    ));
    assert!(matches!(
        period_time_weighted_equity_roe(
            date(1),
            date(11),
            scoped(&scope, 100),
            scoped(&scope, 1),
            &[change(&scope, date(12), 1)]
        ),
        Err(PeriodRoeError::ChangeOutsideWindow { .. })
    ));
    assert!(matches!(
        period_time_weighted_equity_roe(
            date(1),
            date(11),
            scoped(&scope, 100),
            scoped(&foreign, 1),
            &[]
        ),
        Err(PeriodRoeError::ScopeMismatch { .. })
    ));
    assert!(matches!(
        period_time_weighted_equity_roe(
            date(11),
            date(1),
            scoped(&scope, 100),
            scoped(&scope, 1),
            &[]
        ),
        Err(PeriodRoeError::InvalidWindow { .. })
    ));
}

#[test]
fn scope_and_equity_attribution_basis_must_match_for_every_input() {
    let scope = scope();
    assert!(matches!(
        period_time_weighted_equity_roe(
            date(1),
            date(11),
            scoped(&scope, 100),
            scoped_basis(&scope, EquityBasis::AttributableToParent, 1),
            &[]
        ),
        Err(PeriodRoeError::EquityBasisMismatch { .. })
    ));
    assert!(matches!(
        period_time_weighted_equity_roe(
            date(1),
            date(11),
            scoped(&scope, 100),
            scoped(&scope, 1),
            &[DatedEquityChange {
                scope: scope.clone(),
                basis: EquityBasis::AttributableToParent,
                date: date(5),
                delta: amount(1)
            }]
        ),
        Err(PeriodRoeError::EquityBasisMismatch { .. })
    ));
    assert!(matches!(
        period_time_weighted_equity_roe(
            date(1),
            date(11),
            scoped_basis(&scope, EquityBasis::AttributableToParent, 100),
            scoped(&scope, 1),
            &[]
        ),
        Err(PeriodRoeError::EquityBasisMismatch { .. })
    ));
}

#[test]
fn amount_and_weighted_sum_overflow_are_explicit() {
    let scope = scope();
    let result = period_time_weighted_equity_roe(
        date(1),
        date(11),
        scoped(&scope, i128::MAX),
        scoped(&scope, 0),
        &[],
    );
    assert_eq!(
        result,
        Err(PeriodRoeError::ArithmeticOverflow {
            operation: "weighted equity days sum"
        })
    );
}

#[test]
fn same_day_changes_net_before_checked_equity_range_validation() {
    let scope = scope();
    let changes = [
        change(&scope, date(2), i128::MAX),
        change(&scope, date(2), 1),
        change(&scope, date(2), -1),
    ];
    let PeriodRoeOutcome::Available(result) = period_time_weighted_equity_roe(
        date(1),
        date(2),
        scoped(&scope, 0),
        scoped(&scope, 0),
        &changes,
    )
    .unwrap() else {
        panic!("same-day net equity fits i128")
    };

    assert_eq!(
        result.average_equity_cents,
        ExactRatio {
            numerator: i128::MAX,
            denominator: 1
        }
    );
    let mut reversed_changes = changes;
    reversed_changes.reverse();
    let PeriodRoeOutcome::Available(reversed_result) = period_time_weighted_equity_roe(
        date(1),
        date(2),
        scoped(&scope, 0),
        scoped(&scope, 0),
        &reversed_changes,
    )
    .unwrap() else {
        panic!("same-day net equity fits i128")
    };
    assert_eq!(result, reversed_result);
}
