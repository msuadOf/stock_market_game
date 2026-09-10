//! 负向拒绝金样：不平衡、重复来源、未知科目、非现金触碰现金、负现金、已封期间、
//! 非正金额/缺边。所有账套级拒绝都断言**完整状态不变**（Books 整体 PartialEq），
//! 逐笔分录错误不留半条账。合法负权益显式可表示（不 clamp）。
//! serde/恢复边界用例在 `serde_restore`；金额算术单测在 `amount_unit`。

mod atomicity;
mod posting_guards;
mod serde_restore;

use crate::{acct, entry};
use engine::accounting::Books;
use engine::accounting::{BusinessKind, CashFlowClass, PostingSide};

/// 带期初（cash/equity 各 100 元 @2029-12-31）的账套。
pub(crate) fn opened_books() -> Books {
    let mut b = crate::books();
    b.post_batch(vec![entry(
        1,
        "2029-12-31",
        BusinessKind::OpeningBalance,
        CashFlowClass::Financing,
        &[
            (acct::CASH, PostingSide::Debit, 100),
            (acct::CAPITAL, PostingSide::Credit, 100),
        ],
    )])
    .expect("opening");
    b
}
