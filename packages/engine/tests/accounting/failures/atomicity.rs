//! 批次原子性 + 来源唯一性：不平衡整批拒绝、重复 BusinessEventId 拒绝，
//! 两条路径都断言完整状态不变（合法的批内兄弟分录也不留痕迹）。

use super::opened_books;
use crate::{acct, yuan};
use engine::accounting::{
    AccountingError, BusinessEventId, BusinessKind, CashFlowClass, PostingSide,
};

#[test]
fn unbalanced_batch_rejected_and_full_state_unchanged() {
    let mut b = opened_books();
    let before = b.clone();
    // 批内第 1 笔合法、第 2 笔借贷不等（借 30 / 贷 20）：整批拒绝、零痕迹。
    let bad = vec![
        crate::entry(
            7,
            "2030-01-05",
            BusinessKind::CashRevenue,
            CashFlowClass::Operating,
            &[
                (acct::CASH, PostingSide::Debit, 10),
                (acct::REVENUE, PostingSide::Credit, 10),
            ],
        ),
        crate::entry(
            8,
            "2030-01-06",
            BusinessKind::CashRevenue,
            CashFlowClass::Operating,
            &[
                (acct::CASH, PostingSide::Debit, 30),
                (acct::REVENUE, PostingSide::Credit, 20),
            ],
        ),
    ];
    let err = b.post_batch(bad).expect_err("unbalanced must be rejected");
    match &err {
        AccountingError::BatchAborted {
            failed_index,
            cause,
            ..
        } => {
            assert_eq!(*failed_index, Some(1));
            assert!(
                matches!(**cause, AccountingError::Unbalanced { .. }),
                "got {cause:?}"
            );
        }
        other => panic!("expected BatchAborted, got {other:?}"),
    }
    // 完整状态不变：合法的第 1 笔也未被记账（journal + ledger 全量比对）
    assert_eq!(b, before);
}

#[test]
fn duplicate_business_event_id_rejected() {
    let mut b = opened_books();
    b.post_batch(vec![crate::entry(
        7,
        "2030-01-05",
        BusinessKind::CashExpense,
        CashFlowClass::Operating,
        &[
            (acct::OPEX, PostingSide::Debit, 5),
            (acct::CASH, PostingSide::Credit, 5),
        ],
    )])
    .expect("first post ok");
    let before = b.clone();

    // 已入账来源再次入账：显式重复错误，不重记。
    let again = crate::entry(
        7,
        "2030-01-06",
        BusinessKind::CashExpense,
        CashFlowClass::Operating,
        &[
            (acct::OPEX, PostingSide::Debit, 6),
            (acct::CASH, PostingSide::Credit, 6),
        ],
    );
    let err = b
        .post_batch(vec![again])
        .expect_err("duplicate must be rejected");
    match &err {
        AccountingError::BatchAborted { cause, .. } => {
            assert!(
                matches!(**cause, AccountingError::DuplicatePosting { event, .. } if event == BusinessEventId::new(7)),
                "got {cause:?}"
            );
        }
        other => panic!("expected BatchAborted, got {other:?}"),
    }
    assert_eq!(b, before);

    // 同一批内部重复：同样拒绝（第 2 笔相对第 1 笔是重复）。
    let dup_in_batch = vec![
        crate::entry(
            8,
            "2030-01-07",
            BusinessKind::CashExpense,
            CashFlowClass::Operating,
            &[
                (acct::OPEX, PostingSide::Debit, 1),
                (acct::CASH, PostingSide::Credit, 1),
            ],
        ),
        crate::entry(
            8,
            "2030-01-07",
            BusinessKind::CashExpense,
            CashFlowClass::Operating,
            &[
                (acct::OPEX, PostingSide::Debit, 2),
                (acct::CASH, PostingSide::Credit, 2),
            ],
        ),
    ];
    let err = b
        .post_batch(dup_in_batch)
        .expect_err("in-batch duplicate rejected");
    match &err {
        AccountingError::BatchAborted {
            failed_index,
            cause,
            ..
        } => {
            assert_eq!(*failed_index, Some(1));
            assert!(
                matches!(**cause, AccountingError::DuplicatePosting { .. }),
                "got {cause:?}"
            );
        }
        other => panic!("expected BatchAborted, got {other:?}"),
    }
    assert_eq!(
        b, before,
        "state must stay identical after duplicate rejection"
    );
    assert_eq!(b.journal().entry_count(), 2, "opening + first post only");
    assert_eq!(b.ledger().cash_total().expect("cash"), yuan(95));
}
