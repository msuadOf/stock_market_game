//! `Entity` 资源实体结构契约测试（数据类型，随类型存在即绿）。

use super::Entity;

#[test]
fn cash_proceeds_carries_local_and_amount() {
    let e = Entity::CashProceeds {
        local: 7,
        amount: 123_456,
    };
    assert_eq!(
        e,
        Entity::CashProceeds {
            local: 7,
            amount: 123_456
        }
    );
}

#[test]
fn cash_refund_carries_local_and_amount() {
    let e = Entity::CashRefund {
        local: 8,
        amount: 99,
    };
    assert_eq!(
        e,
        Entity::CashRefund {
            local: 8,
            amount: 99
        }
    );
}

#[test]
fn bought_and_refunded_shares_carry_local_and_qty() {
    let bought = Entity::BoughtShares { local: 9, qty: 300 };
    let refunded = Entity::RefundedShares { local: 9, qty: 55 };
    assert_eq!(bought, Entity::BoughtShares { local: 9, qty: 300 });
    assert_eq!(refunded, Entity::RefundedShares { local: 9, qty: 55 });
    assert_ne!(bought, refunded);
}

#[test]
fn closed_is_zero_value_marker_for_inflight_registry() {
    // 零额终结也必须发 Closed，否则组端在途登记（foreign_active）泄漏。
    let e = Entity::Closed {
        local: 11,
        order_serial: 5,
    };
    assert_eq!(
        e,
        Entity::Closed {
            local: 11,
            order_serial: 5,
        }
    );
    // Copy 契约：实体可随批次缓冲复制（报告/审计路径）。
    let copied = e;
    assert_eq!(copied, e);
}

#[test]
fn entity_distinguishes_all_five_variants() {
    // 五类实体齐备且互不相等：成交净额/退款/买股/退股/终结标记。
    let variants = [
        Entity::CashProceeds {
            local: 0,
            amount: 1,
        },
        Entity::CashRefund {
            local: 0,
            amount: 1,
        },
        Entity::BoughtShares { local: 0, qty: 1 },
        Entity::RefundedShares { local: 0, qty: 1 },
        Entity::Closed {
            local: 0,
            order_serial: 0,
        },
    ];
    for (i, a) in variants.iter().enumerate() {
        for (j, b) in variants.iter().enumerate() {
            assert_eq!((a == b), (i == j), "变体 {i}/{j} 应仅在同下标时相等");
        }
    }
}
