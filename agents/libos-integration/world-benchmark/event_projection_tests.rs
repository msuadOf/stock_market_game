use super::event_projection::{check_sequence, same_local_order};
use engine::{Event, Money, RejectionReason, StockId, TradingPhase};
use engine::orderbook::{AccountId, OrderId, Side};

fn accepted(seq: u64, stock: u32, account: u64, id: u64) -> Event {
    Event::OrderAccepted {seq, account:AccountId(account), code:StockId(stock), id:OrderId(id), side:Side::Buy, price:Money::from_cents(100), remaining_qty:100}
}

#[test]
fn 只允许无关股票与账户的事件交错变化() {
    let left = vec![accepted(1,0,10,1),accepted(2,1,20,2)];
    let right = vec![accepted(1,1,20,2),accepted(2,0,10,1)];
    assert!(same_local_order(&left, &right).unwrap());
    let same_stock = vec![accepted(1,0,20,2),accepted(2,0,10,1)];
    let same_stock_left = vec![accepted(1,0,10,1),accepted(2,0,20,2)];
    assert!(!same_local_order(&same_stock_left, &same_stock).unwrap());
    let same_account = vec![accepted(1,1,10,2),accepted(2,0,10,1)];
    let same_account_left = vec![accepted(1,0,10,1),accepted(2,1,10,2)];
    assert!(!same_local_order(&same_account_left, &same_account).unwrap());
}

#[test]
fn 全局事件形成屏障且序号必须有效() {
    let boundary = |seq| Event::DayBoundary {seq, day:1, closed_daily_candles:Default::default()};
    let left = vec![accepted(1,0,10,1),boundary(2),accepted(3,1,20,2)];
    let right = vec![accepted(1,1,20,2),boundary(2),accepted(3,0,10,1)];
    assert!(!same_local_order(&left, &right).unwrap());
    assert!(same_local_order(&[accepted(2,0,10,1),accepted(1,1,20,2)], &right).is_err());
    assert!(!same_local_order(&[accepted(1,0,10,1)], &[accepted(2,0,10,1)]).unwrap());
    let mut previous = None;
    check_sequence(&[accepted(10,0,10,1)], &mut previous).unwrap();
    check_sequence(&[], &mut previous).unwrap();
    assert!(check_sequence(&[accepted(10,1,20,2)], &mut previous).is_err());
}

#[test]
fn 成交的买卖两账户都保留局部顺序() {
    let trade = |seq| Event::Trade {seq, code:StockId(0), price:Money::from_cents(100), qty:100, maker:AccountId(10), taker:AccountId(20)};
    for account in [10,20] {
        let left = vec![trade(1), accepted(2,1,account,5)];
        let right = vec![accepted(1,1,account,5), trade(2)];
        assert!(!same_local_order(&left, &right).unwrap());
    }
}

#[test]
fn 证券事件字段改变与同证券重排均被拒绝() {
    let variants = |seq| vec![
        Event::OrderCanceled {seq, code:StockId(0), account:AccountId(10), id:OrderId(7), remaining_qty:100},
        Event::IntentRejected {seq, code:StockId(0), account:AccountId(10), reason:RejectionReason::InsufficientCash},
        Event::SettlementError {seq, code:StockId(0), account:AccountId(10), reason:"验证".to_owned()},
        Event::AuctionTick {seq, tick:1, phase:TradingPhase::CallAuction, code:StockId(0), indicative_price:None, matched_volume:0, imbalance:0},
        Event::AuctionCompleted {seq, tick:1, phase:TradingPhase::CallAuction, code:StockId(0), clearing_price:None, matched_volume:0},
    ];
    for (left_event, right_event) in variants(1).into_iter().zip(variants(2)) {
        let left = vec![left_event, accepted(2,0,20,5)];
        let right = vec![accepted(1,0,20,5), right_event];
        assert!(!same_local_order(&left, &right).unwrap());
    }
    let changed = Event::OrderAccepted {seq:1, account:AccountId(10), code:StockId(0), id:OrderId(1), side:Side::Buy, price:Money::from_cents(101), remaining_qty:100};
    assert!(!same_local_order(&[accepted(1,0,10,1)], &[changed]).unwrap());
    assert!(same_local_order(&[accepted(1,0,10,1),accepted(1,1,20,2)], &[]).is_err());
}

#[test]
fn 多个连续全局屏障保持顺序() {
    let boundary = |seq, day| Event::DayBoundary {seq, day, closed_daily_candles:Default::default()};
    let left = vec![boundary(1,1),boundary(2,2)];
    let right = vec![boundary(1,2),boundary(2,1)];
    assert!(!same_local_order(&left, &right).unwrap());
}
