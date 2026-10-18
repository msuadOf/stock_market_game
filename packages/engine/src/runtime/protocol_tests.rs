//! `DeskIn`/`RoutedPrice`/`DeskBatch` 协议原语测试。
//!
//! 行为测试（verify 防御核对）走红绿；结构测试（字段/trait 契约）随类型存在即绿，
//! 作为契约文档保留。

use super::{DeskBatch, DeskBatchMismatch, DeskIn, OrderRef, Owner, RoutedPrice, StockIdx};
use crate::orderbook::Side;
use std::collections::HashMap;

fn sample_batch(stock: StockIdx, tick: u64, round: u32) -> DeskBatch {
    DeskBatch {
        stock,
        tick,
        round,
        entities: Vec::new(),
        fills: Vec::new(),
        receipts: Vec::new(),
    }
}

// ── DeskBatch 防御字段核对（Sol 终审：tick/round 冗余防御，错配=显式失败）──

#[test]
fn desk_batch_verify_accepts_matching_stock_tick_round() {
    let batch = sample_batch(7, 100, 2);
    assert_eq!(batch.verify(7, 100, 2), Ok(()));
}

#[test]
fn desk_batch_verify_rejects_stock_mismatch() {
    let batch = sample_batch(8, 100, 2);
    assert_eq!(
        batch.verify(7, 100, 2),
        Err(DeskBatchMismatch::Stock {
            expected: 7,
            actual: 8
        })
    );
}

#[test]
fn desk_batch_verify_rejects_tick_mismatch() {
    let batch = sample_batch(7, 99, 2);
    assert_eq!(
        batch.verify(7, 100, 2),
        Err(DeskBatchMismatch::Tick {
            expected: 100,
            actual: 99
        })
    );
}

#[test]
fn desk_batch_verify_rejects_round_mismatch() {
    let batch = sample_batch(7, 100, 1);
    assert_eq!(
        batch.verify(7, 100, 2),
        Err(DeskBatchMismatch::Round {
            expected: 2,
            actual: 1
        })
    );
}

#[test]
fn desk_batch_mismatch_messages_carry_expected_and_actual() {
    // 错误信息必须带 expected/actual 数值（显式失败可诊断，不静默）。
    let err = DeskBatchMismatch::Tick {
        expected: 100,
        actual: 99,
    };
    let msg = err.to_string();
    assert!(msg.contains("expected=100"), "消息缺 expected：{msg}");
    assert!(msg.contains("actual=99"), "消息缺 actual：{msg}");
    assert!(msg.contains("tick"), "消息缺字段名 tick：{msg}");
}

// ── 结构契约 ──

#[test]
fn desk_in_place_carries_owner_side_price_qty_purse() {
    let op = DeskIn::Place {
        owner: (3, 42),
        side: Side::Buy,
        price: RoutedPrice::Fixed(1_055),
        qty: 200,
        purse: 211_000,
    };
    let DeskIn::Place {
        owner,
        side,
        price,
        qty,
        purse,
    } = op
    else {
        panic!("应构造出 Place 变体");
    };
    assert_eq!(owner, (3, 42));
    assert_eq!(side, Side::Buy);
    assert_eq!(price, RoutedPrice::Fixed(1_055));
    assert_eq!(qty, 200);
    assert_eq!(purse, 211_000);
}

#[test]
fn desk_in_cancel_all_and_cancel_carry_owner() {
    let cancel_all = DeskIn::CancelAll { owner: (0, 9) };
    let cancel = DeskIn::Cancel {
        owner: (0, 9),
        order: (5, 17),
    };
    assert_eq!(cancel_all, DeskIn::CancelAll { owner: (0, 9) });
    assert_eq!(
        cancel,
        DeskIn::Cancel {
            owner: (0, 9),
            order: (5, 17)
        }
    );
}

#[test]
fn routed_price_covers_fixed_highest_lowest_market() {
    // 四个符号变体齐备：限价/本方上限/本方下限/市价（涨跌停保护）。
    assert_eq!(RoutedPrice::Fixed(-1), RoutedPrice::Fixed(-1));
    assert_ne!(RoutedPrice::Highest, RoutedPrice::Lowest);
    assert_ne!(RoutedPrice::Lowest, RoutedPrice::Market);
    assert_ne!(RoutedPrice::Market, RoutedPrice::Highest);
}

#[test]
fn owner_is_copy_and_hashable_for_zero_lookup_ledger() {
    // 组端零查找落账依赖 Owner 可 Copy + 作 HashMap key。
    let owner: Owner = (2, 1234);
    let copied = owner;
    let mut map = HashMap::new();
    map.insert(owner, "cash");
    assert_eq!(map.get(&copied), Some(&"cash"));
}

#[test]
fn order_ref_orders_by_stock_then_serial() {
    // OrderRef = (StockIdx, u64) 复合标识，字典序：先股后本地序号。
    let a: OrderRef = (5, 3);
    let b: OrderRef = (6, 1);
    let c: OrderRef = (5, 4);
    assert!(a < b, "股索引优先：(5,3) 应小于 (6,1)");
    assert!(a < c, "同股比序号：(5,3) 应小于 (5,4)");
}

#[test]
fn desk_batch_aggregates_entities_fills_receipts() {
    // 批次为聚合 Vec（不逐条发消息），三个通道分立：资源转移 / 成交事实 / 回执。
    let mut batch = sample_batch(7, 100, 0);
    batch.entities.push(super::Entity::Closed {
        local: 1,
        order_serial: 3,
    });
    assert_eq!(batch.entities.len(), 1);
    assert_eq!(batch.fills.len(), 0);
    assert_eq!(batch.receipts.len(), 0);
    assert_eq!(batch.stock, 7);
    assert_eq!(batch.tick, 100);
    assert_eq!(batch.round, 0);
}

#[test]
fn desk_batch_verify_tick_round_is_group_side_check() {
    // 组端核对（组无法预知发方交易部）：只核对 tick/轮归属。
    let batch = sample_batch(9, 100, 2);
    assert_eq!(batch.verify_tick_round(100, 2), Ok(()));
    assert_eq!(
        batch.verify_tick_round(101, 2),
        Err(DeskBatchMismatch::Tick {
            expected: 101,
            actual: 100
        })
    );
    assert_eq!(
        batch.verify_tick_round(100, 3),
        Err(DeskBatchMismatch::Round {
            expected: 3,
            actual: 2
        })
    );
    // 与全量 verify 的分工：stock 由发送面已知的拓扑用全量，组端共享收件箱用本方法。
    assert_eq!(batch.verify(9, 100, 2), Ok(()));
}
