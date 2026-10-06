use super::tests::{amount, date, fixture};
use super::*;

pub(super) fn line(account: &str, side: PostingSide, amount: AccountingAmount) -> JournalLine {
    JournalLine {
        account: LedgerAccountId(account.into()),
        side,
        amount,
    }
}

pub(super) fn bind_capital(state: &mut SimpleFinanceState) {
    state.define_dividend_legal_facts(legal_facts()).unwrap();
}

fn legal_facts() -> DividendLegalFacts {
    DividendLegalFacts {
        registered_capital: amount(10_000),
        source_evidence: "explicit test fixture fact".into(),
    }
}

#[test]
fn registered_capital_requires_one_explicit_immutable_source_fact() {
    let mut state = fixture();
    assert!(matches!(
        state.distributable_profit(),
        Err(SimpleFinanceError::DividendUnsupported(_))
    ));
    let facts = legal_facts();
    state.define_dividend_legal_facts(facts.clone()).unwrap();
    state.define_dividend_legal_facts(facts).unwrap();
    assert!(matches!(
        state.define_dividend_legal_facts(DividendLegalFacts {
            registered_capital: amount(10_001),
            source_evidence: "different source".into(),
        }),
        Err(SimpleFinanceError::DividendLegalFactsConflict(_))
    ));
    let mut saved = serde_json::to_value(state).unwrap();
    saved.as_object_mut().unwrap().remove("legal_facts");
    assert!(serde_json::from_value::<SimpleFinanceState>(saved).is_err());
}

pub(super) fn profitable_annual_fixture() -> SimpleFinanceState {
    let mut state = fixture();
    for month in 1..=12 {
        let end = if month == 12 {
            CivilDate::from_ymd(2031, 1, 1).unwrap().prev().unwrap()
        } else {
            CivilDate::from_ymd(2030, month + 1, 1)
                .unwrap()
                .prev()
                .unwrap()
        };
        state
            .apply_month(
                end,
                &PeriodAmounts {
                    revenue: amount(10_000),
                    fixed_expense: amount(3_000),
                    variable_expense: amount(4_000),
                },
            )
            .unwrap();
    }
    state
}

#[test]
fn dividend_uses_book_profit_and_plan_identity_is_idempotent() {
    let mut state = profitable_annual_fixture();
    bind_capital(&mut state);
    let declaration = DividendDeclaration {
        plan_id: "plan-2030-a".into(),
        approved_on: CivilDate::from_ymd(2031, 1, 1).unwrap(),
        total_gross: amount(1_000),
        registered_capital: amount(10_000),
    };
    let first = state.declare_dividend(declaration.clone()).unwrap();
    let repeated = state.declare_dividend(declaration.clone()).unwrap();
    assert_eq!(first.plan_id, repeated.plan_id);
    assert_eq!(first.amount, repeated.amount);
    assert!(repeated.already_declared);
    let second = state
        .declare_dividend(DividendDeclaration {
            plan_id: "plan-2030-b".into(),
            approved_on: declaration.approved_on,
            total_gross: amount(1_000),
            registered_capital: amount(10_000),
        })
        .unwrap();
    assert_eq!(second.statutory_reserve, AccountingAmount::ZERO);
    let before = state.books().journal().entries().count();
    let mut conflict = declaration;
    conflict.total_gross = amount(1_001);
    assert!(matches!(
        state.declare_dividend(conflict),
        Err(SimpleFinanceError::DividendPlanConflict { .. })
    ));
    assert_eq!(state.books().journal().entries().count(), before);
}

#[test]
fn zero_reserve_plan_rejects_a_spurious_basis_year() {
    let mut state = profitable_annual_fixture();
    bind_capital(&mut state);
    let declaration = DividendDeclaration {
        plan_id: "reserve-present".into(),
        approved_on: CivilDate::from_ymd(2031, 1, 1).unwrap(),
        total_gross: amount(1_000),
        registered_capital: amount(10_000),
    };
    state.declare_dividend(declaration.clone()).unwrap();
    state
        .declare_dividend(DividendDeclaration {
            plan_id: "reserve-zero".into(),
            approved_on: declaration.approved_on,
            total_gross: amount(1_000),
            registered_capital: amount(10_000),
        })
        .unwrap();
    let plan = state.dividends.get_mut("reserve-zero").unwrap();
    assert_eq!(plan.reserve, AccountingAmount::ZERO);
    plan.reserve_basis_year = RequiredOption(Some(2030));
    assert!(state.validate().is_err());
}

#[test]
fn declared_dividend_payment_does_not_consume_or_claim_simple_cash() {
    let mut state = profitable_annual_fixture();
    bind_capital(&mut state);
    let cash_before = state.books().ledger().cash_total().unwrap();
    state
        .declare_dividend(DividendDeclaration {
            plan_id: "plan-pay".into(),
            approved_on: CivilDate::from_ymd(2031, 1, 1).unwrap(),
            total_gross: amount(1_000),
            registered_capital: amount(10_000),
        })
        .unwrap();
    let payment_date = CivilDate::from_ymd(2031, 1, 2).unwrap();
    let receipt = state
        .pay_dividend("plan-pay", "payment-1", payment_date, amount(400))
        .unwrap();
    assert_eq!(receipt.amount, amount(400));
    assert_eq!(state.books().ledger().cash_total().unwrap(), cash_before);
    assert!(
        state
            .pay_dividend("plan-pay", "payment-1", payment_date, amount(400))
            .unwrap()
            .already_paid
    );
    state
        .pay_dividend(
            "plan-pay",
            "payment-2",
            CivilDate::from_ymd(2031, 1, 3).unwrap(),
            amount(600),
        )
        .unwrap();
    state
        .apply_month(
            CivilDate::from_ymd(2031, 2, 1).unwrap().prev().unwrap(),
            &PeriodAmounts {
                revenue: amount(0),
                fixed_expense: amount(0),
                variable_expense: amount(0),
            },
        )
        .unwrap();
    let equity = state
        .prepare_report(
            AccountingPeriod::from_ymd(2031, 1).unwrap(),
            ReportKind::Monthly,
        )
        .unwrap()
        .equity
        .clone();
    assert_eq!(equity.distributions, amount(1_000));
    assert_eq!(equity.capital_contributions, AccountingAmount::ZERO);
}

#[test]
fn statutory_reserve_uses_tax_after_profit_remaining_after_opening_loss() {
    let baseline = tests::fixture();
    let mut config = baseline.config.clone();
    config.opening_lines = vec![
        line("1002", PostingSide::Debit, amount(9_500)),
        line("4001", PostingSide::Credit, amount(10_000)),
        line("4103", PostingSide::Debit, amount(500)),
    ];
    let mut state = SimpleFinanceState::create(
        baseline.company.clone(),
        baseline.kind,
        &config,
        date(1, 1).prev().unwrap(),
    )
    .unwrap();
    for month in 1..=12 {
        let end = if month == 12 {
            CivilDate::from_ymd(2031, 1, 1).unwrap().prev().unwrap()
        } else {
            CivilDate::from_ymd(2030, month + 1, 1)
                .unwrap()
                .prev()
                .unwrap()
        };
        state
            .apply_month(
                end,
                &PeriodAmounts {
                    revenue: if month == 1 { amount(2_000) } else { amount(0) },
                    fixed_expense: amount(0),
                    variable_expense: amount(0),
                },
            )
            .unwrap();
    }
    bind_capital(&mut state);
    let profit = state.distributable_profit().unwrap();
    assert_eq!(profit.reserve_basis_year, Some(2030));
    assert_eq!(profit.statutory_reserve, amount(100));
    state
        .declare_dividend(DividendDeclaration {
            plan_id: "reserve-line".into(),
            approved_on: CivilDate::from_ymd(2031, 1, 1).unwrap(),
            total_gross: amount(100),
            registered_capital: amount(10_000),
        })
        .unwrap();
    let mut saved = serde_json::to_value(&state).unwrap();
    saved["dividends"]["reserve-line"]
        .as_object_mut()
        .unwrap()
        .remove("reserve_basis_year");
    assert!(serde_json::from_value::<SimpleFinanceState>(saved).is_err());
    state
        .pay_dividend(
            "reserve-line",
            "payment",
            CivilDate::from_ymd(2031, 1, 2).unwrap(),
            amount(100),
        )
        .unwrap();
    state
        .apply_month(
            CivilDate::from_ymd(2031, 2, 1).unwrap().prev().unwrap(),
            &PeriodAmounts {
                revenue: amount(0),
                fixed_expense: amount(0),
                variable_expense: amount(0),
            },
        )
        .unwrap();
    let report = state
        .prepare_report(
            AccountingPeriod::from_ymd(2031, 1).unwrap(),
            ReportKind::Monthly,
        )
        .unwrap();
    report.validate().unwrap();
    assert!(report.balance_sheet.equity_lines.contains(&(
        crate::accounting::reports::BsLine::StatutoryReserve,
        amount(100)
    )));
    assert!(report.balance_sheet.equity_lines.contains(&(
        crate::accounting::reports::BsLine::RetainedEarnings,
        amount(800)
    )));
}
