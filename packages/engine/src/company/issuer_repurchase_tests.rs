//! 发行人回购基础测试：方案校验、成交额度约束、计划完成回收与注销边界
//! （ADR-0038，2026-10-07 M 批）。

use super::{
    IssuerRepurchaseBook, IssuerRepurchaseError, IssuerRepurchasePlan, RepurchaseFillRecord,
    RepurchasePurpose,
};
use crate::company::CompanyId;
use crate::{account::StockCode, calendar::CivilDate, money::Money};

fn date(value: &str) -> CivilDate {
    CivilDate::from_iso(value).unwrap()
}

fn plan() -> IssuerRepurchasePlan {
    IssuerRepurchasePlan {
        event_id: "repurchase-1".to_owned(),
        approval_reference: "board-resolution-r1".to_owned(),
        issuer: CompanyId("issuer-1".to_owned()),
        stock: StockCode("600001".to_owned()),
        exchange: crate::calendar::CalendarExchange::Sse,
        approved_on: date("2030-06-01"),
        announced_on: date("2030-06-02"),
        window_start_on: date("2030-06-03"),
        window_deadline_on: date("2030-06-11"),
        price_cap_per_share: Money::from_cents(1_200),
        total_budget: Money::from_cents(3_000_000),
        max_shares: 2_500,
        purpose: RepurchasePurpose::ReduceCapital,
    }
}

fn fill(day: &str, shares: u64, gross_cents: i64, fees_cents: i64) -> RepurchaseFillRecord {
    RepurchaseFillRecord {
        stock: StockCode("600001".to_owned()),
        day: date(day),
        shares,
        gross: Money::from_cents(gross_cents),
        fees: Money::from_cents(fees_cents),
    }
}

#[test]
fn plan_validation_rejects_misordered_dates_and_nonpositive_amounts() {
    plan().validate().unwrap();
    let mut bad = plan();
    bad.window_deadline_on = date("2030-06-02");
    assert!(bad.validate().is_err());
    let mut bad = plan();
    bad.total_budget = Money::from_cents(0);
    assert!(bad.validate().is_err());
    let mut bad = plan();
    bad.max_shares = 0;
    assert!(bad.validate().is_err());
    let calendar = crate::calendar::TradingCalendar::current_default_calendar().unwrap();
    let mut bad = plan();
    bad.window_start_on = date("2030-06-08");
    assert!(matches!(
        bad.validate_calendar(&calendar),
        Err(IssuerRepurchaseError::InvalidTradingDate { .. })
    ));
    plan().validate_calendar(&calendar).unwrap();
}

#[test]
fn fills_are_bounded_by_budget_share_cap_and_window() {
    let mut book = IssuerRepurchaseBook::new(plan()).unwrap();
    book.announce(date("2030-06-02")).unwrap();
    book.record_fill(fill("2030-06-03", 1_000, 1_100_000, 110)).unwrap();
    // 同日重复拒绝。
    assert!(book.record_fill(fill("2030-06-03", 1, 1_100, 1)).is_err());
    // 窗口外拒绝。
    assert!(book.record_fill(fill("2030-06-12", 100, 110_000, 11)).is_err());
    // 超额度拒绝：已花 1,100,110，额度 3,000,000；再花 1,900,000 → 超出。
    assert!(matches!(
        book.record_fill(fill("2030-06-04", 1_600, 1_900_000, 1)),
        Err(IssuerRepurchaseError::FillExceedsBudget { .. })
    ));
    // 超数量上限拒绝（1,000 + 1,600 > 2,500）。
    assert!(book
        .record_fill(fill("2030-06-04", 1_600, 1_600_000, 16))
        .is_err());
    book.record_fill(fill("2030-06-04", 500, 550_000, 55)).unwrap();
    assert_eq!(book.total_filled_shares(), 1_500);
    assert_eq!(
        book.remaining_budget().unwrap(),
        Money::from_cents(3_000_000 - 1_100_110 - 550_055)
    );
    book.validate().unwrap();
}

#[test]
fn completion_withdraws_exact_remainder_and_cancellation_bounded_by_fills() {
    let mut book = IssuerRepurchaseBook::new(plan()).unwrap();
    book.announce(date("2030-06-02")).unwrap();
    book.record_fill(fill("2030-06-03", 1_000, 1_100_000, 110)).unwrap();
    // 完成日前置拒绝。
    assert!(book.complete(date("2030-06-10"), Money::from_cents(1)).is_err());
    let remainder = Money::from_cents(3_000_000 - 1_100_110);
    // 回收额与额度—支出不一致拒绝。
    assert!(book.complete(date("2030-06-11"), Money::from_cents(1)).is_err());
    book.complete(date("2030-06-11"), remainder).unwrap();
    book.complete(date("2030-06-11"), remainder).unwrap();
    // 注销超过累计成交拒绝。
    assert!(matches!(
        book.record_cancellation(date("2030-06-12"), 1_001),
        Err(IssuerRepurchaseError::CancellationExceedsFills { .. })
    ));
    book.record_cancellation(date("2030-06-12"), 1_000).unwrap();
    book.record_cancellation(date("2030-06-12"), 1_000).unwrap();
    book.validate().unwrap();
    // 完成后不再受理成交。
    assert!(book.record_fill(fill("2030-06-05", 1, 1_100, 1)).is_err());
    // 严格 JSON 往返深等。
    let json = serde_json::to_string(&book).unwrap();
    let restored: IssuerRepurchaseBook = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, book);
}
