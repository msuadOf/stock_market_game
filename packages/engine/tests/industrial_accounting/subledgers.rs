//! 共享子账（accounting/{inventory,fixed_assets,receivables}）单元金样——
//! 任务 9–11（银行/保险/地产）复用同一接口，此处锁定其语义契约：
//! 移动加权平均守恒、直线折旧守恒、开项部分核销/账龄/逾期/核销。

use super::{d, yuan};
use engine::accounting::{
    AccountingAmount, FixedAssetCode, FixedAssetError, InventoryError, InventoryItemCode,
    LedgerAccountId, OpenItemId, TradeLedgerError,
};
use engine::calendar::CivilDate;

fn item(code: &str) -> InventoryItemCode {
    InventoryItemCode(code.to_string())
}

fn asset(code: &str) -> FixedAssetCode {
    FixedAssetCode(code.to_string())
}

fn acct(code: &str) -> LedgerAccountId {
    LedgerAccountId(code.to_string())
}

#[test]
fn inventory_moving_weighted_average_conserves_total_cost() {
    let mut inv = engine::accounting::InventoryLedger::default();
    // 两批入库：3 件 100 分 + 2 件 350 分 = 5 件 450 分（加权单价 90 分）。
    inv.receipt(
        item("A"),
        acct("1403"),
        3,
        AccountingAmount::from_cents(100),
    )
    .expect("receipt 1");
    inv.receipt(
        item("A"),
        acct("1403"),
        2,
        AccountingAmount::from_cents(350),
    )
    .expect("receipt 2");
    // 发出 4 件 = rhe(450×4/5) = 360 分；preview 与 apply 一致。
    assert_eq!(
        inv.preview_issue(&item("A"), 4).expect("preview"),
        AccountingAmount::from_cents(360)
    );
    assert_eq!(
        inv.apply_issue(&item("A"), 4).expect("apply"),
        AccountingAmount::from_cents(360)
    );
    assert_eq!(inv.quantity(&item("A")), 1);
    assert_eq!(inv.total_cost(&item("A")), AccountingAmount::from_cents(90));
    // 最后一件带走全部剩余成本（守恒：Σ发出 + 期末 = 450 分）。
    assert_eq!(
        inv.apply_issue(&item("A"), 1).expect("last issue"),
        AccountingAmount::from_cents(90)
    );
    assert_eq!(inv.total_cost(&item("A")), AccountingAmount::ZERO);
}

#[test]
fn inventory_rejects_illegal_states_without_mutation() {
    let mut inv = engine::accounting::InventoryLedger::default();
    inv.receipt(item("A"), acct("1403"), 10, yuan(100))
        .expect("receipt");
    let before = inv.clone();
    assert!(matches!(
        inv.apply_issue(&item("A"), 11),
        Err(InventoryError::InsufficientQuantity { .. })
    ));
    assert!(matches!(
        inv.preview_issue(&item("A"), 0),
        Err(InventoryError::NonPositiveQuantity { .. })
    ));
    assert!(matches!(
        inv.receipt(item("A"), acct("1403"), 1, AccountingAmount::ZERO),
        Err(InventoryError::NonPositiveCost { .. })
    ));
    assert_eq!(inv, before);
    // 同一项目第二科目入库 → 类型化拒绝（科目绑定在首次入库时固定）。
    inv.receipt(item("B"), acct("1405"), 1, yuan(1))
        .expect("new item on 1405");
    assert!(matches!(
        inv.receipt(item("B"), acct("1403"), 1, yuan(1)),
        Err(InventoryError::AccountMismatch { .. })
    ));
}

#[test]
fn receivables_partial_apply_aging_overdue_and_write_off() {
    let mut ledger = engine::accounting::TradeOpenLedger::default();
    let id1 = OpenItemId("AR-1".to_string());
    let id2 = OpenItemId("AR-2".to_string());
    ledger
        .open(
            id1.clone(),
            "CUST",
            d("2030-01-01"),
            d("2030-02-01"),
            yuan(100),
        )
        .expect("open 1");
    ledger
        .open(
            id2.clone(),
            "CUST",
            d("2029-12-01"),
            d("2030-01-15"),
            yuan(50),
        )
        .expect("open 2");
    // 部分核销 40：剩 60；超额核销 → 类型化拒绝。
    assert_eq!(ledger.apply(&id1, yuan(40)).expect("partial"), yuan(60));
    assert!(matches!(
        ledger.apply(&id1, yuan(61)),
        Err(TradeLedgerError::OverApplication { .. })
    ));
    assert_eq!(ledger.total_open().expect("total"), yuan(110));
    // 账龄（as_of = 2030-01-31）：AR-1 开立 30 天（≤30 桶，余 60）；
    // AR-2 开立 61 天（61–90 桶，50）。
    let buckets = ledger.aging_buckets(d("2030-01-31")).expect("aging");
    assert_eq!(buckets[0], yuan(60));
    assert_eq!(buckets[1], AccountingAmount::ZERO);
    assert_eq!(buckets[2], yuan(50));
    assert_eq!(buckets[3], AccountingAmount::ZERO);
    // 逾期（as_of = 2030-01-16）：AR-2 到期 01-15 < 01-16 → 逾期；AR-1 未到期。
    let overdue: Vec<&OpenItemId> = ledger
        .overdue(d("2030-01-16"))
        .iter()
        .map(|(id, _)| *id)
        .collect();
    assert_eq!(overdue, vec![&id2]);
    // 核销 AR-1（余额 60）→ 清零并计入核销合计。
    assert_eq!(ledger.write_off(&id1).expect("write off"), yuan(60));
    assert_eq!(ledger.written_off_total().expect("written off"), yuan(60));
    // 已清空项目再操作 → 类型化拒绝；到期早于开立 → 拒绝；重复 id → 拒绝。
    assert!(matches!(
        ledger.apply(&id1, yuan(1)),
        Err(TradeLedgerError::ItemCleared { .. })
    ));
    assert!(matches!(
        ledger.open(
            OpenItemId("AR-3".to_string()),
            "CUST",
            d("2030-01-05"),
            d("2030-01-01"),
            yuan(1)
        ),
        Err(TradeLedgerError::DueBeforeOpen { .. })
    ));
    assert!(matches!(
        ledger.open(
            id2.clone(),
            "CUST",
            d("2030-01-05"),
            d("2030-02-05"),
            yuan(1)
        ),
        Err(TradeLedgerError::DuplicateItem { .. })
    ));
    let _ = CivilDate::from_iso("2030-01-01").expect("date");
}

#[test]
fn fixed_assets_register_policy_and_conservation() {
    let mut register = engine::accounting::FixedAssetRegister::default();
    // 非法计量政策：寿命 0 月、残值 > 成本、零成本。
    assert!(matches!(
        register.register(asset("M"), yuan(1_000), AccountingAmount::ZERO, 0),
        Err(FixedAssetError::AssetPolicyInvalid { .. })
    ));
    assert!(matches!(
        register.register(asset("M"), yuan(100), yuan(200), 12),
        Err(FixedAssetError::AssetPolicyInvalid { .. })
    ));
    assert!(matches!(
        register.register(
            asset("M"),
            AccountingAmount::ZERO,
            AccountingAmount::ZERO,
            12
        ),
        Err(FixedAssetError::AssetPolicyInvalid { .. })
    ));
    register
        .register(asset("M"), yuan(1_000), AccountingAmount::ZERO, 2)
        .expect("register");
    assert!(matches!(
        register.register(asset("M"), yuan(1_000), AccountingAmount::ZERO, 2),
        Err(FixedAssetError::DuplicateAsset { .. })
    ));
    // 1000 元 / 2 月：m1 = rhe(100_000/2) = 50_000 分；m2 = 50_000 分；Σ = 成本。
    assert_eq!(
        register.apply_depreciation(&asset("M")).expect("m1"),
        yuan(500)
    );
    assert_eq!(
        register.apply_depreciation(&asset("M")).expect("m2"),
        yuan(500)
    );
    assert!(matches!(
        register.apply_depreciation(&asset("M")),
        Err(FixedAssetError::FullyDepreciated { .. })
    ));
    // 减值上限 = 账面 − 残值 = 0 → 任何减值拒绝。
    assert!(matches!(
        register.apply_impairment(&asset("M"), yuan(1)),
        Err(FixedAssetError::ImpairmentBeyondFloor { .. })
    ));
    let entry = register.get(&asset("M")).expect("asset");
    assert_eq!(
        entry.carrying_amount().expect("carrying"),
        AccountingAmount::ZERO
    );
}
