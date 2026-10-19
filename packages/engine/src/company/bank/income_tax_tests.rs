use super::*;
use crate::accounting::{IncomeTaxPolicy, PostingSide};

fn date(value: &str) -> crate::calendar::CivilDate {
    crate::calendar::CivilDate::from_iso(value).unwrap()
}

fn amount(value: i128) -> AccountingAmount {
    AccountingAmount::from_cents(value)
}

fn fixture() -> BankBooks {
    BankBooks::new(BankConfig {
        chart: bank_account_chart(),
        as_of: date("2030-01-01"),
        opening_lines: vec![
            line("1003", PostingSide::Debit, amount(10000)),
            line("4001", PostingSide::Credit, amount(10000)),
        ],
        counterparties: vec![crate::company::ExternalCounterparty {
            id: crate::company::CounterpartyId("税测客户".into()),
            kind: crate::company::CounterpartyKind::Customer,
            name: "虚拟税测客户".into(),
        }],
        ecl_policy: EclPolicy {
            version: 1,
            stage1_default: vec![EclScenario {
                weight_bp: 10000,
                pd_bp: 0,
                lgd_bp: 0,
            }],
            lifetime_default: vec![EclScenario {
                weight_bp: 10000,
                pd_bp: 0,
                lgd_bp: 0,
            }],
        },
        income_tax_policy: IncomeTaxPolicy {
            rate_bp: 2500,
            loss_carryforward_years: 5,
        },
    })
    .unwrap()
}

#[test]
fn bank_income_tax_is_idempotent_and_payment_never_refunds() {
    let mut bank = fixture();
    bank.earn_fee(
        &crate::company::CounterpartyId("税测客户".into()),
        amount(800),
        date("2030-01-02"),
    )
    .unwrap();
    let first = bank.accrue_income_tax(date("2030-12-31")).unwrap();
    assert_eq!(first.current_tax, amount(200));
    let before = bank.clone();
    assert_eq!(
        bank.accrue_income_tax(date("2030-12-31")).unwrap().event,
        None
    );
    assert_eq!(bank, before);
    bank.pay_income_tax(amount(200), date("2031-01-01"))
        .unwrap();
    assert_eq!(
        bank.books()
            .ledger()
            .account_net_debit(&LedgerAccountId("1003".into()))
            .unwrap(),
        amount(10600)
    );
    let before = bank.clone();
    assert!(bank.pay_income_tax(amount(1), date("2031-01-02")).is_err());
    assert_eq!(bank, before);
    bank.validate_income_tax_state().unwrap();
}

#[test]
fn bank_tax_owner_rejects_missing_or_inconsistent_saved_state() {
    let bank = fixture();
    let mut value = serde_json::to_value(&bank).unwrap();
    value.as_object_mut().unwrap().remove("income_tax_position");
    assert!(serde_json::from_value::<BankBooks>(value).is_err());
    let mut value = serde_json::to_value(&bank).unwrap();
    value["income_tax_position"]["initial_deferred_tax_asset"] =
        serde_json::to_value(amount(1)).unwrap();
    let restored: BankBooks = serde_json::from_value(value).unwrap();
    assert!(restored.validate_income_tax_state().is_err());
}

#[test]
fn bank_owner_rejects_raw_journal_subledger_drift() {
    let mut bank = fixture();
    bank.validate_owner_state().unwrap();
    bank.books_mut()
        .post_batch(vec![crate::accounting::JournalEntry {
            source: crate::accounting::BusinessEventId::new(2),
            date: date("2030-01-02"),
            kind: crate::accounting::BusinessKind::CustomerDeposit,
            cash_flow: crate::accounting::CashFlowClass::Operating,
            lines: vec![
                line("1003", PostingSide::Debit, amount(100)),
                line("2011", PostingSide::Credit, amount(100)),
            ],
        }])
        .unwrap();
    assert!(bank.validate_owner_state().is_err());
}

#[test]
fn bank_historical_tax_cascade_restores_without_rewriting_cash_dates() {
    let mut bank = fixture();
    let customer = crate::company::CounterpartyId("税测客户".into());
    for year in [2030, 2031] {
        let end = crate::calendar::CivilDate::from_ymd(year, 12, 31).unwrap();
        bank.earn_fee(&customer, amount(800), end).unwrap();
        bank.accrue_income_tax(end).unwrap();
    }
    bank.pay_income_tax(amount(400), date("2032-01-01"))
        .unwrap();
    let source = crate::accounting::BusinessEventId::new(bank.next_event_id);
    bank.post_with_commit(
        bank.next_event_id + 1,
        vec![crate::accounting::JournalEntry {
            source,
            date: date("2032-01-02"),
            kind: crate::accounting::BusinessKind::InterestAccrual,
            cash_flow: crate::accounting::CashFlowClass::Operating,
            lines: vec![
                line("6411", PostingSide::Debit, amount(1600)),
                line("1003", PostingSide::Credit, amount(1600)),
            ],
        }],
    )
    .unwrap();
    let cash = bank
        .books()
        .ledger()
        .account_net_debit(&LedgerAccountId("1003".into()))
        .unwrap();
    let adjustments = std::collections::BTreeMap::from([(
        source,
        crate::accounting::AccountingPeriod::from_ymd(2030, 12).unwrap(),
    )]);
    let result = bank
        .reassess_income_tax(2030, date("2032-01-02"), &adjustments)
        .unwrap();
    assert_eq!(result.current_tax_delta, amount(-400));
    assert_eq!(
        bank.books()
            .ledger()
            .account_net_debit(&LedgerAccountId("222104".into()))
            .unwrap(),
        amount(400)
    );
    assert_eq!(
        bank.books()
            .ledger()
            .account_net_debit(&LedgerAccountId("1003".into()))
            .unwrap(),
        cash
    );
    assert_eq!(
        bank.books().journal().posted_date(source),
        Some(date("2032-01-02"))
    );
    bank.validate_owner_state().unwrap();
    let mut restored: BankBooks =
        serde_json::from_slice(&serde_json::to_vec(&bank).unwrap()).unwrap();
    restored.validate_owner_state().unwrap();
    let before = restored.clone();
    assert_eq!(
        restored
            .reassess_income_tax(2030, date("2032-01-02"), &adjustments)
            .unwrap()
            .event,
        None
    );
    assert_eq!(restored, before);
}

#[test]
fn bank_tax_payment_insufficient_cash_keeps_all_owner_bytes() {
    let mut bank = fixture();
    let customer = crate::company::CounterpartyId("税测客户".into());
    bank.earn_fee(&customer, amount(800), date("2030-01-02"))
        .unwrap();
    bank.accrue_income_tax(date("2030-03-31")).unwrap();
    bank.issue_loan(
        BankProductKind::TermLoan,
        ContractId("耗用现金贷款".into()),
        &customer,
        amount(10700),
        0,
        date("2030-04-01"),
        date("2030-05-01"),
    )
    .unwrap();
    let before = serde_json::to_vec(&bank).unwrap();
    assert!(matches!(
        bank.pay_income_tax(amount(200), date("2030-04-02")),
        Err(BankError::PaymentFailed { .. })
    ));
    assert_eq!(serde_json::to_vec(&bank).unwrap(), before);
    bank.validate_owner_state().unwrap();
}
