//! entry_count 的合法状态与实际迭代工作量回归；金额以分计，不更改过账事实。
use super::*;
use crate::accounting::{
    AccountChart, AccountingAmount, Books, BooksTransactionError, BusinessKind, CashFlowClass,
    JournalLine, LedgerAccountId, PostingSide,
};

fn entry(source: u64, four_lines: bool) -> JournalEntry {
    let line = |account: &str, side| JournalLine {
        account: LedgerAccountId(account.to_owned()),
        side,
        amount: AccountingAmount::from_cents(100),
    };
    let mut lines = vec![
        line("1002", PostingSide::Debit),
        line("6001", PostingSide::Credit),
    ];
    if four_lines {
        lines.extend([
            line("1122", PostingSide::Debit),
            line("6001", PostingSide::Credit),
        ]);
    }
    JournalEntry {
        source: BusinessEventId::new(source),
        date: CivilDate::from_iso("2030-01-05").unwrap(),
        kind: BusinessKind::CashRevenue,
        cash_flow: CashFlowClass::Operating,
        lines,
    }
}
fn books() -> Books {
    Books::new(AccountChart::generic_account_chart())
}
fn assert_count(journal: &Journal, expected: usize) {
    assert_eq!(journal.entry_count(), expected);
    assert_eq!(
        journal.entries().count(),
        expected,
        "独立批次事实展平数量必须相同"
    );
}

#[test]
fn entry_count_counts_entries_across_batches_not_lines_and_clone_is_independent() {
    let mut books = books();
    assert_count(books.journal(), 0);
    books.post_batch(vec![]).unwrap();
    assert_eq!(books.journal().batches.len(), 0, "空批不是新事实");
    books.post_batch(vec![entry(1, false)]).unwrap();
    books
        .post_batch(vec![entry(2, true), entry(3, false)])
        .unwrap();
    assert_eq!(books.journal().batches.len(), 2);
    assert_eq!(
        books
            .journal()
            .entries()
            .map(|entry| entry.lines.len())
            .sum::<usize>(),
        8
    );
    assert_count(books.journal(), 3);
    let snapshot = books.clone();
    let snapshot_bytes = serde_json::to_vec(&snapshot).unwrap();
    books.post_batch(vec![entry(4, false)]).unwrap();
    assert_count(books.journal(), 4);
    assert_count(snapshot.journal(), 3);
    assert_eq!(serde_json::to_vec(&snapshot).unwrap(), snapshot_bytes);
}

#[test]
fn entry_count_preserves_duplicate_rejection_and_real_transaction_rollback() {
    let mut books = books();
    books.post_batch(vec![entry(1, false)]).unwrap();
    let before = books.clone();
    for batch in [vec![entry(1, true)], vec![entry(2, false), entry(2, true)]] {
        let error = books.post_batch(batch).expect_err("重复来源必须拒绝");
        assert!(
            matches!(error, AccountingError::BatchAborted { cause, .. } if matches!(*cause, AccountingError::DuplicatePosting { .. }))
        );
        assert_eq!(books, before);
        assert_count(books.journal(), 1);
        assert_eq!(books.journal().posted_date(BusinessEventId::new(2)), None);
    }
    let result = books.transact_post_batch(vec![entry(2, false), entry(3, true)], |posted| {
        assert_count(posted.journal(), 3);
        Err::<(), _>("真实事务操作拒绝")
    });
    assert!(matches!(
        result,
        Err(BooksTransactionError::Operation("真实事务操作拒绝"))
    ));
    assert_eq!(books, before);
    assert_count(books.journal(), 1);
    assert_eq!(books.journal().posted_date(BusinessEventId::new(2)), None);
    assert_eq!(books.journal().posted_date(BusinessEventId::new(3)), None);
    let committed = books.transact_post_batch(vec![entry(2, false), entry(3, true)], |posted| {
        Ok::<_, &'static str>(posted.journal().entry_count())
    });
    assert!(matches!(committed, Ok(3)));
    assert_count(books.journal(), 3);
    assert_count(before.journal(), 1);
}

#[test]
fn entry_count_journal_and_books_serde_rebuild_one_source_index_without_new_wire() {
    let mut books = books();
    books
        .post_batch(vec![entry(1, false), entry(2, true)])
        .unwrap();
    books.post_batch(vec![entry(3, false)]).unwrap();
    books
        .close_period(AccountingPeriod::from_iso("2029-12").unwrap())
        .unwrap();
    let empty_batches: Journal =
        serde_json::from_value(serde_json::json!({"batches": [[], []], "closed": []})).unwrap();
    assert_eq!(empty_batches.batches.len(), 2);
    assert_count(&empty_batches, 0);
    let journal_wire = serde_json::to_value(books.journal()).unwrap();
    let mut keys: Vec<_> = journal_wire
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort();
    assert_eq!(keys, ["batches", "closed"], "不存posted或新增计数缓存");
    let journal: Journal = serde_json::from_value(journal_wire.clone()).unwrap();
    assert_count(&journal, 3);
    assert_eq!(&journal, books.journal());
    let wire = serde_json::to_vec(&books).unwrap();
    let restored: Books = serde_json::from_slice(&wire).unwrap();
    assert_count(restored.journal(), 3);
    assert_eq!(restored, books);
    assert_eq!(serde_json::to_vec(&restored).unwrap(), wire);
    let mut duplicate = journal_wire;
    let repeated = duplicate["batches"][0][0].clone();
    duplicate["batches"][1]
        .as_array_mut()
        .unwrap()
        .push(repeated);
    assert!(serde_json::from_value::<Journal>(duplicate.clone()).is_err());
    let mut books_wire = serde_json::to_value(&books).unwrap();
    books_wire["journal"] = duplicate;
    assert!(serde_json::from_value::<Books>(books_wire).is_err());
}

#[test]
fn entry_count_does_not_walk_accumulated_batch_history() {
    const QUERIES: usize = 8;
    let mut measured = Vec::new();
    for batches in [32, 256] {
        let mut books = books();
        for source in 0..batches {
            books.post_batch(vec![entry(source as u64, false)]).unwrap();
        }
        assert_count(books.journal(), batches);
        let (total, yielded) = crate::shared_history::yield_probe::measure(|| {
            (0..QUERIES)
                .map(|_| std::hint::black_box(books.journal()).entry_count())
                .sum::<usize>()
        });
        assert_eq!(total, QUERIES * batches);
        // 已disarm后再读事实oracle，不把测试校验自身计入entry_count工作量。
        assert_count(books.journal(), batches);
        measured.push(yielded);
    }
    eprintln!("固定8次entry_count在32/256批次的真实HistoryIter成功yield={measured:?}");
    assert_eq!(
        measured[0], measured[1],
        "同Q计数的历史访问量不应随batch数增长；不钉死恰0或某个实现"
    );
}
