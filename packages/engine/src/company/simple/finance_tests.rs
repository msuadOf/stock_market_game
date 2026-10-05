use super::*;
use crate::accounting::{CashFlowClass, IncomeTaxPolicy, VatPolicy};

pub(super) fn date(month: u8, day: u8) -> CivilDate {
    CivilDate::from_ymd(2030, month, day).unwrap()
}
pub(super) fn amount(cents: i128) -> AccountingAmount {
    AccountingAmount::from_cents(cents)
}

#[test]
fn synthetic_period_summary_cannot_claim_real_operating_cash_flow() {
    let entry = JournalEntry {
        source: BusinessEventId::new(2),
        date: date(1, 31),
        kind: BusinessKind::SimplePeriodSummary,
        cash_flow: CashFlowClass::Operating,
        lines: vec![
            line("1122", PostingSide::Debit, amount(100)),
            line("6001", PostingSide::Credit, amount(100)),
        ],
    };
    assert!(entry.validate_invariants().is_err());
}
pub(super) fn fixture() -> SimpleFinanceState {
    SimpleFinanceState::create(
        CompanyId("C-TEST".into()),
        CompanyKind::Industrial,
        &SimpleFinanceConfig {
            opening_lines: vec![
                line("1002", PostingSide::Debit, amount(10_000)),
                line("4001", PostingSide::Credit, amount(10_000)),
            ],
            tax_policy: TaxPolicy {
                version: 1,
                vat: VatPolicy {
                    output_rate_bp: 0,
                    input_rate_bp: 0,
                    deductible_share_bp: 10_000,
                },
                income_tax: IncomeTaxPolicy {
                    rate_bp: 2_500,
                    loss_carryforward_years: 5,
                },
            },
            summary_rule: SimpleSummaryRule::ReceivableRevenuePayableExpenses,
        },
        date(1, 1).prev().unwrap(),
    )
    .unwrap()
}

#[test]
fn generated_summary_has_complete_balanced_reports_and_zero_display_cash_flow() {
    let mut state = fixture();
    state
        .apply_month(
            date(1, 31),
            &PeriodAmounts {
                revenue: amount(10_000),
                fixed_expense: amount(3_000),
                variable_expense: amount(4_000),
            },
        )
        .unwrap();
    let period = AccountingPeriod::from_ymd(2030, 1).unwrap();
    let report = state.prepare_report(period, ReportKind::Monthly).unwrap();
    report.validate().unwrap();
    assert_eq!(report.income.cumulative.net_income, amount(2_250));
    assert_eq!(
        state.books().ledger().equity_rolling().unwrap(),
        amount(12_250)
    );
    assert_eq!(state.books().ledger().cash_total().unwrap(), amount(10_000));
    assert_eq!(
        state
            .books()
            .ledger()
            .cash_flow_for_period(period, CashFlowClass::Operating),
        amount(0)
    );
}

#[test]
fn duplicate_month_and_invalid_amount_leave_entire_summary_unchanged() {
    let mut state = fixture();
    let amounts = PeriodAmounts {
        revenue: amount(500),
        fixed_expense: amount(600),
        variable_expense: amount(0),
    };
    state.apply_month(date(1, 31), &amounts).unwrap();
    let before = serde_json::to_vec(&state).unwrap();
    assert!(state.apply_month(date(1, 31), &amounts).is_err());
    assert_eq!(serde_json::to_vec(&state).unwrap(), before);
    assert!(state
        .apply_month(
            date(2, 28),
            &PeriodAmounts {
                revenue: amount(-1),
                ..amounts
            }
        )
        .is_err());
    assert_eq!(serde_json::to_vec(&state).unwrap(), before);
}

#[test]
fn loss_and_year_tax_follow_existing_policy_without_tax_cash_refund() {
    let mut state = fixture();
    for month in 1..=12 {
        let next_month = if month == 12 {
            CivilDate::from_ymd(2031, 1, 1).unwrap()
        } else {
            date(month + 1, 1)
        };
        state
            .apply_month(
                next_month.prev().unwrap(),
                &PeriodAmounts {
                    revenue: amount(0),
                    fixed_expense: amount(100),
                    variable_expense: amount(0),
                },
            )
            .unwrap();
    }
    let report = state
        .prepare_report(
            AccountingPeriod::from_ymd(2030, 12).unwrap(),
            ReportKind::Annual,
        )
        .unwrap();
    report.validate().unwrap();
    assert_eq!(report.income.cumulative.net_income, amount(-900));
    assert_eq!(state.books().ledger().cash_total().unwrap(), amount(10_000));
    let restored: SimpleFinanceState =
        serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
    restored.validate().unwrap();
    assert_eq!(restored, state);
}
