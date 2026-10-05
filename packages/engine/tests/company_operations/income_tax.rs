use super::fixtures::*;
use engine::accounting::{AccountingAmount, BusinessKind, LedgerAccountId};
use engine::calendar::CivilDate;
use engine::company::operations::{
    CompanyOperations, CompanyOperationsConfig, OperatingCompanyConfig,
};

fn assert_year_end_tax(
    factory: fn(CivilDate) -> OperatingCompanyConfig,
    variant: &str,
    positive_tax: bool,
) {
    let date = d("2030-12-31");
    let company = factory(date.prev().unwrap());
    let id = company.spec.id.clone();
    let mut operations = CompanyOperations::new(
        CompanyOperationsConfig {
            seed: 1,
            shock_params: quiet_params(),
            companies: vec![company],
        },
        date,
    )
    .unwrap();
    let report = operations.advance_civil_day(date).unwrap();
    assert!(report.payment_failures.is_empty());
    let owner = operations.company(&id).unwrap().books();
    let encoded = serde_json::to_value(owner).unwrap();
    let assessment = &encoded[variant]["income_tax_position"]["assessments"]["2030"];
    assert!(assessment.is_object(), "{variant} 未执行真实年末计税");
    let current_tax: AccountingAmount =
        serde_json::from_value(assessment["current_tax"].clone()).unwrap();
    if positive_tax {
        assert!(current_tax.is_positive());
    } else {
        assert!(current_tax.is_zero());
    }
    let tax_entries = owner
        .books()
        .journal()
        .entries()
        .filter(|entry| entry.kind == BusinessKind::TaxAccrual)
        .count();
    let payments: Vec<_> = owner
        .books()
        .journal()
        .entries()
        .filter(|entry| entry.kind == BusinessKind::TaxPayment)
        .collect();
    assert_eq!(payments.len(), usize::from(current_tax.is_positive()));
    if current_tax.is_positive() {
        assert!(tax_entries > 0);
        assert_eq!(payments[0].date, date);
        assert_eq!(
            payments[0]
                .lines
                .iter()
                .find(|line| line.account.0 == "222104")
                .unwrap()
                .amount,
            current_tax,
        );
    }
    assert!(owner
        .books()
        .ledger()
        .account_net_debit(&LedgerAccountId("222104".into()))
        .unwrap()
        .is_zero());
    let before = serde_json::to_value(&operations).unwrap();
    assert!(operations.advance_civil_day(date).is_err());
    assert_eq!(serde_json::to_value(&operations).unwrap(), before);
    let mut restored: CompanyOperations = serde_json::from_value(before).unwrap();
    restored.advance_civil_day(date.next().unwrap()).unwrap();
    assert_eq!(
        restored
            .company(&id)
            .unwrap()
            .books()
            .books()
            .journal()
            .entries()
            .filter(|entry| entry.kind == BusinessKind::TaxAccrual)
            .count(),
        tax_entries,
    );
}

#[test]
fn bank_year_end_uses_real_operating_income_and_pays_once() {
    assert_year_end_tax(bank_c, "Bank", true);
}

#[test]
fn insurance_year_end_uses_earned_service_income_and_pays_once() {
    assert_year_end_tax(insurance_c, "Insurance", true);
}

#[test]
fn real_estate_year_end_records_assessment_without_fabricating_cash_or_tax() {
    assert_year_end_tax(real_estate_c, "RealEstate", false);
}

#[test]
fn industrial_year_end_accrues_tax_once_and_depreciates_the_asset_to_84_months() {
    let date = d("2030-12-31");
    let company = industrial_a(d("2027-12-31"));
    let id = company.spec.id.clone();
    let mut operations = CompanyOperations::generate_history(
        CompanyOperationsConfig {
            seed: 1,
            shock_params: quiet_params(),
            companies: vec![company],
        },
        date,
    )
    .unwrap();
    operations.advance_civil_day(date).unwrap();
    let books = operations.industrial_books(&id).unwrap();
    assert_eq!(
        books
            .books()
            .journal()
            .entries()
            .filter(|entry| entry.date == date && entry.kind == BusinessKind::TaxAccrual)
            .count(),
        1
    );
    assert_eq!(
        books
            .assets()
            .get(&engine::accounting::FixedAssetCode("FA-1".into()))
            .unwrap()
            .remaining_months(),
        84
    );
}

fn assert_restore_rejects_tax_drift(
    factory: fn(CivilDate) -> OperatingCompanyConfig,
    variant: &str,
) {
    let mut company = factory(d("2027-12-31"));
    if let engine::company::operations::FlowParams::Insurance(parameters) = &mut company.flow {
        parameters.daily_groups_base = 1;
        parameters.coverage_days = 2;
        parameters.claim_every_days = 1;
    }
    let id = company.spec.id.0.clone();
    let date = d("2030-01-05");
    let operations = CompanyOperations::new(
        CompanyOperationsConfig { seed: 1, shock_params: quiet_params(), companies: vec![company] },
        date,
    ).unwrap();
    let mut encoded = serde_json::to_value(&operations).unwrap();
    encoded["companies"][&id]["books"][variant]["income_tax_position"]
        ["initial_deferred_tax_asset"] = serde_json::json!("1.00");
    let rejection = serde_json::from_value::<CompanyOperations>(encoded)
        .err()
        .unwrap_or_else(|| panic!("{variant} 接受了与开局不符的税资产"))
        .to_string();
    assert!(
        rejection.contains("税") || rejection.to_ascii_lowercase().contains("tax"),
        "{variant} 没有报告税务状态错误：{rejection}"
    );
}

#[test]
fn bank_operations_restore_rejects_edited_tax_asset_baseline() {
    assert_restore_rejects_tax_drift(bank_c, "Bank");
}

#[test]
fn insurance_operations_restore_rejects_edited_tax_asset_baseline() {
    assert_restore_rejects_tax_drift(insurance_c, "Insurance");
}

#[test]
fn real_estate_operations_restore_rejects_edited_tax_asset_baseline() {
    assert_restore_rejects_tax_drift(real_estate_c, "RealEstate");
}

#[test]
fn bank_tax_cash_failure_keeps_debt_and_does_not_stop_other_company() {
    use engine::company::bank::BankProductKind;
    use engine::company::events::PaymentObligationStatus;
    use engine::company::operations::{FlowParams, IndustryBooks};
    use engine::company::{ContractId, CounterpartyId};

    let date = d("2030-12-31");
    let mut bank = bank_c(date.prev().unwrap());
    let bank_id = bank.spec.id.clone();
    if let IndustryBooks::Bank(owner) = &mut bank.books {
        owner
            .earn_fee(
                &CounterpartyId("EXT-FEE".into()),
                yuan(1_000),
                date.prev().unwrap(),
            )
            .unwrap();
        owner
            .issue_loan(
                BankProductKind::TermLoan,
                ContractId("CASH-USED-BY-REAL-LOAN".into()),
                &CounterpartyId("EXT-BOR".into()),
                yuan(51_000).sub(amt(5)).unwrap(),
                0,
                date.prev().unwrap(),
                d("2031-01-31"),
            )
            .unwrap();
    }
    if let FlowParams::Bank(parameters) = &mut bank.flow {
        parameters.deposit_principal = amt(1);
        parameters.loan_principal = amt(1);
        parameters.daily_fee_income = amt(1);
    }
    let insurance = insurance_c(date.prev().unwrap());
    let insurance_id = insurance.spec.id.clone();
    assert!(bank_id < insurance_id);
    let mut operations = CompanyOperations::new(
        CompanyOperationsConfig {
            seed: 1,
            shock_params: quiet_params(),
            companies: vec![bank, insurance],
        },
        date,
    )
    .unwrap();
    let report = operations.advance_civil_day(date).unwrap();
    let failure = report
        .payment_failures
        .iter()
        .find(|failure| failure.company == bank_id && failure.what == "overdue income tax payment")
        .unwrap();
    assert_eq!(
        failure.obligation_status,
        PaymentObligationStatus::StatutoryPaymentFailure
    );
    let unpaid = failure.amount;
    assert!(unpaid.is_positive());
    let bank_books = operations.company(&bank_id).unwrap().books().books();
    assert!(bank_books.ledger().cash_total().unwrap() < yuan(1));
    assert_eq!(
        bank_books
            .ledger()
            .account_net_debit(&LedgerAccountId("222104".into()))
            .unwrap()
            .neg()
            .unwrap(),
        unpaid
    );
    assert_eq!(
        bank_books
            .journal()
            .entries()
            .filter(|entry| entry.kind == BusinessKind::TaxPayment)
            .count(),
        0
    );
    assert_eq!(
        operations
            .company(&insurance_id)
            .unwrap()
            .books()
            .books()
            .journal()
            .entries()
            .filter(|entry| entry.kind == BusinessKind::TaxPayment)
            .count(),
        1
    );
    let accruals = bank_books
        .journal()
        .entries()
        .filter(|entry| entry.kind == BusinessKind::TaxAccrual)
        .count();
    let mut restored: CompanyOperations =
        serde_json::from_value(serde_json::to_value(&operations).unwrap()).unwrap();
    let next = restored.advance_civil_day(date.next().unwrap()).unwrap();
    assert!(next
        .payment_failures
        .iter()
        .any(|failure| failure.company == bank_id
            && failure.what == "overdue income tax payment"
            && failure.amount == unpaid));
    let bank_books = restored.company(&bank_id).unwrap().books().books();
    assert!(bank_books.ledger().cash_total().unwrap() < yuan(1));
    assert_eq!(
        bank_books
            .journal()
            .entries()
            .filter(|entry| entry.kind == BusinessKind::TaxAccrual)
            .count(),
        accruals
    );
    assert_eq!(
        bank_books
            .journal()
            .entries()
            .filter(|entry| entry.kind == BusinessKind::TaxPayment)
            .count(),
        0
    );
}

#[test]
fn bank_operations_restore_rejects_tax_closing_effective_period_mismatch() {
    let company = bank_c(d("2027-12-31"));
    let id = company.spec.id.0.clone();
    let operations = CompanyOperations::new(
        CompanyOperationsConfig { seed: 1, shock_params: quiet_params(), companies: vec![company] },
        d("2030-01-05"),
    ).unwrap();
    let source = operations
        .company(&engine::company::CompanyId(id.clone()))
        .unwrap()
        .books()
        .books()
        .journal()
        .entries()
        .next()
        .unwrap()
        .source
        .value()
        .to_string();
    let mut encoded = serde_json::to_value(operations).unwrap();
    let mapping = &mut encoded["companies"][&id]["books"]["Bank"]
        ["income_tax_position"]["restatements"];
    assert!(mapping.as_object().unwrap().is_empty());
    mapping[&source] = serde_json::json!("2026-12");
    let error = serde_json::from_value::<CompanyOperations>(encoded)
        .err()
        .expect("税务与结账有效期间映射不一致必须拒绝");
    assert!(
        error
            .to_string()
            .contains("income tax and closing restatement periods differ"),
        "{error}"
    );
}
