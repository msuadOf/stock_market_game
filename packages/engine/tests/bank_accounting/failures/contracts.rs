//! 未支持合同显式拒绝 + 开局子账种子守卫：结构化衍生品（CAS 22 §23–§26 +
//! 解释第20号第一部分）、FVTPL/FVOCI 类投资一律 `UnsupportedContract`
//! （docs/company-accounting.md §6——不冒充已实现、不静默 fallback）；
//! 开局行触碰银行子账科目 → 构造期拒绝（经营前史归任务 14）。

use super::super::{acct, base_config, cent_line, d, yuan};
use super::{bor_cp, dep_cp, loan_id, with_deposit};
use engine::accounting::PostingSide;
use engine::company::bank::{BankBooks, BankConfig, BankError, BankProductKind};
use engine::company::ContractId;

#[test]
fn structured_and_unsupported_contracts_are_explicitly_rejected() {
    let mut bank = with_deposit();
    let before = bank.clone();
    match bank.accept_deposit(
        BankProductKind::StructuredDerivative,
        ContractId("SX-1".to_string()),
        &dep_cp(),
        yuan(100),
        300,
        d("2030-01-05"),
        d("2031-01-05"),
    ) {
        Err(BankError::UnsupportedContract { kind, .. }) => {
            assert_eq!(kind, BankProductKind::StructuredDerivative);
        }
        other => panic!("expected UnsupportedContract, got {other:?}"),
    }
    assert_eq!(bank, before);
    assert!(matches!(
        bank.issue_loan(
            BankProductKind::FvtplInstrument,
            loan_id(),
            &bor_cp(),
            yuan(100),
            600,
            d("2030-01-05"),
            d("2030-07-05"),
        ),
        Err(BankError::UnsupportedContract { .. })
    ));
    assert!(matches!(
        bank.issue_loan(
            BankProductKind::FvociInstrument,
            loan_id(),
            &bor_cp(),
            yuan(100),
            600,
            d("2030-01-05"),
            d("2030-07-05"),
        ),
        Err(BankError::UnsupportedContract { .. })
    ));
    assert_eq!(bank, before);
}

#[test]
fn opening_lines_touching_bank_subledgers_are_rejected() {
    // 开局不得给银行子账科目（贷款/存款/准备/损益）种子——经营前史由任务 14
    // 用同一处理器生成；开局只允许现金 + 权益（显式诚实边界）。
    let mut cfg: BankConfig = base_config();
    cfg.opening_lines = vec![
        cent_line(acct::CASH, PostingSide::Debit, 300_000),
        cent_line(acct::ST_DEPOSIT, PostingSide::Credit, 100_000),
        cent_line(acct::CAPITAL, PostingSide::Credit, 200_000),
    ];
    assert!(matches!(
        BankBooks::new(cfg),
        Err(BankError::OpeningBankBooksSeeded { .. })
    ));
    // 非法开局行本身（不平衡）→ 透传 Accounting 错误。
    let mut cfg = base_config();
    cfg.opening_lines = vec![
        cent_line(acct::CASH, PostingSide::Debit, 200_001),
        cent_line(acct::CAPITAL, PostingSide::Credit, 200_000),
    ];
    assert!(matches!(BankBooks::new(cfg), Err(BankError::Accounting(_))));
}
