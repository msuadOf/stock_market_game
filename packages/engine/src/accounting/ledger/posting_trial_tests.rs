//! 过账候选的真实复制工作量与完整回滚边界；金额单位均为分。

use super::*;
use crate::accounting::{Books, BooksTransactionError, BusinessEventId, BusinessKind};
use crate::calendar::CivilDate;

fn date(year: i32, month: u8) -> CivilDate {
    CivilDate::from_ymd(year, month, 1).unwrap()
}

fn entry(
    source: u64,
    when: CivilDate,
    class: CashFlowClass,
    lines: &[(&str, PostingSide, i128)],
) -> JournalEntry {
    JournalEntry {
        source: BusinessEventId::new(source),
        date: when,
        kind: BusinessKind::OpeningBalance,
        cash_flow: class,
        lines: lines
            .iter()
            .map(|(code, side, cents)| JournalLine {
                account: LedgerAccountId((*code).into()),
                side: *side,
                amount: AccountingAmount::from_cents(*cents),
            })
            .collect(),
    }
}

fn opened_books() -> Books {
    let mut books = Books::new(AccountChart::generic_account_chart());
    books
        .post_batch(vec![entry(
            1,
            date(2026, 1),
            CashFlowClass::Financing,
            &[
                ("1002", PostingSide::Debit, 10_000),
                ("4001", PostingSide::Credit, 10_000),
            ],
        )])
        .unwrap();
    books
}

fn assert_aborted(
    error: AccountingError,
    index: Option<usize>,
    expected: impl FnOnce(&AccountingError) -> bool,
) {
    match error {
        AccountingError::BatchAborted {
            failed_index,
            cause,
            ..
        } => {
            assert_eq!(failed_index, index);
            assert!(
                expected(&cause),
                "实际过账错误必须保留原类型与顺序：{cause:?}"
            );
        }
        other => panic!("应返回 BatchAborted，实际为{other:?}"),
    }
}

#[test]
fn small_post_and_transaction_do_not_clone_accumulated_period_balances() {
    const POSTS: usize = 4;
    let mut measured = Vec::new();
    for transactional in [false, true] {
        let mut counts = Vec::new();
        for periods in [32, 256] {
            let mut books = Books::new(AccountChart::generic_account_chart());
            for index in 0..periods {
                let when = date(2000 + index / 12, (index % 12 + 1) as u8);
                books
                    .post_batch(vec![entry(
                        index as u64,
                        when,
                        CashFlowClass::NonCash,
                        &[
                            ("1122", PostingSide::Debit, 1),
                            ("6001", PostingSide::Credit, 1),
                        ],
                    )])
                    .unwrap();
            }
            let (_, cloned) = super::balance_clone_probe::measure(|| {
                for index in 0..POSTS {
                    let batch = vec![entry(
                        10_000 + index as u64,
                        date(2030, 1),
                        CashFlowClass::NonCash,
                        &[
                            ("1122", PostingSide::Debit, 1),
                            ("6001", PostingSide::Credit, 1),
                        ],
                    )];
                    if transactional {
                        books
                            .transact_post_batch(batch, |_| Ok::<_, ()>(()))
                            .unwrap_or_else(|_| panic!("合法 transaction 必须成功"));
                    } else {
                        books.post_batch(batch).unwrap();
                    }
                }
            });
            // oracle、clone 与 serde 均在 probe 已 disarm 后执行。
            assert_eq!(books.journal().entries().count(), periods as usize + POSTS);
            let wire = serde_json::to_vec(&books).unwrap();
            assert_eq!(serde_json::from_slice::<Books>(&wire).unwrap(), books);
            counts.push(cloned);
        }
        measured.push(counts);
    }
    eprintln!("固定4次post/transact在32/256期间的真实AccountBalance::clone次数={measured:?}");
    for counts in measured {
        assert_eq!(
            counts[0], counts[1],
            "小批过账复制余额的工作量不得随历史期间增长"
        );
    }
}

#[test]
fn full_batch_structural_duplicate_and_period_checks_precede_ledger_application() {
    let unknown = entry(
        2,
        date(2026, 2),
        CashFlowClass::NonCash,
        &[
            ("9999", PostingSide::Debit, 1),
            ("6001", PostingSide::Credit, 1),
        ],
    );
    for variant in 0..3 {
        let mut books = opened_books();
        let mut second = entry(
            if variant == 1 { 2 } else { 3 },
            date(2026, 1),
            CashFlowClass::NonCash,
            &[
                ("1122", PostingSide::Debit, 1),
                ("6001", PostingSide::Credit, 1),
            ],
        );
        if variant == 0 {
            second.lines.clear();
        }
        if variant == 2 {
            books
                .close_period(AccountingPeriod::from_ymd(2026, 1).unwrap())
                .unwrap();
        }
        let before = books.clone();
        let error = books.post_batch(vec![unknown.clone(), second]).unwrap_err();
        assert_aborted(error, Some(1), |cause| match variant {
            0 => matches!(cause, AccountingError::EmptyLines { .. }),
            1 => matches!(cause, AccountingError::DuplicatePosting { .. }),
            _ => matches!(cause, AccountingError::ClosedPeriod { .. }),
        });
        assert_eq!(books, before, "全批结构失败不能留下第一笔的部分投影");
    }
}

#[test]
fn partial_index_writes_and_late_line_overflow_restore_complete_books() {
    for noncash in [true, false] {
        let mut books = opened_books();
        let before = books.clone();
        let bad = entry(
            2,
            date(2026, 2),
            CashFlowClass::NonCash,
            &[
                ("1122", PostingSide::Debit, 1),
                (
                    if noncash { "1002" } else { "9999" },
                    PostingSide::Credit,
                    1,
                ),
            ],
        );
        assert_aborted(books.post_batch(vec![bad]).unwrap_err(), Some(0), |cause| {
            if noncash {
                matches!(cause, AccountingError::NonCashTouchesCash { .. })
            } else {
                matches!(cause, AccountingError::UnknownAccount { .. })
            }
        });
        assert_eq!(books, before, "新期间/新索引键必须删除，不能留下默认零值");
    }
    let mut books = Books::new(AccountChart::generic_account_chart());
    books
        .post_batch(vec![entry(
            1,
            date(2026, 1),
            CashFlowClass::Financing,
            &[
                ("1002", PostingSide::Debit, i128::MAX),
                ("4001", PostingSide::Credit, i128::MAX),
            ],
        )])
        .unwrap();
    let before = books.clone();
    // 第一行新收入键已成功写入，第二行现金借方累计才溢出；保持原行顺序和精确 failed_index。
    let overflow = entry(
        2,
        date(2026, 2),
        CashFlowClass::Operating,
        &[
            ("6001", PostingSide::Credit, 1),
            ("1002", PostingSide::Debit, 1),
        ],
    );
    assert_aborted(
        books.post_batch(vec![overflow]).unwrap_err(),
        Some(0),
        |cause| matches!(cause, AccountingError::AmountOverflow { .. }),
    );
    assert_eq!(books, before);
}

#[test]
fn cash_floor_is_batch_final_and_keeps_original_chart_error_order() {
    let mut books = opened_books();
    let expense = |source, account| {
        entry(
            source,
            date(2026, 2),
            CashFlowClass::Operating,
            &[
                ("6602", PostingSide::Debit, 20_000),
                (account, PostingSide::Credit, 20_000),
            ],
        )
    };
    books
        .post_batch(vec![
            expense(2, "1002"),
            entry(
                3,
                date(2026, 2),
                CashFlowClass::Financing,
                &[
                    ("1002", PostingSide::Debit, 20_000),
                    ("2001", PostingSide::Credit, 20_000),
                ],
            ),
        ])
        .unwrap();
    assert_eq!(
        books.ledger().cash_total().unwrap(),
        AccountingAmount::from_cents(10_000)
    );
    let before = books.clone();
    // 两个现金科目均为负，仍按原 chart 顺序先报告1001，不按批次触键顺序挑错误。
    let error = books
        .post_batch(vec![expense(4, "1002"), expense(5, "1001")])
        .unwrap_err();
    assert_aborted(
        error,
        None,
        |cause| matches!(cause, AccountingError::NegativeCashProhibited { account, .. } if account.0 == "1001"),
    );
    assert_eq!(books, before);
}

#[test]
fn transaction_callback_sees_full_candidate_and_error_restores_first_values() {
    let mut books = opened_books();
    let before = books.clone();
    let batch = vec![
        entry(
            2,
            date(2026, 2),
            CashFlowClass::Operating,
            &[
                ("1002", PostingSide::Debit, 10),
                ("6001", PostingSide::Credit, 10),
            ],
        ),
        entry(
            3,
            date(2026, 2),
            CashFlowClass::Operating,
            &[
                ("1002", PostingSide::Debit, 20),
                ("6001", PostingSide::Credit, 20),
            ],
        ),
        entry(
            4,
            date(2026, 3),
            CashFlowClass::Financing,
            &[
                ("1002", PostingSide::Debit, 30),
                ("2001", PostingSide::Credit, 30),
            ],
        ),
    ];
    let error = books.transact_post_batch(batch.clone(), |candidate| {
        assert_eq!(candidate.journal().entries().count(), 4);
        assert_eq!(
            candidate.ledger().cash_total().unwrap(),
            AccountingAmount::from_cents(10_060)
        );
        assert_eq!(
            candidate.ledger().cash_flow_for_period(
                AccountingPeriod::from_ymd(2026, 2).unwrap(),
                CashFlowClass::Operating
            ),
            AccountingAmount::from_cents(30)
        );
        Err::<(), _>("原业务拒绝")
    });
    assert!(matches!(
        error,
        Err(BooksTransactionError::Operation("原业务拒绝"))
    ));
    assert_eq!(
        books, before,
        "重复触同键必须恢复首次旧值，原None键必须删除"
    );
    let candidate_count = books
        .transact_post_batch(batch, |candidate| {
            Ok::<_, ()>(candidate.journal().entries().count())
        })
        .unwrap_or_else(|_| panic!("回滚的原source必须可以重新提交"));
    assert_eq!(candidate_count, 4);
    let committed = books.clone();
    assert_eq!(
        books
            .transact_post_batch(vec![], |candidate| Ok::<_, ()>(
                candidate.journal().entries().count()
            ))
            .unwrap_or_else(|_| panic!("空批callback必须保留")),
        4
    );
    assert_eq!(books, committed);
    let wire = serde_json::to_vec(&books).unwrap();
    assert_eq!(serde_json::from_slice::<Books>(&wire).unwrap(), books);
}
