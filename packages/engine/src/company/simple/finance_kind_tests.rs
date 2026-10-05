use super::tests::{amount, date, fixture};
use super::*;

#[test]
fn every_company_kind_has_balanced_summary_reports_without_fake_industry_business() {
    for (kind, version, cash) in [
        (CompanyKind::Industrial, 2, "1002"),
        (CompanyKind::Bank, 3, "1003"),
        (CompanyKind::Insurance, 4, "1002"),
        (CompanyKind::RealEstate, 5, "1002"),
    ] {
        let mut config = fixture().config().clone();
        config.opening_lines[0].account = LedgerAccountId(cash.into());
        let mut state = SimpleFinanceState::create(
            CompanyId(format!("summary-{kind:?}")),
            kind,
            &config,
            CivilDate::from_ymd(2029, 12, 31).unwrap(),
        )
        .unwrap();
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
        state.validate().unwrap();
        assert_eq!(state.kind(), kind);
        assert_eq!(state.books().ledger().chart().version(), version);
        let report = state
            .report(AccountingPeriod::of_date(date(1, 31)), ReportKind::Monthly)
            .unwrap();
        report.validate().unwrap();
        assert_eq!(report.income.cumulative.net_income, amount(2_250));
        assert_eq!(state.books().ledger().cash_total().unwrap(), amount(10_000));
        for (code, expected) in [
            ("simple_receivable", 10_000),
            ("simple_payable", -7_000),
            ("simple_fixed_expense", 3_000),
            ("simple_variable_expense", 4_000),
        ] {
            assert_eq!(
                state
                    .books()
                    .ledger()
                    .account_net_debit(&LedgerAccountId(code.into()))
                    .unwrap(),
                amount(expected)
            );
        }
        for code in [
            "1131", "1301", "2011", "2601", "2501", "2502", "6011", "6411", "6051", "6451",
        ] {
            assert!(!report.notes.items.iter().any(|item| item.code == code));
        }
        for (code, target) in [
            (
                "simple_revenue",
                crate::accounting::reports::NoteTarget::Income(
                    crate::accounting::reports::IncomeLine::OperatingRevenue,
                ),
            ),
            (
                "simple_fixed_expense",
                crate::accounting::reports::NoteTarget::Income(
                    crate::accounting::reports::IncomeLine::AdministrativeExpense,
                ),
            ),
            (
                "simple_variable_expense",
                crate::accounting::reports::NoteTarget::Income(
                    crate::accounting::reports::IncomeLine::OperatingCost,
                ),
            ),
        ] {
            assert_eq!(
                report
                    .notes
                    .items
                    .iter()
                    .find(|item| item.code == code)
                    .unwrap()
                    .target,
                target
            );
        }
    }
}

#[test]
fn finance_kind_is_required_and_must_match_its_chart_after_restore() {
    let mut saved = serde_json::to_value(fixture()).unwrap();
    saved.as_object_mut().unwrap().remove("kind");
    assert!(serde_json::from_value::<SimpleFinanceState>(saved).is_err());
    let mut saved = serde_json::to_value(fixture()).unwrap();
    saved["kind"] = serde_json::json!("Bank");
    let corrupt: SimpleFinanceState = serde_json::from_value(saved).unwrap();
    assert!(corrupt.validate().is_err());
}
