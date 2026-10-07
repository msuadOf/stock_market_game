use super::*;
use crate::accounting::{IncomeTaxPolicy, PostingSide};
use crate::calendar::CivilDate;
use crate::company::{CounterpartyKind, CreditLine, ExternalCounterparty};

fn date(day: u8) -> CivilDate {
    CivilDate::from_ymd(2030, 1, day).unwrap()
}
fn amount(cents: i128) -> AccountingAmount {
    AccountingAmount::from_cents(cents)
}
fn party() -> CounterpartyId {
    CounterpartyId("地产独立守卫对手方".into())
}
fn project() -> ProjectId {
    ProjectId("地产守卫项目".into())
}
fn contract() -> ContractId {
    ContractId("地产守卫合同".into())
}

fn fixture() -> RealEstateBooks {
    let mut books = RealEstateBooks::new(RealEstateConfig {
        chart: real_estate_account_chart(),
        as_of: date(1),
        opening_lines: vec![
            line("1002", PostingSide::Debit, amount(1_000_000)),
            line("4001", PostingSide::Credit, amount(1_000_000)),
        ],
        counterparties: vec![ExternalCounterparty {
            id: party(),
            kind: CounterpartyKind::Lender,
            name: "合成对手方".into(),
        }],
        budget: OperatingBudget::new(
            amount(0),
            vec![CreditLine {
                lender: party(),
                limit: amount(2_000_000),
            }],
        )
        .unwrap(),
        capitalization_policy: CapitalizationPolicy {
            version: 1,
            suspension_min_days: 2,
        },
        income_tax_policy: IncomeTaxPolicy {
            rate_bp: 2500,
            loss_carryforward_years: 5,
        },
        max_projects: 2,
    })
    .unwrap();
    books
        .acquire_land(project(), &party(), 3, amount(7), date(1))
        .unwrap();
    books
        .incur_development(&project(), &party(), amount(4), date(2))
        .unwrap();
    books
}

fn edited(books: &RealEstateBooks, mutate: impl FnOnce(&mut serde_json::Value)) -> RealEstateBooks {
    let mut value = serde_json::to_value(books).unwrap();
    mutate(&mut value);
    serde_json::from_value(value).unwrap()
}

#[test]
fn estate_owner_rejects_project_cost_and_units_drift() {
    let books = fixture();
    books.validate_owner_state().unwrap();
    for (field, value) in [
        ("remaining_cost", serde_json::to_value(amount(12)).unwrap()),
        ("remaining_units", serde_json::json!(4)),
        (
            "development_cost",
            serde_json::to_value(amount(-1)).unwrap(),
        ),
    ] {
        let invalid = edited(&books, |saved| {
            saved["projects"][&project().0][field] = value
        });
        assert!(
            invalid.validate_owner_state().is_err(),
            "未拒绝项目字段{field}漂移"
        );
    }
}

#[test]
fn estate_owner_rejects_presale_amount_flags_and_reference_drift() {
    let mut books = fixture();
    books
        .sign_presale(contract(), &project(), &party(), 2, amount(100), date(2))
        .unwrap();
    books
        .collect_presale(&contract(), amount(50), date(2))
        .unwrap();
    books.validate_owner_state().unwrap();
    for (field, value) in [
        ("collected", serde_json::to_value(amount(101)).unwrap()),
        ("delivered", serde_json::json!(true)),
        ("project", serde_json::json!("不存在的项目")),
        ("buyer", serde_json::json!("不存在的买方")),
        ("units", serde_json::json!(4)),
    ] {
        let invalid = edited(&books, |saved| {
            saved["presales"][&contract().0][field] = value
        });
        assert!(
            invalid.validate_owner_state().is_err(),
            "未拒绝预售字段{field}漂移"
        );
    }
}

#[test]
fn estate_owner_rejects_debt_account_offset_and_invalid_remainders() {
    let mut books = fixture();
    books
        .borrow_project_loan(
            contract(),
            &party(),
            amount(365_000),
            1000,
            date(1),
            date(20),
            Some(project()),
        )
        .unwrap();
    books.accrue_interest(date(3)).unwrap();
    books.validate_owner_state().unwrap();
    for (field, value) in [
        ("debt_account", serde_json::json!("2501")),
        ("outstanding", serde_json::to_value(amount(-1)).unwrap()),
        ("annual_rate_bp", serde_json::json!(-1)),
        ("project", serde_json::json!("不存在的项目")),
        (
            "carried_cap",
            serde_json::to_value(crate::accounting::FractionUnits::from_units(1_825_001)).unwrap(),
        ),
    ] {
        let invalid = edited(&books, |saved| saved["loans"][&contract().0][field] = value);
        assert!(
            invalid.validate_owner_state().is_err(),
            "未拒绝借款字段{field}漂移"
        );
    }
}

#[test]
fn estate_owner_rejects_receivable_balance_and_identity_drift_after_delivery() {
    let mut books = fixture();
    books
        .sign_presale(contract(), &project(), &party(), 3, amount(100), date(2))
        .unwrap();
    books.complete_project(&project(), date(3)).unwrap();
    let delivery = books.deliver(&contract(), date(4)).unwrap();
    books
        .collect_final(&delivery.receivable.clone().unwrap(), amount(30), date(5))
        .unwrap();
    books.validate_owner_state().unwrap();
    let item = delivery.receivable.unwrap().0;
    for (field, value) in [
        ("open_amount", serde_json::to_value(amount(71)).unwrap()),
        ("party", serde_json::json!("不存在的对手方")),
        ("due_on", serde_json::to_value(date(1)).unwrap()),
    ] {
        let invalid = edited(&books, |saved| {
            saved["receivables"]["items"][&item][field] = value
        });
        assert!(
            invalid.validate_owner_state().is_err(),
            "未拒绝尾款字段{field}漂移"
        );
    }
}

#[test]
fn estate_owner_keeps_closed_loans_and_matured_interest_valid() {
    let mut books = fixture();
    books
        .borrow_project_loan(
            contract(),
            &party(),
            amount(365_000),
            1000,
            date(1),
            date(3),
            Some(project()),
        )
        .unwrap();
    books.accrue_interest(date(5)).unwrap();
    books.pay_interest(&contract(), date(5)).unwrap();
    books
        .repay_principal(&contract(), amount(365_000), date(5))
        .unwrap();
    books.validate_owner_state().unwrap();
    let mut input = fixture();
    input.next_event_id = u64::MAX;
    input.validate_owner_state().unwrap();
}

#[test]
fn estate_owner_accepts_consistent_edited_inventory_without_reconstructing_history() {
    let mut books = fixture();
    books
        .books
        .post_batch(vec![crate::accounting::JournalEntry {
            source: crate::accounting::BusinessEventId::new(50),
            date: date(2),
            kind: crate::accounting::BusinessKind::LandAcquisition,
            cash_flow: crate::accounting::CashFlowClass::Operating,
            lines: vec![
                line("1541", PostingSide::Debit, amount(9)),
                line("1002", PostingSide::Credit, amount(9)),
            ],
        }])
        .unwrap();
    books.next_event_id = 51;
    let edited = edited(&books, |saved| {
        saved["projects"][&project().0]["land_cost"] = serde_json::to_value(amount(16)).unwrap();
        saved["projects"][&project().0]["remaining_cost"] =
            serde_json::to_value(amount(20)).unwrap();
    });
    edited.validate_owner_state().unwrap();
}

#[test]
fn estate_project_lifecycle_rejects_backdating_before_development_or_resume_atomically() {
    let mut books = fixture();
    let before = books.clone();
    assert!(books.complete_project(&project(), date(1)).is_err());
    assert_eq!(books, before);
    assert!(books.suspend_development(&project(), date(1)).is_err());
    assert_eq!(books, before);
    assert!(books
        .incur_development(&project(), &party(), amount(1), date(1))
        .is_err());
    assert_eq!(books, before);
    books.suspend_development(&project(), date(3)).unwrap();
    books.resume_development(&project(), date(5)).unwrap();
    let before = books.clone();
    assert!(books.complete_project(&project(), date(4)).is_err());
    assert_eq!(books, before);
    assert!(books.suspend_development(&project(), date(4)).is_err());
    assert_eq!(books, before);
    assert!(books
        .incur_development(&project(), &party(), amount(1), date(4))
        .is_err());
    assert_eq!(books, before);
    books.complete_project(&project(), date(5)).unwrap();
    books.validate_owner_state().unwrap();
}

#[test]
fn estate_owner_rejects_inconsistent_project_timeline_without_replaying_asset_history() {
    let books = fixture();
    for (field, value) in [
        ("completed_on", serde_json::to_value(date(1)).unwrap()),
        ("interrupted_on", serde_json::to_value(date(1)).unwrap()),
        (
            "interruptions",
            serde_json::json!([{ "start": date(1), "end": date(3) }]),
        ),
        (
            "interruptions",
            serde_json::json!([{ "start": date(3), "end": date(5) }, { "start": date(4), "end": date(6) }]),
        ),
        (
            "interruptions",
            serde_json::json!([{ "start": date(5), "end": date(6) }, { "start": date(3), "end": date(4) }]),
        ),
    ] {
        let invalid = edited(&books, |saved| {
            saved["projects"][&project().0][field] = value
        });
        assert!(
            invalid.validate_owner_state().is_err(),
            "未拒绝项目时间字段{field}漂移"
        );
    }
    let mut resumed = fixture();
    resumed.suspend_development(&project(), date(3)).unwrap();
    resumed.resume_development(&project(), date(5)).unwrap();
    for field in ["completed_on", "interrupted_on"] {
        let invalid = edited(&resumed, |saved| {
            saved["projects"][&project().0][field] = serde_json::to_value(date(4)).unwrap()
        });
        assert!(
            invalid.validate_owner_state().is_err(),
            "未拒绝{field}早于复工"
        );
    }
    resumed.suspend_development(&project(), date(5)).unwrap();
    resumed.resume_development(&project(), date(6)).unwrap();
    resumed.complete_project(&project(), date(6)).unwrap();
    resumed.validate_owner_state().unwrap();
}
