//! `FillFact` 防御校验（红绿行为测试）与 `DeskReceipt`/`ReceiptKind` 结构契约。

use super::{DeskReceipt, FillFact, FillFactError, ReceiptKind};
use crate::orderbook::Side;

fn sample_fill(
    side: Side,
    price: i64,
    qty: u32,
    commission: i64,
    stamp_tax: i64,
    transfer_fee: i64,
) -> FillFact {
    FillFact {
        order: (5, 17),
        owner: (2, 33),
        side,
        price,
        qty,
        commission,
        stamp_tax,
        transfer_fee,
    }
}

// ── FillFact::validate 行为（红绿）──

#[test]
fn fill_fact_validate_accepts_sell_with_stamp_tax() {
    // 卖腿：佣金+印花税+过户费齐备，数值域全部合法。
    let fill = sample_fill(Side::Sell, 1_050, 200, 5, 105, 2);
    assert_eq!(fill.validate(), Ok(()));
}

#[test]
fn fill_fact_validate_accepts_buy_without_stamp_tax() {
    // 买腿：印花税必须为 0，佣金/过户费双边存在。
    let fill = sample_fill(Side::Buy, 1_050, 200, 5, 0, 2);
    assert_eq!(fill.validate(), Ok(()));
}

#[test]
fn fill_fact_validate_rejects_buy_with_stamp_tax() {
    // A 股印花税仅向卖方收取；买腿出现印花税=实现 bug，显式失败。
    let fill = sample_fill(Side::Buy, 1_050, 200, 5, 105, 2);
    assert_eq!(
        fill.validate(),
        Err(FillFactError::StampTaxOnBuy { stamp_tax: 105 })
    );
}

#[test]
fn fill_fact_validate_rejects_zero_qty() {
    let fill = sample_fill(Side::Sell, 1_050, 0, 5, 0, 2);
    assert_eq!(fill.validate(), Err(FillFactError::InvalidQty { qty: 0 }));
}

#[test]
fn fill_fact_validate_rejects_non_positive_price() {
    let zero = sample_fill(Side::Sell, 0, 100, 5, 0, 2);
    assert_eq!(
        zero.validate(),
        Err(FillFactError::InvalidPrice { price: 0 })
    );
    let negative = sample_fill(Side::Sell, -1, 100, 5, 0, 2);
    assert_eq!(
        negative.validate(),
        Err(FillFactError::InvalidPrice { price: -1 })
    );
}

#[test]
fn fill_fact_validate_rejects_negative_fees() {
    let commission = sample_fill(Side::Sell, 1_050, 100, -1, 0, 2);
    assert_eq!(
        commission.validate(),
        Err(FillFactError::NegativeCommission { commission: -1 })
    );
    let stamp = sample_fill(Side::Sell, 1_050, 100, 5, -1, 2);
    assert_eq!(
        stamp.validate(),
        Err(FillFactError::NegativeStampTax { stamp_tax: -1 })
    );
    let transfer = sample_fill(Side::Sell, 1_050, 100, 5, 0, -1);
    assert_eq!(
        transfer.validate(),
        Err(FillFactError::NegativeTransferFee { transfer_fee: -1 })
    );
}

// ── DeskReceipt / ReceiptKind 结构契约 ──

#[test]
fn desk_receipt_carries_order_owner_kind() {
    let receipt = DeskReceipt {
        order: (5, 17),
        owner: (2, 33),
        kind: ReceiptKind::AlreadyFilled,
    };
    assert_eq!(receipt.order, (5, 17));
    assert_eq!(receipt.owner, (2, 33));
    assert_eq!(receipt.kind, ReceiptKind::AlreadyFilled);
}

#[test]
fn receipt_kind_covers_six_business_rejections() {
    // 六类显式业务回执：带外价/非整手/超上限/相位拒收/不可撤窗口/已全成。
    let kinds = [
        ReceiptKind::PriceOutOfBand,
        ReceiptKind::NotRoundLot,
        ReceiptKind::ExceedsOrderCap,
        ReceiptKind::PhaseRejected,
        ReceiptKind::CancelWindowClosed,
        ReceiptKind::AlreadyFilled,
    ];
    for (i, a) in kinds.iter().enumerate() {
        for (j, b) in kinds.iter().enumerate() {
            assert_eq!((a == b), (i == j), "ReceiptKind {i}/{j} 应仅在同下标时相等");
        }
    }
}

#[test]
fn fill_fact_is_copy_for_batch_aggregation() {
    // 事实随聚合批次上行（Vec<FillFact>），Copy 契约供审计/投影复用。
    let fill = sample_fill(Side::Buy, 1_050, 200, 5, 0, 2);
    let copied = fill;
    assert_eq!(fill, copied);
    assert_eq!(fill.order, (5, 17));
    assert_eq!(fill.owner, (2, 33));
    assert_eq!(fill.side, Side::Buy);
}

// Sol 二审修复集 4：拍卖实际成交量的事件面投影（事实面的实际成交
// matched_volume 逐字投影——选择量上界不得泄漏到公开口径）。
// 事件批内一致性的世界级锚定见 desk_semantics_tests 的
// p1_auction_self_trade…（实际 100 股）与 sol2_abort_replay…（空档
// 修复后 100 股）——runtime 驱动器 fixture 的 NPC 不在竞价窗交叉，
// 无 AuctionCompleted 事件，批量层无真实锚点（如实登记，不造空转断言）。
#[test]
fn sol4_auction_fact_projects_actual_volume() {
    use super::protocol::DeskAuctionFact;
    let fact = DeskAuctionFact {
        stock: 0,
        tick: 10,
        phase: crate::TradingPhase::CallAuction,
        indicative_price: Some(1000),
        matched_volume: 100,
        completed: true,
        imbalance: 7,
    };
    let mut seq = 5u64;
    let events = super::event_bridge::project_auction_events(&[fact], &mut seq);
    match events.as_slice() {
        [crate::Event::AuctionCompleted {
            matched_volume,
            clearing_price: Some(p),
            seq: event_seq,
            ..
        }] => {
            assert_eq!(*matched_volume, 100, "实际成交量逐字投影（P1-2 事件面）");
            assert_eq!(p.cents(), 1000, "清算价投影");
            // step_frame 接轨件 2：先加后取（对齐旧管线 collect_events
            //——第一条取得游标+1，游标语义=最后已用）。
            assert_eq!(*event_seq, 6, "seq 从游标+1 分配（先加后取）");
        }
        other => panic!("恰投影一条 AuctionCompleted：{other:?}"),
    }
    assert_eq!(seq, 6, "游标推进至最后已用");
}
