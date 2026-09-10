//! 过账守卫：未知科目、非现金触碰现金、负现金（对照合法负权益）、
//! 已封期间、非正金额行、缺借贷边。全部断言完整状态不变。

use super::opened_books;
use crate::{acct, entry, yuan};
use engine::accounting::{
    AccountingAmount, AccountingError, AccountingPeriod, BusinessEventId, BusinessKind,
    CashFlowClass, JournalEntry, JournalLine, LedgerAccountId, PeriodStatus, PostingSide,
};
use engine::calendar::CivilDate;

#[test]
fn unknown_account_and_noncash_touching_cash_rejected() {
    let mut b = opened_books();
    let before = b.clone();

    // 科目表外科目：类型化拒绝（不得悄悄建账）。
    let unknown = entry(
        7,
        "2030-01-05",
        BusinessKind::CashExpense,
        CashFlowClass::Operating,
        &[
            (acct::OPEX, PostingSide::Debit, 5),
            ("9999", PostingSide::Credit, 5),
        ],
    );
    match b.post_batch(vec![unknown]) {
        Err(AccountingError::BatchAborted { cause, .. })
            if matches!(*cause, AccountingError::UnknownAccount { .. }) => {}
        other => panic!("expected UnknownAccount, got {other:?}"),
    }
    assert_eq!(b, before);

    // NonCash 分类却含现金行：矛盾分录拒绝（非现金标识不可与现金行混用）。
    let noncash_cash = entry(
        8,
        "2030-01-06",
        BusinessKind::CashRevenue,
        CashFlowClass::NonCash,
        &[
            (acct::CASH, PostingSide::Debit, 5),
            (acct::REVENUE, PostingSide::Credit, 5),
        ],
    );
    match b.post_batch(vec![noncash_cash]) {
        Err(AccountingError::BatchAborted { cause, .. })
            if matches!(*cause, AccountingError::NonCashTouchesCash { .. }) => {}
        other => panic!("expected NonCashTouchesCash, got {other:?}"),
    }
    assert_eq!(b, before);
}

#[test]
fn negative_cash_rejected_while_negative_equity_representable() {
    let mut b = opened_books();

    // 现金 100 元，支付 150 元：会把现金打成 -50，类型化拒绝（不 clamp）。
    let before = b.clone();
    let overdraft = entry(
        7,
        "2030-01-05",
        BusinessKind::CashExpense,
        CashFlowClass::Operating,
        &[
            (acct::OPEX, PostingSide::Debit, 150),
            (acct::CASH, PostingSide::Credit, 150),
        ],
    );
    match b.post_batch(vec![overdraft]) {
        Err(AccountingError::BatchAborted {
            failed_index,
            cause,
            ..
        }) => {
            assert_eq!(failed_index, None, "cash floor is a batch-level verdict");
            match &*cause {
                AccountingError::NegativeCashProhibited { account, projected } => {
                    assert_eq!(account, &LedgerAccountId(acct::CASH.to_string()));
                    assert_eq!(*projected, yuan(-50));
                }
                other => panic!("expected NegativeCashProhibited, got {other:?}"),
            }
        }
        other => panic!("expected BatchAborted, got {other:?}"),
    }
    assert_eq!(b, before);

    // 批内先出后进：只要批末现金非负即合法（批次是原子单位，中间态不暴露）。
    b.post_batch(vec![
        entry(
            7,
            "2030-01-05",
            BusinessKind::CashExpense,
            CashFlowClass::Operating,
            &[
                (acct::OPEX, PostingSide::Debit, 150),
                (acct::CASH, PostingSide::Credit, 150),
            ],
        ),
        entry(
            8,
            "2030-01-05",
            BusinessKind::LoanDisbursement,
            CashFlowClass::Financing,
            &[
                (acct::CASH, PostingSide::Debit, 200),
                (acct::LOAN, PostingSide::Credit, 200),
            ],
        ),
    ])
    .expect("net-positive batch must post");
    assert_eq!(b.ledger().cash_total().expect("cash"), yuan(150));

    // 合法负权益：大额应计费用（未付）把权益滚成负数——如实表示，不是错误。
    b.post_batch(vec![entry(
        9,
        "2030-01-06",
        BusinessKind::InterestAccrual,
        CashFlowClass::NonCash,
        &[
            (acct::FIN_EXP, PostingSide::Debit, 500),
            (acct::PAYABLE, PostingSide::Credit, 500),
        ],
    )])
    .expect("loss-making entity posts fine");
    // 权益滚动 = 实收资本 100 + 净利 -650 = -550（资产 150 - 负债 700 勾稽一致）
    assert_eq!(b.ledger().equity_rolling().expect("equity"), yuan(-550));
    assert_eq!(b.ledger().net_income().expect("ni"), yuan(-650));
}

#[test]
fn closed_period_rejects_new_postings() {
    let mut b = opened_books();
    b.post_batch(vec![entry(
        7,
        "2030-01-05",
        BusinessKind::CashRevenue,
        CashFlowClass::Operating,
        &[
            (acct::CASH, PostingSide::Debit, 10),
            (acct::REVENUE, PostingSide::Credit, 10),
        ],
    )])
    .expect("january entry");
    let jan = AccountingPeriod::from_ymd(2030, 1).expect("period");
    b.close_period(jan).expect("close january");
    assert_eq!(b.journal().period_status(jan), PeriodStatus::Closed);

    // 已封期间入账：类型化拒绝（结账机制本体属任务 13，此处只有状态 + 守卫）。
    let before = b.clone();
    let late = entry(
        8,
        "2030-01-20",
        BusinessKind::CashRevenue,
        CashFlowClass::Operating,
        &[
            (acct::CASH, PostingSide::Debit, 5),
            (acct::REVENUE, PostingSide::Credit, 5),
        ],
    );
    match b.post_batch(vec![late]) {
        Err(AccountingError::BatchAborted { cause, .. })
            if matches!(*cause, AccountingError::ClosedPeriod { .. }) => {}
        other => panic!("expected ClosedPeriod, got {other:?}"),
    }
    assert_eq!(b, before);

    // 下一开放期间照常入账；重复封账显式拒绝。
    b.post_batch(vec![entry(
        9,
        "2030-02-01",
        BusinessKind::CashRevenue,
        CashFlowClass::Operating,
        &[
            (acct::CASH, PostingSide::Debit, 5),
            (acct::REVENUE, PostingSide::Credit, 5),
        ],
    )])
    .expect("february posts fine");
    assert!(matches!(
        b.close_period(jan),
        Err(AccountingError::PeriodAlreadyClosed { .. })
    ));
}

#[test]
fn nonpositive_line_and_missing_side_rejected() {
    let mut b = opened_books();
    let before = b.clone();
    let date = CivilDate::from_iso("2030-01-05").expect("date");

    // 零金额行：方向语义不允许（正金额 + 借贷方向是唯一表示）。
    let zero_line = JournalEntry {
        source: BusinessEventId::new(7),
        date,
        kind: BusinessKind::CashExpense,
        cash_flow: CashFlowClass::Operating,
        lines: vec![
            JournalLine {
                account: LedgerAccountId(acct::OPEX.to_string()),
                side: PostingSide::Debit,
                amount: AccountingAmount::ZERO,
            },
            JournalLine {
                account: LedgerAccountId(acct::CASH.to_string()),
                side: PostingSide::Credit,
                amount: AccountingAmount::ZERO,
            },
        ],
    };
    match b.post_batch(vec![zero_line]) {
        Err(AccountingError::BatchAborted { cause, .. })
            if matches!(*cause, AccountingError::NonPositiveLine { .. }) => {}
        other => panic!("expected NonPositiveLine, got {other:?}"),
    }
    assert_eq!(b, before);

    // 负金额行同样拒绝（负数必须来自对方科目/方向，不来自行金额符号）。
    let mut negative_line = entry(
        8,
        "2030-01-05",
        BusinessKind::CashExpense,
        CashFlowClass::Operating,
        &[
            (acct::OPEX, PostingSide::Debit, 5),
            (acct::CASH, PostingSide::Credit, 5),
        ],
    );
    negative_line.lines[1].amount = yuan(-5);
    match b.post_batch(vec![negative_line]) {
        Err(AccountingError::BatchAborted { cause, .. })
            if matches!(*cause, AccountingError::NonPositiveLine { .. }) => {}
        other => panic!("expected NonPositiveLine, got {other:?}"),
    }
    assert_eq!(b, before);

    // 只有借方（合计相等也）不构成复式分录：缺贷方拒绝。
    let one_sided = JournalEntry {
        source: BusinessEventId::new(9),
        date,
        kind: BusinessKind::CashExpense,
        cash_flow: CashFlowClass::NonCash,
        lines: vec![
            JournalLine {
                account: LedgerAccountId(acct::OPEX.to_string()),
                side: PostingSide::Debit,
                amount: yuan(5),
            },
            JournalLine {
                account: LedgerAccountId(acct::FIN_EXP.to_string()),
                side: PostingSide::Debit,
                amount: yuan(5),
            },
        ],
    };
    match b.post_batch(vec![one_sided]) {
        Err(AccountingError::BatchAborted { cause, .. })
            if matches!(*cause, AccountingError::MissingSide { .. }) => {}
        other => panic!("expected MissingSide, got {other:?}"),
    }
    assert_eq!(b, before);

    // 空行分录：同样拒绝（守卫可达性——每个错误变体都有触发路径）。
    let empty_lines = JournalEntry {
        source: BusinessEventId::new(10),
        date,
        kind: BusinessKind::CashExpense,
        cash_flow: CashFlowClass::NonCash,
        lines: vec![],
    };
    match b.post_batch(vec![empty_lines]) {
        Err(AccountingError::BatchAborted { cause, .. })
            if matches!(*cause, AccountingError::EmptyLines { .. }) => {}
        other => panic!("expected EmptyLines, got {other:?}"),
    }
    assert_eq!(b, before);
}
