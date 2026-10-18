//! bank 对象归属重构的短行为保护；Fixture 参数为显式游戏假设。

use super::*;
use crate::accounting::{BusinessEventId, PostingSide};
use crate::calendar::CivilDate;
use crate::company::{ContractId, CounterpartyKind, ExternalCounterparty};

fn d(iso: &str) -> CivilDate {
    CivilDate::from_iso(iso).expect("合法 Fixture 日期")
}

fn a(cents: i128) -> AccountingAmount {
    AccountingAmount::from_cents(cents)
}

fn cp() -> CounterpartyId {
    CounterpartyId("客户".to_string())
}

fn id() -> ContractId {
    ContractId("合同".to_string())
}

fn scenarios() -> Vec<EclScenario> {
    vec![EclScenario {
        weight_bp: 10_000,
        pd_bp: 100,
        lgd_bp: 5_000,
    }]
}

fn config() -> BankConfig {
    BankConfig {
        chart: bank_account_chart(),
        as_of: d("2030-01-01"),
        opening_lines: vec![
            line(chart::acct::CASH, PostingSide::Debit, a(200_000)),
            line("4001", PostingSide::Credit, a(200_000)),
        ],
        counterparties: vec![ExternalCounterparty {
            id: cp(),
            kind: CounterpartyKind::Customer,
            name: "Fixture 客户".to_string(),
        }],
        ecl_policy: EclPolicy {
            version: 1,
            stage1_default: scenarios(),
            lifetime_default: scenarios(),
        },
    }
}

fn with_loan() -> BankBooks {
    let mut bank = BankBooks::new(config()).expect("装配银行");
    bank.issue_loan(
        BankProductKind::TermLoan,
        id(),
        &cp(),
        a(60_000),
        600,
        d("2030-01-01"),
        d("2030-07-01"),
    )
    .expect("发放贷款");
    bank
}

fn bytes(bank: &BankBooks) -> Vec<u8> {
    serde_json::to_vec(bank).expect("序列化完整账套")
}

#[test]
fn opening_policy_precedes_first_forbidden_account() {
    let mut cfg = config();
    cfg.opening_lines = vec![
        line(chart::acct::ST_DEPOSIT, PostingSide::Credit, a(1)),
        line(chart::acct::LOAN_PRINCIPAL, PostingSide::Debit, a(1)),
    ];
    cfg.ecl_policy.stage1_default.clear();
    assert_eq!(
        BankBooks::new(cfg.clone()),
        Err(BankError::EclInvalid {
            detail: "stage1_default scenario list is empty".to_string()
        })
    );
    cfg.ecl_policy.stage1_default = scenarios();
    assert_eq!(
        BankBooks::new(cfg),
        Err(BankError::OpeningBankBooksSeeded {
            account: LedgerAccountId(chart::acct::ST_DEPOSIT.to_string())
        })
    );
}

#[test]
fn deposit_partial_withdrawal_keeps_residual_and_maturity_cutoff() {
    let mut bank = BankBooks::new(config()).expect("装配银行");
    bank.accept_deposit(
        BankProductKind::TermDeposit,
        id(),
        &cp(),
        a(100),
        100,
        d("2030-01-01"),
        d("2030-01-03"),
    )
    .expect("存入");
    let first = bank.accrue_deposit_interest(d("2030-01-02")).expect("计提");
    assert_eq!(first[0].amount, a(0));
    assert_eq!(first[0].remaining_carried.units(), 10_000);
    bank.withdraw_deposit(&id(), a(50), d("2030-01-02"))
        .expect("部分提取");
    let second = bank
        .accrue_deposit_interest(d("2030-01-09"))
        .expect("到期截断");
    assert_eq!(second[0].days, 1);
    assert_eq!(second[0].remaining_carried.units(), 15_000);
    assert_eq!(second[0].accrued_through, d("2030-01-03"));
    let before = bytes(&bank);
    assert!(bank
        .accrue_deposit_interest(d("2030-01-10"))
        .expect("停息")
        .is_empty());
    assert_eq!(bytes(&bank), before);
}

#[test]
fn deposit_payment_failure_preserves_residual_flow_and_event() {
    let mut bank = BankBooks::new(config()).expect("装配银行");
    bank.accept_deposit(
        BankProductKind::TermDeposit,
        id(),
        &cp(),
        a(100),
        100,
        d("2030-01-01"),
        d("2030-07-01"),
    )
    .expect("存入");
    bank.accrue_deposit_interest(d("2030-01-02"))
        .expect("零分计提");
    bank.issue_loan(
        BankProductKind::TermLoan,
        ContractId("贷款".to_string()),
        &cp(),
        a(200_100),
        600,
        d("2030-01-02"),
        d("2030-07-02"),
    )
    .expect("贷出全部现金");
    let before = bytes(&bank);
    assert!(matches!(
        bank.withdraw_deposit(&id(), a(50), d("2030-01-03")),
        Err(BankError::PaymentFailed { .. })
    ));
    assert_eq!(bytes(&bank), before);
    assert!(matches!(
        bank.withdraw_deposit(&ContractId("未知".to_string()), a(0), d("2030-01-03")),
        Err(BankError::UnknownDeposit { .. })
    ));
    assert_eq!(bytes(&bank), before);
}

#[test]
fn ecl_sums_scenarios_before_single_half_even_rounding() {
    let weighted = vec![
        EclScenario {
            weight_bp: 5_000,
            pd_bp: 10_000,
            lgd_bp: 5_000
        };
        2
    ];
    for (principal, target) in [(1, 0), (3, 2)] {
        let mut cfg = config();
        cfg.ecl_policy.stage1_default = weighted.clone();
        let mut bank = BankBooks::new(cfg).expect("有效情景");
        bank.issue_loan(
            BankProductKind::TermLoan,
            id(),
            &cp(),
            a(principal),
            0,
            d("2030-01-01"),
            d("2030-07-01"),
        )
        .expect("首日计量");
        assert_eq!(bank.loan(&id()).expect("贷款").allowance(), a(target));
    }
}

#[test]
fn ecl_validation_precedes_unknown_loan_and_overflow_does_not_commit() {
    let mut bank = with_loan();
    assert_eq!(
        bank.assess_credit(
            &ContractId("未知".to_string()),
            d("2030-01-02"),
            EclStage::Stage1,
            "Fixture",
            Vec::new()
        ),
        Err(BankError::EclInvalid {
            detail: "assess scenario list is empty".to_string()
        })
    );
    let mut value = serde_json::to_value(&bank).expect("序列化");
    value["loans"]["合同"]["principal"] = serde_json::to_value(a(i128::MAX)).expect("极值");
    bank = serde_json::from_value(value).expect("沿用 serde 接受集合");
    let before = bytes(&bank);
    assert!(matches!(
        bank.assess_credit(
            &id(),
            d("2030-01-02"),
            EclStage::Stage1,
            "Fixture",
            scenarios()
        ),
        Err(BankError::Accounting(AccountingError::AmountOverflow {
            op: "ecl target",
            ..
        }))
    ));
    assert_eq!(bytes(&bank), before);
}

#[test]
fn restored_initial_policy_is_not_revalidated_during_issue() {
    let bank = BankBooks::new(config()).expect("装配银行");
    let mut value = serde_json::to_value(&bank).expect("序列化");
    value["ecl_policy"]["stage1_default"] = serde_json::json!([]);
    let mut restored: BankBooks = serde_json::from_value(value).expect("原 serde 接受集合");
    assert!(restored.ecl_policy().validate().is_err());
    assert_eq!(
        restored
            .issue_loan(
                BankProductKind::TermLoan,
                id(),
                &cp(),
                a(100),
                0,
                d("2030-01-01"),
                d("2030-07-01")
            )
            .expect("沿用原发行接受集合"),
        vec![BusinessEventId::new(2)]
    );
    assert_eq!(restored.loan(&id()).expect("贷款").allowance(), a(0));
}

#[test]
fn loan_collection_and_recovery_keep_first_errors_and_exact_limits() {
    let mut bank = with_loan();
    let before = bytes(&bank);
    assert!(matches!(
        bank.recover_written_off(&id(), a(0), d("2030-01-02")),
        Err(BankError::LoanNotWrittenOff { .. })
    ));
    assert!(matches!(
        bank.collect_loan_principal(&id(), a(60_001), d("2030-01-02")),
        Err(BankError::PrincipalBeyondOutstanding { .. })
    ));
    assert_eq!(bytes(&bank), before);
    bank.accrue_loan_interest(d("2030-01-02"))
        .expect("计提贷款");
    let interest = bank.loan(&id()).expect("贷款").accrued_receivable();
    bank.collect_loan_interest(&id(), interest, d("2030-01-02"))
        .expect("恰好收清利息");
    bank.assess_credit(
        &id(),
        d("2030-01-02"),
        EclStage::Stage3,
        "Fixture 全损",
        vec![EclScenario {
            weight_bp: 10_000,
            pd_bp: 10_000,
            lgd_bp: 10_000,
        }],
    )
    .expect("足额准备");
    bank.write_off(&id(), d("2030-01-02")).expect("核销");
    let before = bytes(&bank);
    assert!(matches!(
        bank.collect_loan_interest(&id(), a(0), d("2030-01-03")),
        Err(BankError::LoanAlreadyWrittenOff { .. })
    ));
    assert!(matches!(
        bank.collect_loan_principal(&id(), a(0), d("2030-01-03")),
        Err(BankError::LoanAlreadyWrittenOff { .. })
    ));
    assert_eq!(bytes(&bank), before);
    bank.recover_written_off(&id(), a(60_000), d("2030-01-03"))
        .expect("恰好全额回收");
    assert_eq!(bank.loan(&id()).expect("贷款").recoverable(), a(0));
    assert_eq!(bank.loan(&id()).expect("贷款").allowance(), a(60_000));
}

#[test]
fn loan_same_day_and_written_off_accrual_keep_zero_amount_progress() {
    let mut bank = with_loan();
    let before = bytes(&bank);
    assert!(bank
        .accrue_loan_interest(d("2030-01-01"))
        .expect("同日")
        .is_empty());
    assert_eq!(bytes(&bank), before);
    bank.assess_credit(
        &id(),
        d("2030-01-01"),
        EclStage::Stage3,
        "Fixture 全损",
        vec![EclScenario {
            weight_bp: 10_000,
            pd_bp: 10_000,
            lgd_bp: 10_000,
        }],
    )
    .expect("足额准备");
    bank.write_off(&id(), d("2030-01-01")).expect("核销");
    let event = bank.next_event_id;
    let items = bank
        .accrue_loan_interest(d("2030-01-02"))
        .expect("原核销后零分推进");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].amount, a(0));
    assert_eq!(bank.next_event_id, event + 1);
    assert_eq!(
        bank.loan(&id()).expect("贷款").last_accrual_date(),
        d("2030-01-02")
    );
}

#[test]
fn recovery_overflow_retains_existing_post_then_partial_apply_order() {
    let mut bank = with_loan();
    let mut value = serde_json::to_value(&bank).expect("序列化");
    value["loans"]["合同"]["written_off"] = serde_json::json!(true);
    value["loans"]["合同"]["recoverable"] = serde_json::to_value(a(10)).expect("可回收");
    value["loans"]["合同"]["allowance"] = serde_json::to_value(a(i128::MAX)).expect("准备极值");
    bank = serde_json::from_value(value).expect("原 serde 接受集合");
    let before = bank.clone();
    assert!(matches!(
        bank.recover_written_off(&id(), a(1), d("2030-01-02")),
        Err(BankError::Accounting(
            AccountingError::AmountOverflow { .. }
        ))
    ));
    assert_eq!(bank.next_event_id, before.next_event_id + 1);
    assert_ne!(bank.books(), before.books());
    assert_eq!(bank.counterparties(), before.counterparties());
    assert_eq!(bank.loan(&id()).expect("贷款").recoverable(), a(9));
    assert_eq!(bank.loan(&id()).expect("贷款").allowance(), a(i128::MAX));
}
