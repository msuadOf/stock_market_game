//! OOP 迁移的短行为保护：首错、过账失败、双余数与套数/成本守恒。

use super::*;
use crate::accounting::{FractionUnits, PostingSide};
use crate::calendar::CivilDate;
use crate::company::{CounterpartyKind, CreditLine, ExternalCounterparty};

fn date(day: u8) -> CivilDate {
    CivilDate::from_iso(&format!("2030-01-{day:02}")).expect("合法 Fixture 日期")
}

fn amount(cents: i128) -> AccountingAmount {
    AccountingAmount::from_cents(cents)
}

fn party() -> CounterpartyId {
    CounterpartyId("外部对手方".to_string())
}

fn project() -> ProjectId {
    ProjectId("项目".to_string())
}

fn contract(id: &str) -> ContractId {
    ContractId(id.to_string())
}

fn config() -> RealEstateConfig {
    RealEstateConfig {
        chart: real_estate_account_chart(),
        as_of: date(1),
        opening_lines: vec![
            line(chart::acct::CASH, PostingSide::Debit, amount(1_000_000)),
            line(chart::acct::CAPITAL, PostingSide::Credit, amount(1_000_000)),
        ],
        counterparties: vec![ExternalCounterparty {
            id: party(),
            kind: CounterpartyKind::Lender,
            name: "合成对手方".to_string(),
        }],
        budget: OperatingBudget::new(
            AccountingAmount::ZERO,
            vec![CreditLine {
                lender: party(),
                limit: amount(2_000_000),
            }],
        )
        .expect("合法 Fixture budget"),
        capitalization_policy: CapitalizationPolicy {
            version: 1,
            suspension_min_days: 2,
        },
        max_projects: 2,
        income_tax_policy: crate::accounting::IncomeTaxPolicy {
            rate_bp: 2500,
            loss_carryforward_years: 5,
        },
    }
}

fn developing() -> RealEstateBooks {
    let mut books = RealEstateBooks::new(config()).unwrap();
    books
        .acquire_land(project(), &party(), 3, amount(7), date(1))
        .unwrap();
    books
        .incur_development(&project(), &party(), amount(4), date(1))
        .unwrap();
    books
}

#[test]
fn opening_policy_precedes_first_seeded_account() {
    let mut input = config();
    input.opening_lines = vec![
        line(chart::acct::AR, PostingSide::Debit, amount(1)),
        line(chart::acct::DEV_INVENTORY, PostingSide::Credit, amount(1)),
    ];
    input.max_projects = 0;
    assert!(matches!(
        RealEstateBooks::new(input.clone()),
        Err(RealEstateError::InvalidPolicy { .. })
    ));
    input.max_projects = 1;
    assert_eq!(
        RealEstateBooks::new(input).unwrap_err(),
        RealEstateError::OpeningRealEstateSeeded {
            account: LedgerAccountId(chart::acct::AR.to_string()),
        }
    );
}

#[test]
fn project_guards_preserve_first_error_and_rejected_state() {
    let mut books = RealEstateBooks::new(config()).unwrap();
    books
        .acquire_land(project(), &party(), 3, amount(7), date(1))
        .unwrap();
    books
        .incur_development(&project(), &party(), amount(4), date(2))
        .unwrap();
    books.suspend_development(&project(), date(3)).unwrap();
    let before = books.clone();
    assert_eq!(
        books
            .incur_development(&project(), &party(), amount(0), date(4))
            .unwrap_err(),
        RealEstateError::DevelopmentWhileSuspended { project: project() }
    );
    assert!(matches!(
        books.resume_development(&project(), date(3)),
        Err(RealEstateError::ResumeNotForward { .. })
    ));
    assert!(matches!(
        books.complete_project(&project(), date(4)),
        Err(RealEstateError::CompleteWhileSuspended { .. })
    ));
    assert_eq!(books, before);
    books.resume_development(&project(), date(4)).unwrap();
    let before = books.clone();
    assert!(matches!(
        books.complete_project(&project(), date(1)),
        Err(RealEstateError::OwnerStateInconsistent { .. })
    ));
    assert_eq!(books, before);
    books.complete_project(&project(), date(4)).unwrap();
    let state = books.project(&project()).unwrap();
    assert_eq!(state.dev_started_on(), Some(date(2)));
    assert_eq!(state.completed_on(), Some(date(4)));
    assert!(state.completed_on().unwrap() >= state.dev_started_on().unwrap());
    let before = books.clone();
    assert_eq!(
        books
            .incur_development(&project(), &party(), amount(0), date(4))
            .unwrap_err(),
        RealEstateError::DevelopmentAfterCompletion { project: project() }
    );
    assert_eq!(books, before);
}

#[test]
fn accrual_tracks_mixed_loans_project_total_and_entry_slots() {
    let mut books = developing();
    for (id, assigned) in [("A", Some(project())), ("B", Some(project())), ("C", None)] {
        books
            .borrow_project_loan(
                contract(id),
                &party(),
                amount(365_000),
                1000,
                date(1),
                date(20),
                assigned,
            )
            .unwrap();
    }
    let base = books.next_event_id;
    let items = books.accrue_interest(date(3)).unwrap();
    assert_eq!(
        items
            .iter()
            .map(|item| item.contract.clone())
            .collect::<Vec<_>>(),
        vec![contract("A"), contract("B"), contract("C")]
    );
    assert_eq!(items[0].capitalized_amount, amount(200));
    assert_eq!(items[1].capitalized_amount, amount(200));
    assert_eq!(items[2].capitalized_amount, amount(0));
    assert_eq!(items[2].expensed_amount, amount(200));
    assert_eq!(
        books.project(&project()).unwrap().capitalized_interest(),
        amount(400)
    );
    assert_eq!(books.net_of(chart::acct::FIN_EXP).unwrap(), amount(200));
    assert_eq!(books.next_event_id, base + 3);
}

#[test]
fn planning_and_post_failures_do_not_advance_accrual_state() {
    let mut books = developing();
    books
        .borrow_project_loan(
            contract("A"),
            &party(),
            amount(365_000),
            1000,
            date(1),
            date(20),
            Some(project()),
        )
        .unwrap();
    books
        .borrow_project_loan(
            contract("B"),
            &party(),
            amount(365_000),
            1000,
            date(5),
            date(20),
            None,
        )
        .unwrap();
    let before = books.clone();
    assert!(
        matches!(books.accrue_interest(date(3)), Err(RealEstateError::AccrualNotForward { contract: id, .. }) if id == contract("B"))
    );
    assert_eq!(books, before);
    books.next_event_id = 1; // 与 OpeningBalance source 冲突，强制真实 ledger 提交失败。
    let before = books.clone();
    assert!(matches!(
        books.accrue_interest(date(6)),
        Err(RealEstateError::Accounting(_))
    ));
    assert_eq!(books, before);
}

#[test]
fn zero_amount_accrual_keeps_both_remainder_chains_and_date() {
    let mut books = developing();
    books
        .borrow_project_loan(
            contract("A"),
            &party(),
            amount(1),
            1000,
            date(1),
            date(20),
            Some(project()),
        )
        .unwrap();
    let base = books.next_event_id;
    books.accrue_interest(date(2)).unwrap();
    books.complete_project(&project(), date(2)).unwrap();
    books.accrue_interest(date(3)).unwrap();
    let loan = books.loan(&contract("A")).unwrap();
    assert_eq!(loan.accrued_unpaid(), amount(0));
    assert_eq!(loan.carried_cap(), FractionUnits::from_units(1000));
    assert_eq!(loan.carried_exp(), FractionUnits::from_units(1000));
    assert_eq!(loan.last_accrual_date(), date(3));
    assert_eq!(books.next_event_id, base);
    let before = books.clone();
    assert!(matches!(
        books.pay_interest(&contract("A"), date(3)),
        Err(RealEstateError::NothingAccrued { .. })
    ));
    assert!(matches!(
        books.repay_principal(&contract("A"), amount(0), date(3)),
        Err(RealEstateError::NonPositiveAmount { .. })
    ));
    assert_eq!(books, before);
}

#[test]
fn delivery_last_unit_conserves_cost_and_collection_preconditions() {
    let mut books = developing();
    for (id, units) in [("A", 2), ("B", 1)] {
        books
            .sign_presale(
                contract(id),
                &project(),
                &party(),
                units,
                amount(30),
                date(1),
            )
            .unwrap();
    }
    assert_eq!(books.available_units(&project()).unwrap(), 0);
    books
        .collect_presale(&contract("A"), amount(30), date(2))
        .unwrap();
    books.complete_project(&project(), date(2)).unwrap();
    assert_eq!(
        books
            .deliver(&contract("A"), date(3))
            .unwrap()
            .cost_of_sales,
        amount(7)
    );
    let before = books.clone();
    assert_eq!(
        books
            .collect_presale(&contract("A"), amount(0), date(3))
            .unwrap_err(),
        RealEstateError::CollectionAfterDelivery {
            contract: contract("A")
        }
    );
    assert_eq!(
        books.deliver(&contract("A"), date(3)).unwrap_err(),
        RealEstateError::PresaleAlreadyDelivered {
            contract: contract("A")
        }
    );
    assert_eq!(books, before);
    assert_eq!(
        books
            .deliver(&contract("B"), date(3))
            .unwrap()
            .cost_of_sales,
        amount(4)
    );
    let state = books.project(&project()).unwrap();
    assert_eq!(state.remaining_units(), 0);
    assert_eq!(state.remaining_cost(), amount(0));
    assert_eq!(state.carried_out_cost(), amount(11));
    assert_eq!(books.net_of(chart::acct::DEV_INVENTORY).unwrap(), amount(0));
}

#[test]
fn owner_guards_are_read_only_and_return_existing_context() {
    let mut state = ProjectState::new(amount(7), 3);
    let before = state.clone();
    assert_eq!(
        state.validate_suspend(&project()).unwrap_err(),
        RealEstateError::SuspensionBeforeDevelopment { project: project() }
    );
    assert_eq!(
        state.validate_complete(&project()).unwrap_err(),
        RealEstateError::CompleteBeforeDevelopment { project: project() }
    );
    assert_eq!(
        state.preview_resume(&project(), date(2)).unwrap_err(),
        RealEstateError::NotSuspended { project: project() }
    );
    assert_eq!(state, before);
    state.add_development(amount(4), date(1));
    state.begin_interruption(date(3));
    let before = state.clone();
    assert_eq!(
        state.preview_resume(&project(), date(2)).unwrap_err(),
        RealEstateError::ResumeNotForward {
            project: project(),
            suspended_on: date(3),
            resume_on: date(2),
        }
    );
    assert_eq!(state, before);

    let mut books = developing();
    books
        .sign_presale(contract("A"), &project(), &party(), 1, amount(30), date(1))
        .unwrap();
    books
        .collect_presale(&contract("A"), amount(20), date(2))
        .unwrap();
    let presale = books.presale(&contract("A")).unwrap();
    let before = presale.clone();
    assert_eq!(presale.remaining_payment().unwrap(), amount(10));
    assert_eq!(
        presale
            .validate_collection(&contract("A"), amount(11))
            .unwrap_err(),
        RealEstateError::PresaleBeyondContract {
            contract: contract("A"),
            requested: amount(11),
            remaining: amount(10),
        }
    );
    assert_eq!(*presale, before);

    let loan = ProjectLoanState::new(
        amount(10),
        1000,
        date(1),
        date(2),
        party(),
        None,
        chart::acct::ST_DEBT,
    );
    let before = loan.clone();
    assert_eq!(
        loan.preview_interest_payment(&contract("A")).unwrap_err(),
        RealEstateError::NothingAccrued {
            contract: contract("A")
        }
    );
    assert_eq!(
        loan.validate_repayment(&contract("A"), amount(11))
            .unwrap_err(),
        RealEstateError::PrincipalBeyondOutstanding {
            contract: contract("A"),
            requested: amount(11),
            outstanding: amount(10),
        }
    );
    assert_eq!(loan, before);
}

#[test]
fn segmented_accrual_conserves_capitalized_and_expensed_chains() {
    const PRINCIPAL_CENTS: i128 = 13_753;
    const RATE_BP: i32 = 731;
    const DIVISOR: i128 = 3_650_000;
    let mut segmented = developing();
    segmented
        .borrow_project_loan(
            contract("A"),
            &party(),
            amount(PRINCIPAL_CENTS),
            RATE_BP,
            date(1),
            date(20),
            Some(project()),
        )
        .unwrap();
    let mut whole = segmented.clone();
    whole.complete_project(&project(), date(3)).unwrap();
    whole.accrue_interest(date(6)).unwrap();
    let mut paid_cap = amount(0);
    let mut paid_exp = amount(0);
    for through in [2, 3, 4, 6] {
        if through == 4 {
            segmented.complete_project(&project(), date(3)).unwrap();
        }
        let items = segmented.accrue_interest(date(through)).unwrap();
        paid_cap = paid_cap.add(items[0].capitalized_amount).unwrap();
        paid_exp = paid_exp.add(items[0].expensed_amount).unwrap();
    }
    let loan = segmented.loan(&contract("A")).unwrap();
    assert_eq!(loan, whole.loan(&contract("A")).unwrap());
    assert_eq!(
        paid_cap.cents() * DIVISOR + loan.carried_cap().units(),
        PRINCIPAL_CENTS * i128::from(RATE_BP) * 2
    );
    assert_eq!(
        paid_exp.cents() * DIVISOR + loan.carried_exp().units(),
        PRINCIPAL_CENTS * i128::from(RATE_BP) * 3
    );
    assert_eq!(loan.accrued_unpaid(), paid_cap.add(paid_exp).unwrap());
    assert_eq!(segmented.project(&project()), whole.project(&project()));
}

#[test]
fn loan_apply_overflow_preserves_existing_posted_partial_failure() {
    let mut books = developing();
    books
        .borrow_project_loan(
            contract("A"),
            &party(),
            amount(365_000),
            1000,
            date(1),
            date(20),
            Some(project()),
        )
        .unwrap();
    // serde 既有接受集允许独立子账事实；本用例故意构造应计额已满的边界。
    let mut value = serde_json::to_value(books.loan(&contract("A")).unwrap()).unwrap();
    value["accrued_unpaid"] = serde_json::to_value(AccountingAmount::MAX).unwrap();
    books
        .loans_map_mut()
        .insert(contract("A"), serde_json::from_value(value).unwrap());
    let before = books.clone();
    assert!(matches!(
        books.accrue_interest(date(2)),
        Err(RealEstateError::Accounting(
            AccountingError::AmountOverflow { .. }
        ))
    ));
    assert_eq!(books.next_event_id, before.next_event_id + 1);
    assert_ne!(books.books(), before.books());
    assert_eq!(books.loan(&contract("A")), before.loan(&contract("A")));
    assert_eq!(books.project(&project()), before.project(&project()));
}

#[test]
fn annual_income_tax_excludes_presales_and_repeated_assessment_is_idempotent() {
    let mut books = developing();
    books
        .sign_presale(
            contract("税务预售"),
            &project(),
            &party(),
            3,
            amount(100),
            date(2),
        )
        .unwrap();
    books
        .collect_presale(&contract("税务预售"), amount(100), date(2))
        .unwrap();
    let before = books.clone();
    let first = books.accrue_income_tax(date(3)).unwrap();
    assert_eq!(first.pretax, amount(0));
    assert_eq!(first.current_tax, amount(0));
    assert_eq!(books.books(), before.books());
    books.complete_project(&project(), date(3)).unwrap();
    books.deliver(&contract("税务预售"), date(4)).unwrap();
    let actual = books.accrue_income_tax(date(4)).unwrap();
    assert_eq!(actual.pretax, amount(89));
    assert_eq!(actual.current_tax, amount(22));
    let assessed = books.clone();
    let repeat = books.accrue_income_tax(date(4)).unwrap();
    assert_eq!(repeat.current_tax_delta, amount(0));
    assert_eq!(books, assessed);
    books.validate_income_tax_state().unwrap();
}

#[test]
fn income_tax_payment_failure_preserves_owner_and_does_not_generate_cash() {
    let mut books = developing();
    books
        .sign_presale(
            contract("税款"),
            &project(),
            &party(),
            3,
            amount(8_000_000),
            date(2),
        )
        .unwrap();
    books.complete_project(&project(), date(3)).unwrap();
    let delivery = books.deliver(&contract("税款"), date(4)).unwrap();
    let tax = books.accrue_income_tax(date(4)).unwrap();
    let before = books.clone();
    assert!(matches!(
        books.pay_income_tax(tax.current_tax, date(4)),
        Err(RealEstateError::PaymentFailed { .. })
    ));
    assert_eq!(books, before);
    books
        .collect_final(&delivery.receivable.unwrap(), amount(8_000_000), date(5))
        .unwrap();
    books.pay_income_tax(tax.current_tax, date(5)).unwrap();
    assert_eq!(books.net_of("222104").unwrap(), amount(0));
    books.validate_income_tax_state().unwrap();
}

#[test]
fn income_tax_owner_rejects_missing_assessment_and_invalid_policy() {
    let mut input = config();
    input.income_tax_policy.rate_bp = -1;
    assert!(matches!(
        RealEstateBooks::new(input),
        Err(RealEstateError::IncomeTaxStateInconsistent { .. })
    ));
    let mut books = developing();
    books
        .sign_presale(
            contract("恢复税"),
            &project(),
            &party(),
            3,
            amount(100),
            date(2),
        )
        .unwrap();
    books.complete_project(&project(), date(3)).unwrap();
    books.deliver(&contract("恢复税"), date(4)).unwrap();
    books.accrue_income_tax(date(4)).unwrap();
    let saved = serde_json::to_value(&books).unwrap();
    let restored: RealEstateBooks = serde_json::from_value(saved.clone()).unwrap();
    restored.validate_income_tax_state().unwrap();
    assert_eq!(restored, books);
    let mut invalid = saved.clone();
    invalid["income_tax_position"]["assessments"] = serde_json::json!({});
    let rejected: RealEstateBooks = serde_json::from_value(invalid).unwrap();
    assert!(rejected.validate_income_tax_state().is_err());
    let mut missing = saved;
    missing
        .as_object_mut()
        .unwrap()
        .remove("income_tax_position");
    assert!(serde_json::from_value::<RealEstateBooks>(missing).is_err());
}

#[test]
fn failed_income_tax_post_preserves_assessment_and_event_identity() {
    let mut books = developing();
    books
        .sign_presale(
            contract("失败税"),
            &project(),
            &party(),
            3,
            amount(100),
            date(2),
        )
        .unwrap();
    books.complete_project(&project(), date(3)).unwrap();
    books.deliver(&contract("失败税"), date(4)).unwrap();
    books.next_event_id = 1;
    let before = books.clone();
    assert!(books.accrue_income_tax(date(4)).is_err());
    assert_eq!(books, before);
    books.next_event_id = 100;
    books.accrue_income_tax(date(4)).unwrap();
    books.validate_income_tax_state().unwrap();
}

#[test]
fn income_tax_loss_reassessment_reverses_only_deferred_difference() {
    let mut books = developing();
    books
        .borrow_project_loan(
            contract("费用借款"),
            &party(),
            amount(365_000),
            1000,
            date(1),
            date(20),
            None,
        )
        .unwrap();
    books.accrue_interest(date(3)).unwrap();
    let first = books.accrue_income_tax(date(3)).unwrap();
    assert_eq!(first.pretax, amount(-200));
    assert_eq!(first.current_tax, amount(0));
    assert_eq!(first.deferred_delta, amount(50));
    books
        .sign_presale(
            contract("亏损交付"),
            &project(),
            &party(),
            3,
            amount(100),
            date(3),
        )
        .unwrap();
    books.complete_project(&project(), date(3)).unwrap();
    books.deliver(&contract("亏损交付"), date(4)).unwrap();
    let changed = books.accrue_income_tax(date(4)).unwrap();
    assert_eq!(changed.pretax, amount(-111));
    assert_eq!(changed.current_tax, amount(0));
    assert_eq!(changed.deferred_delta, amount(-22));
    assert_eq!(books.net_of("1811").unwrap(), amount(28));
    let before = books.clone();
    books.accrue_income_tax(date(4)).unwrap();
    assert_eq!(books, before);
    books.validate_income_tax_state().unwrap();
}
