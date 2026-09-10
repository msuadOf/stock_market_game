//! 无库存交付 / 非法数量与单价 → 类型化拒绝，账套与子账字节不变。

use super::super::{acct, d, net_debit, yuan};
use super::fresh;
use engine::accounting::InventoryItemCode;
use engine::accounting::LedgerAccountId;
use engine::company::industrial::{IndustrialError, Settlement};
use engine::company::CounterpartyId;

fn cp(id: &str) -> CounterpartyId {
    CounterpartyId(id.to_string())
}

fn fg() -> InventoryItemCode {
    InventoryItemCode("FG".to_string())
}

#[test]
fn sale_without_inventory_is_rejected_state_unchanged() {
    let mut co = fresh();
    let before = co.clone();
    // 无库存交付：FG 子账为空 → InsufficientInventory（携带请求/可用数量）。
    let err = co
        .sell_credit(
            &cp("EXT-CUST"),
            fg(),
            8,
            yuan(100),
            d("2030-02-15"),
            d("2030-01-15"),
        )
        .unwrap_err();
    match err {
        IndustrialError::InsufficientInventory {
            item,
            requested,
            available,
        } => {
            assert_eq!(item, fg());
            assert_eq!(requested, 8);
            assert_eq!(available, 0);
        }
        other => panic!("expected InsufficientInventory, got {other:?}"),
    }
    assert_eq!(co, before);
    // 收入/应收/现金全部未动。
    assert_eq!(net_debit(&co, acct::REVENUE), yuan(0));
    assert_eq!(net_debit(&co, acct::AR), yuan(0));
    assert_eq!(net_debit(&co, acct::BANK), yuan(10_000));
}

#[test]
fn partial_inventory_sale_rejects_only_the_shortfall() {
    let mut co = fresh();
    co.purchase(
        &cp("EXT-SUPP"),
        fg(),
        LedgerAccountId(acct::FINISHED.to_string()),
        2,
        yuan(10),
        Settlement::Cash,
        d("2030-03-01"),
        d("2030-01-05"),
    )
    .expect("purchase 2 units");
    let before = co.clone();
    let err = co
        .sell_credit(
            &cp("EXT-CUST"),
            fg(),
            3,
            yuan(100),
            d("2030-02-15"),
            d("2030-01-15"),
        )
        .unwrap_err();
    assert!(matches!(
        err,
        IndustrialError::InsufficientInventory {
            requested: 3,
            available: 2,
            ..
        }
    ));
    assert_eq!(co, before);
    // 数量内（2 件）仍可正常销售——拒绝不产生持久损伤。
    co.sell_credit(
        &cp("EXT-CUST"),
        fg(),
        2,
        yuan(100),
        d("2030-02-15"),
        d("2030-01-16"),
    )
    .expect("sale within stock");
}

#[test]
fn production_and_purchase_reject_non_positive_quantities_and_prices() {
    let mut co = fresh();
    let before = co.clone();
    // 生产领料无库存 → 拒绝。
    assert!(matches!(
        co.produce(
            fg(),
            1,
            InventoryItemCode("FG2".to_string()),
            1,
            LedgerAccountId(acct::FINISHED.to_string()),
            yuan(1),
            d("2030-01-10"),
        ),
        Err(IndustrialError::InsufficientInventory { .. })
    ));
    // 零/负数量、零单价采购 → 拒绝。
    assert!(matches!(
        co.purchase(
            &cp("EXT-SUPP"),
            fg(),
            LedgerAccountId(acct::RAW.to_string()),
            0,
            yuan(10),
            Settlement::Cash,
            d("2030-03-01"),
            d("2030-01-05"),
        ),
        Err(IndustrialError::NonPositiveAmount { .. })
    ));
    assert!(matches!(
        co.purchase(
            &cp("EXT-SUPP"),
            fg(),
            LedgerAccountId(acct::RAW.to_string()),
            1,
            yuan(0),
            Settlement::Cash,
            d("2030-03-01"),
            d("2030-01-05"),
        ),
        Err(IndustrialError::NonPositiveAmount { .. })
    ));
    // 完工数量非正 → 拒绝。
    assert!(matches!(
        co.produce(
            fg(),
            1,
            InventoryItemCode("FG2".to_string()),
            0,
            LedgerAccountId(acct::FINISHED.to_string()),
            yuan(1),
            d("2030-01-10"),
        ),
        Err(IndustrialError::NonPositiveAmount { .. })
    ));
    assert_eq!(co, before);
}
