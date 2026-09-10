//! 期间类型 + 账套 serde/恢复边界：事实重放重建派生总账；篡改存档
//! （注入不平衡分录 / 重复来源）必须恢复失败。

use super::opened_books;
use crate::{acct, entry, yuan};
use engine::accounting::{
    AccountingAmount, AccountingError, AccountingPeriod, Books, BusinessEventId, BusinessKind,
    CashFlowClass, JournalEntry, JournalLine, LedgerAccountId, PeriodStatus, PostingSide,
};
use engine::calendar::CivilDate;

#[test]
fn period_types_validate_and_serialize_iso() {
    assert!(AccountingPeriod::from_ymd(2030, 1).is_ok());
    assert!(matches!(
        AccountingPeriod::from_ymd(2030, 13),
        Err(AccountingError::InvalidPeriod { .. })
    ));
    assert!(matches!(
        AccountingPeriod::from_ymd(1899, 1),
        Err(AccountingError::InvalidPeriod { .. })
    ));
    let p = AccountingPeriod::from_iso("2030-01").expect("iso");
    assert_eq!(p.year(), 2030);
    assert_eq!(p.month(), 1);
    assert_eq!(p.to_string(), "2030-01");
    assert!(matches!(
        AccountingPeriod::from_iso("2030-1"),
        Err(AccountingError::PeriodParse { .. })
    ));
    // serde = "YYYY-MM" 字符串
    let json = serde_json::to_value(p).expect("ser");
    assert_eq!(json, serde_json::Value::String("2030-01".to_string()));
    let back: AccountingPeriod = serde_json::from_value(json).expect("de");
    assert_eq!(back, p);
    // of_date 取自合法 CivilDate（日期已验，期间构造不可失败）
    let date = CivilDate::from_iso("2030-11-03").expect("date");
    assert_eq!(
        AccountingPeriod::of_date(date),
        AccountingPeriod::from_ymd(2030, 11).expect("period")
    );
}

/// 账套级 serde 往返：Books 只序列化「事实」（科目表 + 日记账），总账索引为派生
/// 重建——来源事实与派生 report 边界不可混用。篡改存档注入非法分录必须恢复失败。
#[test]
fn books_serde_round_trip_and_tampered_save_rejected() {
    let mut b = opened_books();
    b.post_batch(vec![entry(
        7,
        "2030-01-05",
        BusinessKind::CashRevenue,
        CashFlowClass::Operating,
        &[
            (acct::CASH, PostingSide::Debit, 30),
            (acct::REVENUE, PostingSide::Credit, 30),
        ],
    )])
    .expect("post");
    b.close_period(AccountingPeriod::from_ymd(2029, 12).expect("period"))
        .expect("close 2029-12");

    let json = serde_json::to_string(&b).expect("ser");
    let back: Books = serde_json::from_str(&json).expect("de");
    assert_eq!(
        back, b,
        "books must round-trip exactly (ledger rebuilt from facts)"
    );
    // 派生索引在恢复后与原账套一致（现金/权益/封闭期间全部还原）
    assert_eq!(back.ledger().cash_total().expect("cash"), yuan(130));
    assert_eq!(
        back.journal()
            .period_status(AccountingPeriod::from_ymd(2029, 12).expect("p")),
        PeriodStatus::Closed
    );

    // 篡改：向第一批注入一条不平衡分录（source 99 只有借方现金 5 元）。
    let mut value: serde_json::Value = serde_json::from_str(&json).expect("value");
    let bad_entry = serde_json::json!({
        "source": 99,
        "date": "2030-01-05",
        "kind": "CashRevenue",
        "cash_flow": "Operating",
        "lines": [
            { "account": acct::CASH, "side": "Debit", "amount": "5.00" }
        ]
    });
    value["journal"]["batches"][0]
        .as_array_mut()
        .expect("batches")
        .push(bad_entry);
    let err = serde_json::from_value::<Books>(value);
    assert!(
        err.is_err(),
        "tampered save with invalid entry must fail restore"
    );

    // 篡改：注入与既有来源重复的 event id（重记现金）。
    let mut dup_value: serde_json::Value = serde_json::from_str(&json).expect("value");
    let dup_entry = serde_json::json!({
        "source": 1,
        "date": "2030-01-05",
        "kind": "CashRevenue",
        "cash_flow": "Operating",
        "lines": [
            { "account": acct::CASH, "side": "Debit", "amount": "9.00" },
            { "account": acct::REVENUE, "side": "Credit", "amount": "9.00" }
        ]
    });
    dup_value["journal"]["batches"][1]
        .as_array_mut()
        .expect("batches")
        .push(dup_entry);
    let err = serde_json::from_value::<Books>(dup_value);
    assert!(
        err.is_err(),
        "tampered save with duplicate source must fail restore"
    );
}

/// 大于 JS 安全整数的多行分录经完整账套 serde 精确往返（十进制字符串承载）。
#[test]
fn journal_entry_serde_carries_big_amounts_exactly() {
    let e = JournalEntry {
        source: BusinessEventId::new(42),
        date: CivilDate::from_iso("2030-01-05").expect("date"),
        kind: BusinessKind::CashRevenue,
        cash_flow: CashFlowClass::Operating,
        lines: vec![
            JournalLine {
                account: LedgerAccountId(acct::CASH.to_string()),
                side: PostingSide::Debit,
                amount: AccountingAmount::from_cents(9_007_199_254_740_994),
            },
            JournalLine {
                account: LedgerAccountId(acct::REVENUE.to_string()),
                side: PostingSide::Credit,
                amount: AccountingAmount::from_cents(9_007_199_254_740_994),
            },
        ],
    };
    let json = serde_json::to_string(&e).expect("ser");
    assert!(
        json.contains("\"90071992547409.94\""),
        "amount as decimal string: {json}"
    );
    assert!(
        !json.contains("9.007199254740"),
        "no JS-number form: {json}"
    );
    let back: JournalEntry = serde_json::from_str(&json).expect("de");
    assert_eq!(back, e);
}
