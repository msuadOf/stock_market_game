//! 重复回款 / 未知应收 / 超额回款 / 无准备核销 / 非法坏账率 → 类型化拒绝，
//! 状态字节不变；回款绝不重复计收入。

use super::super::{acct, d, net_debit, yuan};
use super::fresh;
use engine::accounting::InventoryItemCode;
use engine::accounting::LedgerAccountId;
use engine::company::industrial::{IndustrialError, Settlement};
use engine::company::CounterpartyId;

fn cp(id: &str) -> CounterpartyId {
    CounterpartyId(id.to_string())
}

fn goods() -> InventoryItemCode {
    InventoryItemCode("GOODS".to_string())
}

/// 夹具：现购 10 件 @ 10 元并赊销 10 件 @ 100 元（应收 1130 元）。
fn sold_book() -> (
    engine::company::industrial::IndustrialBooks,
    engine::accounting::OpenItemId,
) {
    let mut co = fresh();
    co.purchase(
        &cp("EXT-SUPP"),
        goods(),
        LedgerAccountId(acct::FINISHED.to_string()),
        10,
        yuan(10),
        Settlement::Cash,
        d("2030-03-01"),
        d("2030-01-05"),
    )
    .expect("purchase");
    let sale = co
        .sell_credit(
            &cp("EXT-CUST"),
            goods(),
            10,
            yuan(100),
            d("2030-02-15"),
            d("2030-01-10"),
        )
        .expect("sale");
    (co, sale.receivable)
}

#[test]
fn duplicate_collection_is_rejected_without_double_revenue_or_cash() {
    let (mut co, ar) = sold_book();
    let revenue_before = net_debit(&co, acct::REVENUE);
    co.collect(&ar, yuan(1_130), d("2030-01-20"))
        .expect("first collection");
    let cash_after_first = net_debit(&co, acct::BANK);
    assert_eq!(net_debit(&co, acct::REVENUE), revenue_before);
    // 第二次回款同一应收：项目已清 → 类型化拒绝（不补钱、不重复计收入）。
    let before = co.clone();
    assert!(matches!(
        co.collect(&ar, yuan(1_130), d("2030-01-21")),
        Err(IndustrialError::Trade(
            engine::accounting::TradeLedgerError::ItemCleared { .. }
        ))
    ));
    assert_eq!(co, before);
    assert_eq!(net_debit(&co, acct::BANK), cash_after_first);
    assert_eq!(net_debit(&co, acct::REVENUE), revenue_before);
}

#[test]
fn over_collection_and_unknown_receivable_are_rejected() {
    let (mut co, ar) = sold_book();
    let before = co.clone();
    // 超额回款 → OverApplication。
    assert!(matches!(
        co.collect(&ar, yuan(1_131), d("2030-01-20")),
        Err(IndustrialError::Trade(
            engine::accounting::TradeLedgerError::OverApplication { .. }
        ))
    ));
    // 未知应收 id → UnknownItem。
    assert!(matches!(
        co.collect(
            &engine::accounting::OpenItemId("AR-999".to_string()),
            yuan(1),
            d("2030-01-20")
        ),
        Err(IndustrialError::Trade(
            engine::accounting::TradeLedgerError::UnknownItem { .. }
        ))
    ));
    // 零金额回款 → NonPositiveAmount。
    assert!(matches!(
        co.collect(&ar, yuan(0), d("2030-01-20")),
        Err(IndustrialError::NonPositiveAmount { .. })
    ));
    assert_eq!(co, before);
    // 合法部分回款仍可行：收 130 元。
    co.collect(&ar, yuan(130), d("2030-01-20"))
        .expect("partial collection");
    assert_eq!(net_debit(&co, acct::AR), yuan(1_000));
}

#[test]
fn illegal_ecl_rate_and_zero_delta_allowance_behaviour() {
    let (mut co, _ar) = sold_book();
    let before = co.clone();
    // 坏账率越界（>10000bp / 负）→ 类型化拒绝。
    assert!(matches!(
        co.update_bad_debt_allowance(10_001, d("2030-01-31")),
        Err(IndustrialError::InvalidRateBp { rate_bp: 10_001 })
    ));
    assert!(matches!(
        co.update_bad_debt_allowance(-1, d("2030-01-31")),
        Err(IndustrialError::InvalidRateBp { rate_bp: -1 })
    ));
    assert_eq!(co, before);
    // 零坏账率 → 目标 0，posted 0 → delta 0 → Ok(None)（无分录、状态不变）。
    let outcome = co
        .update_bad_debt_allowance(0, d("2030-01-31"))
        .expect("zero target is legal");
    assert!(outcome.is_none());
    assert_eq!(co, before);
}

#[test]
fn write_off_requires_full_allowance_and_open_item() {
    let (mut co, ar) = sold_book();
    let before = co.clone();
    // 未计提任何准备 → InsufficientAllowance。
    assert!(matches!(
        co.write_off_receivable(&ar, d("2030-02-01")),
        Err(IndustrialError::InsufficientAllowance { .. })
    ));
    assert_eq!(co, before);
    // 未知应收核销 → UnknownItem。
    assert!(matches!(
        co.write_off_receivable(
            &engine::accounting::OpenItemId("AR-999".to_string()),
            d("2030-02-01")
        ),
        Err(IndustrialError::Trade(
            engine::accounting::TradeLedgerError::UnknownItem { .. }
        ))
    ));
    assert_eq!(co, before);
}
