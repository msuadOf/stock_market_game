//! 提款/存取/收款守卫：超可支付现金提款 → PaymentFailed（不是负现金，
//! 银行继续运行）；越权提取/收本/收息；未知合同；时间回拨；非法利率；
//! 重复 id；未登记对手方；无已提利息付息。

use super::super::{acct, amt, d, net_debit, yuan};
use super::{bor_cp, dep_cp, dep_id, fresh, loan_id, with_deposit, with_loan};
use engine::company::bank::{BankError, BankProductKind};
use engine::company::{ContractId, CounterpartyId};
#[test]
fn withdrawal_beyond_payable_cash_is_payment_failed_not_negative_cash() {
    // 现金 2000 + 存款 1000 = 3000；贷出 2900 → 现金 100（日终 ECL 3.00
    // 非现金）。提取 1000 元 > 可支付现金 100 元 → PaymentFailed；账套与
    // 子账字节不变（无负现金、无静默补钱）；银行继续运行（小额提取成功）。
    let mut bank = with_deposit();
    bank.issue_loan(
        BankProductKind::TermLoan,
        loan_id(),
        &bor_cp(),
        yuan(2_900),
        600,
        d("2030-01-05"),
        d("2030-07-05"),
    )
    .expect("loan drains cash to 100");
    assert_eq!(net_debit(&bank, acct::CASH), yuan(100));
    let before = bank.clone();
    assert!(matches!(
        bank.withdraw_deposit(&dep_id(), yuan(1_000), d("2030-01-10")),
        Err(BankError::PaymentFailed { .. })
    ));
    assert_eq!(bank, before);
    assert_eq!(net_debit(&bank, acct::CASH), yuan(100));
    assert_eq!(bank.deposit(&dep_id()).unwrap().principal(), yuan(1_000));
    // 银行继续运行：等额可支付提取照常成功。
    bank.withdraw_deposit(&dep_id(), yuan(100), d("2030-01-11"))
        .expect("bank keeps running");
    assert_eq!(net_debit(&bank, acct::CASH), yuan(0));
    assert_eq!(bank.deposit(&dep_id()).unwrap().principal(), yuan(900));
}

#[test]
fn deposit_guards_reject_overdraft_and_bad_inputs() {
    let mut bank = with_deposit();
    // 提取超存款本金 → WithdrawalBeyondPrincipal（不是负负债）。
    let before = bank.clone();
    assert!(matches!(
        bank.withdraw_deposit(&dep_id(), yuan(1_001), d("2030-01-10")),
        Err(BankError::WithdrawalBeyondPrincipal { .. })
    ));
    assert_eq!(bank, before);
    // 未知存款 → UnknownDeposit；非正金额 → NonPositiveAmount。
    assert!(matches!(
        bank.withdraw_deposit(&ContractId("NOPE".to_string()), yuan(1), d("2030-01-10")),
        Err(BankError::UnknownDeposit { .. })
    ));
    assert!(matches!(
        bank.withdraw_deposit(&dep_id(), yuan(0), d("2030-01-10")),
        Err(BankError::NonPositiveAmount { .. })
    ));
    // 重复存款 id / 未知对手方 / 负利率 / 到期不晚于起息。
    assert!(matches!(
        bank.accept_deposit(
            BankProductKind::TermDeposit,
            dep_id(),
            &dep_cp(),
            yuan(1),
            150,
            d("2030-01-11"),
            d("2030-07-11"),
        ),
        Err(BankError::DuplicateContract { .. })
    ));
    assert!(matches!(
        bank.accept_deposit(
            BankProductKind::TermDeposit,
            ContractId("D2".to_string()),
            &CounterpartyId("EXT-NONE".to_string()),
            yuan(1),
            150,
            d("2030-01-11"),
            d("2030-07-11"),
        ),
        Err(BankError::Company(
            engine::company::CompanyError::UnknownCounterparty { .. }
        ))
    ));
    assert!(matches!(
        bank.accept_deposit(
            BankProductKind::TermDeposit,
            ContractId("D2".to_string()),
            &dep_cp(),
            yuan(1),
            -1,
            d("2030-01-11"),
            d("2030-07-11"),
        ),
        Err(BankError::InvalidRateBp { .. })
    ));
    assert!(matches!(
        bank.accept_deposit(
            BankProductKind::TermDeposit,
            ContractId("D2".to_string()),
            &dep_cp(),
            yuan(1),
            150,
            d("2030-07-11"),
            d("2030-01-11"),
        ),
        Err(BankError::MaturityNotAfterStart { .. })
    ));
    assert_eq!(bank, before);
    // 长期存款（>365 天）入 2601；未提利息付息 → NothingAccrued。
    bank.accept_deposit(
        BankProductKind::TermDeposit,
        ContractId("D2".to_string()),
        &dep_cp(),
        yuan(200),
        150,
        d("2030-01-12"),
        d("2031-07-12"),
    )
    .expect("long deposit");
    assert_eq!(net_debit(&bank, acct::LT_DEPOSIT), yuan(-200));
    let before = bank.clone();
    assert!(matches!(
        bank.pay_deposit_interest(&ContractId("D2".to_string()), d("2030-02-01")),
        Err(BankError::NothingAccrued { .. })
    ));
    assert_eq!(bank, before);
}

#[test]
fn loan_guards_reject_overcollections_and_backwards_accrual() {
    let mut bank = with_loan();
    bank.accrue_loan_interest(d("2030-02-01")).expect("accrue"); // 266 分
    let before = bank.clone();
    // 收息超已提 → InterestBeyondAccrued；收本超未偿 → PrincipalBeyondOutstanding。
    assert!(matches!(
        bank.collect_loan_interest(&loan_id(), amt(267), d("2030-02-10")),
        Err(BankError::InterestBeyondAccrued { .. })
    ));
    assert!(matches!(
        bank.collect_loan_principal(&loan_id(), yuan(601), d("2030-02-10")),
        Err(BankError::PrincipalBeyondOutstanding { .. })
    ));
    // 时间回拨计提 → AccrualNotForward；未知贷款重估 → UnknownLoan。
    assert!(matches!(
        bank.accrue_loan_interest(d("2030-01-31")),
        Err(BankError::AccrualNotForward { .. })
    ));
    assert!(matches!(
        bank.assess_credit(
            &ContractId("NOPE".to_string()),
            d("2030-02-01"),
            engine::company::bank::EclStage::Stage2,
            "unknown",
            bank.ecl_policy().lifetime_default.clone(),
        ),
        Err(BankError::UnknownLoan { .. })
    ));
    assert_eq!(bank, before);
    // 贷款发放超现金 → PaymentFailed（银行不能贷出没有的钱）。
    let mut bank2 = fresh();
    assert!(matches!(
        bank2.issue_loan(
            BankProductKind::TermLoan,
            loan_id(),
            &bor_cp(),
            yuan(2_001),
            600,
            d("2030-01-05"),
            d("2030-07-05"),
        ),
        Err(BankError::PaymentFailed { .. })
    ));
    // 重复贷款 id → DuplicateContract。
    let mut bank3 = with_loan();
    assert!(matches!(
        bank3.issue_loan(
            BankProductKind::TermLoan,
            loan_id(),
            &bor_cp(),
            yuan(1),
            600,
            d("2030-01-06"),
            d("2030-07-06"),
        ),
        Err(BankError::DuplicateContract { .. })
    ));
}
