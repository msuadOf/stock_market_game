use super::fixtures::*;
use engine::company::operations::{CompanyOperations, CompanyOperationsConfig, FlowParams};
use engine::company::CompanyId;

#[test]
fn maturity_redeems_bank_deposit_principal_and_interest() {
    let start = d("2030-01-01");
    let mut company = bank_c(start.prev().unwrap());
    if let FlowParams::Bank(params) = &mut company.flow {
        params.deposit_term_days = 1;
        params.deposit_every_days = 100;
    }
    let mut ops = CompanyOperations::new(
        CompanyOperationsConfig {
            seed: 1,
            shock_params: quiet_params(),
            companies: vec![company],
        },
        start,
    )
    .unwrap();
    ops.advance_civil_day(start).unwrap();
    ops.advance_civil_day(start.next().unwrap()).unwrap();
    let books = ops.bank_books(&CompanyId("C-BANK".into())).unwrap();
    let deposit = books
        .deposit(&engine::company::ContractId("DEP-1".into()))
        .unwrap();
    assert!(deposit.principal().is_zero());
    assert!(deposit.accrued_payable().is_zero());
}

#[test]
fn preexisting_bank_deposit_is_scheduled_and_redeemed_on_first_operating_day() {
    use engine::company::{ContractId, CounterpartyId};
    let date = d("2030-01-01");
    let contract = ContractId("EXISTING-DEPOSIT".into());
    let mut company = bank_c(date.prev().unwrap());
    if let engine::company::operations::IndustryBooks::Bank(books) = &mut company.books {
        books
            .accept_deposit(
                engine::company::bank::BankProductKind::TermDeposit,
                contract.clone(),
                &CounterpartyId("EXT-DEP".into()),
                yuan(1_000),
                150,
                date.prev().unwrap(),
                date,
            )
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
    assert!(ops.scheduler().pending().iter().any(|due| due.due_date == date && matches!(&due.action, engine::company::operations::ScheduledAction::ContractMaturity { reference, .. } if reference == "DEP:EXISTING-DEPOSIT")));
    ops.advance_civil_day(date).unwrap();
    let books = ops.bank_books(&CompanyId("C-BANK".into())).unwrap();
    let deposit = books.deposit(&contract).unwrap();
    assert!(deposit.principal().is_zero());
    assert!(deposit.accrued_payable().is_zero());
    assert_eq!(
        books
            .books()
            .journal()
            .entries()
            .filter(|entry| entry.kind == engine::accounting::BusinessKind::DepositInterestPaid)
            .count(),
        1
    );
}

#[test]
fn year_end_retains_future_operating_obligations_in_scheduler() {
    let date = d("2030-12-31");
    let mut operations = CompanyOperations::new(
        CompanyOperationsConfig {
            seed: 1,
            shock_params: quiet_params(),
            companies: vec![bank_c(date.prev().unwrap())],
        },
        date,
    )
    .unwrap();

    operations.advance_civil_day(date).unwrap();

    assert!(
        !operations.scheduler().pending().is_empty(),
        "年末日结后必须保留未来经营义务"
    );
    assert!(operations
        .scheduler()
        .pending()
        .iter()
        .all(|due| due.due_date > date));
}

#[test]
fn commercial_debt_maturity_pays_real_principal_and_accrued_interest() {
    let date = d("2030-06-30");
    let mut ops = CompanyOperations::new(
        CompanyOperationsConfig {
            seed: 1,
            shock_params: quiet_params(),
            companies: vec![industrial_a(date.prev().unwrap())],
        },
        date,
    )
    .unwrap();
    let report = ops.advance_civil_day(date).unwrap();
    assert!(report.payment_failures.is_empty());
    let books = ops.industrial_books(&CompanyId("C-IND-A".into())).unwrap();
    let loan = books.loans().next().unwrap().1;
    assert!(loan.outstanding().is_zero());
    assert!(loan.accrued_unpaid().is_zero());
}

#[test]
fn unpaid_commercial_debt_is_retained_and_retried_without_cash_injection() {
    let date = d("2030-06-30");
    let mut company = industrial_a(date.prev().unwrap());
    if let engine::company::operations::IndustryBooks::Industrial(books) = &mut company.books {
        books
            .pay_expense(
                engine::company::industrial::ExpenseKind::Admin,
                amt(2_199_500),
                date.prev().unwrap(),
            )
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
    for day in [date, date.next().unwrap()] {
        let report = ops.advance_civil_day(day).unwrap();
        assert!(report.payment_failures.iter().any(|failure| failure
            .what
            .contains("overdue commercial debt principal")
            && failure.amount == amt(200_000)));
        let books = ops.industrial_books(&CompanyId("C-IND-A".into())).unwrap();
        assert_eq!(books.loans().next().unwrap().1.outstanding(), amt(200_000));
        assert!(!books.books().ledger().cash_total().unwrap().is_negative());
    }
}

#[test]
fn project_borrowing_maturity_has_real_operating_payment_caller() {
    use engine::company::real_estate::{
        real_estate_account_chart, CapitalizationPolicy, RealEstateBooks, RealEstateConfig,
    };
    use engine::company::{
        CounterpartyId, CounterpartyKind, CreditLine, ExternalCounterparty, OperatingBudget,
    };
    let date = d("2030-01-01");
    let mut company = real_estate_c(date.prev().unwrap());
    let counterparties = [
        ("EXT-LAND", CounterpartyKind::Supplier),
        ("EXT-CON", CounterpartyKind::Supplier),
        ("EXT-BUYER", CounterpartyKind::Customer),
        ("EXT-BANK", CounterpartyKind::Lender),
    ]
    .into_iter()
    .map(|(id, kind)| ExternalCounterparty {
        id: CounterpartyId(id.into()),
        kind,
        name: id.into(),
    })
    .collect();
    let mut books = RealEstateBooks::new(RealEstateConfig {
        chart: real_estate_account_chart(),
        as_of: date.prev().unwrap(),
        opening_lines: vec![
            cent_line("1002", engine::accounting::PostingSide::Debit, 1_000_000),
            cent_line("4001", engine::accounting::PostingSide::Credit, 1_000_000),
        ],
        counterparties,
        budget: OperatingBudget::new(
            engine::accounting::AccountingAmount::ZERO,
            vec![CreditLine {
                lender: CounterpartyId("EXT-BANK".into()),
                limit: amt(100_000),
            }],
        )
        .unwrap(),
        capitalization_policy: CapitalizationPolicy {
            version: 1,
            suspension_min_days: 90,
        },
        max_projects: 2,
        income_tax_policy: engine::accounting::IncomeTaxPolicy { rate_bp: 2500, loss_carryforward_years: 5 },
    })
    .unwrap();
    let contract = engine::company::ContractId("PROJECT-DEBT".into());
    books
        .borrow_project_loan(
            contract.clone(),
            &CounterpartyId("EXT-BANK".into()),
            amt(100_000),
            365,
            date.prev().unwrap(),
            date.next().unwrap(),
            None,
        )
        .unwrap();
    company.books = engine::company::operations::IndustryBooks::RealEstate(books);
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
    ops.advance_civil_day(date.next().unwrap()).unwrap();
    let loan = ops
        .real_estate_books(&CompanyId("C-RE".into()))
        .unwrap()
        .loan(&contract)
        .unwrap();
    assert!(loan.outstanding().is_zero());
    assert!(loan.accrued_unpaid().is_zero());
}
