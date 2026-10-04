use super::fixtures::*;
use engine::company::operations::{CompanyOperations, CompanyOperationsConfig};
use engine::company::CompanyId;

#[test]
fn month_end_depreciates_real_operating_assets() {
    let date = d("2030-01-31");
    let zero_asset = engine::accounting::FixedAssetCode("ZERO-DEPRECIATION".into());
    let mut company = industrial_a(date.prev().unwrap());
    if let engine::company::operations::IndustryBooks::Industrial(books) = &mut company.books {
        books
            .acquire_asset(zero_asset.clone(), amt(1), amt(1), 2, date.prev().unwrap())
            .unwrap();
    }
    let mut ops = CompanyOperations::new(
        CompanyOperationsConfig {
            seed: 1,
            shock_params: quiet_params(),
            companies: vec![company],
        },
        date,
    )
    .unwrap();
    ops.advance_civil_day(date).unwrap();
    let assets = ops
        .industrial_books(&CompanyId("C-IND-A".into()))
        .unwrap()
        .assets();
    assert_eq!(
        assets
            .get(&engine::accounting::FixedAssetCode("FA-1".into()))
            .unwrap()
            .remaining_months(),
        119
    );
    assert_eq!(assets.get(&zero_asset).unwrap().remaining_months(), 1);
    assert!(ops.advance_civil_day(date).is_err());
}

#[test]
fn year_end_accrues_and_pays_income_tax_exactly_once_in_production() {
    let date = d("2030-12-31");
    let mut company = industrial_b(date.prev().unwrap());
    if let engine::company::operations::FlowParams::Industrial(params) = &mut company.flow {
        params.unit_price_excl_vat = yuan(100);
    }
    let mut ops = CompanyOperations::new(
        CompanyOperationsConfig {
            seed: 1,
            shock_params: quiet_params(),
            companies: vec![company],
        },
        date,
    )
    .unwrap();
    ops.advance_civil_day(date).unwrap();
    let books = ops
        .industrial_books(&CompanyId("C-IND-B".into()))
        .unwrap()
        .books();
    assert!(books
        .ledger()
        .account_net_debit(&engine::accounting::LedgerAccountId("222104".into()))
        .unwrap()
        .is_zero());
    assert_eq!(
        books
            .journal()
            .entries()
            .filter(|entry| entry.kind == engine::accounting::BusinessKind::TaxAccrual)
            .count(),
        1
    );
    assert_eq!(
        books
            .journal()
            .entries()
            .filter(|entry| entry.kind == engine::accounting::BusinessKind::TaxPayment)
            .count(),
        1
    );
    assert!(ops.advance_civil_day(date).is_err());
}

#[test]
fn unpaid_tax_survives_year_rollover_without_repeated_accrual() {
    let date = d("2030-12-31");
    let mut company = industrial_b(date.prev().unwrap());
    if let engine::company::operations::IndustryBooks::Industrial(books) = &mut company.books {
        books
            .pay_expense(
                engine::company::industrial::ExpenseKind::Admin,
                amt(260_000),
                date.prev().unwrap(),
            )
            .unwrap();
    }
    if let engine::company::operations::FlowParams::Industrial(params) = &mut company.flow {
        params.unit_price_excl_vat = yuan(10_000);
    }
    let mut ops = CompanyOperations::new(
        CompanyOperationsConfig {
            seed: 1,
            shock_params: quiet_params(),
            companies: vec![company],
        },
        date,
    )
    .unwrap();
    let report = ops.advance_civil_day(date).unwrap();
    let failed_tax = report
        .payment_failures
        .iter()
        .find(|failure| failure.what == "overdue income tax payment")
        .unwrap()
        .amount;
    assert!(failed_tax.is_positive());
    let mut restored: CompanyOperations =
        serde_json::from_slice(&serde_json::to_vec(&ops).unwrap()).unwrap();
    let next = restored.advance_civil_day(date.next().unwrap()).unwrap();
    assert!(next.payment_failures.iter().any(|failure| failure.what
        == "overdue income tax payment"
        && failure.amount == failed_tax));
    let books = restored
        .industrial_books(&CompanyId("C-IND-B".into()))
        .unwrap()
        .books();
    assert_eq!(
        books
            .journal()
            .entries()
            .filter(|entry| entry.kind == engine::accounting::BusinessKind::TaxAccrual)
            .count(),
        1
    );
    assert!(books.ledger().cash_total().unwrap().is_zero());
}
