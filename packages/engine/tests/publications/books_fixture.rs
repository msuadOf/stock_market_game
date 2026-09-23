//! 原始账套夹具（更正/失败路径用；industry_reports 套件同款形态）。

use crate::fixture::d;
use engine::accounting::{
    AccountingAmount, Books, BusinessEventId, BusinessKind, CashFlowClass, JournalEntry,
    JournalLine, LedgerAccountId, PostingSide,
};

/// 元 → AccountingAmount（分）。
pub(crate) fn yuan(v: i128) -> AccountingAmount {
    AccountingAmount::from_cents(v.checked_mul(100).expect("fixture yuan overflow"))
}

/// 快速构造分录（金额参数单位 = 元）。
pub(crate) fn entry(
    source: u64,
    date: &str,
    kind: BusinessKind,
    cash_flow: CashFlowClass,
    lines: &[(&str, PostingSide, i128)],
) -> JournalEntry {
    JournalEntry {
        source: BusinessEventId::new(source),
        date: d(date),
        kind,
        cash_flow,
        lines: lines
            .iter()
            .map(|(account, side, amount)| JournalLine {
                account: LedgerAccountId((*account).to_string()),
                side: *side,
                amount: yuan(*amount),
            })
            .collect(),
    }
}

/// 更正金样账套（科目表 v2）：2029 开局 + 2030 全年经营流（经 Q1/年末）。
pub(crate) fn correction_books() -> Books {
    use BusinessKind::*;
    use CashFlowClass::*;
    use PostingSide::{Credit, Debit};
    let mut books = Books::new(engine::company::industrial::industrial_chart_v2());
    books
        .post_batch(vec![
            entry(
                1,
                "2029-12-31",
                OpeningBalance,
                Financing,
                &[
                    ("1002", Debit, 90_000),
                    ("1601", Debit, 10_000),
                    ("4001", Credit, 100_000),
                ],
            ),
            entry(
                2,
                "2030-01-15",
                CashRevenue,
                Operating,
                &[("1002", Debit, 2_000), ("6001", Credit, 2_000)],
            ),
            entry(
                3,
                "2030-02-10",
                CreditSale,
                NonCash,
                &[("1122", Debit, 1_500), ("6001", Credit, 1_500)],
            ),
            entry(
                4,
                "2030-02-20",
                ReceivableCollection,
                Operating,
                &[("1002", Debit, 1_000), ("1122", Credit, 1_000)],
            ),
            entry(
                5,
                "2030-03-15",
                Depreciation,
                NonCash,
                &[("6602", Debit, 120), ("1602", Credit, 120)],
            ),
            entry(
                6,
                "2030-11-05",
                CashRevenue,
                Operating,
                &[("1002", Debit, 3_000), ("6001", Credit, 3_000)],
            ),
        ])
        .expect("fixture entries must post");
    books
}
