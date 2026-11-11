//! 仅比较可见事件的同股、同账户局部顺序；不代替真实 receipt 或撮合事实。
use engine::Event;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Default, PartialEq)]
struct Segment {
    stocks: BTreeMap<u32, Vec<Value>>,
    accounts: BTreeMap<u64, Vec<Value>>,
}

#[derive(PartialEq)]
enum Piece { Local(Segment), Barrier(Value) }

/// 检查跨 tick 严格递增，空 tick 保留之前的序号。
pub fn check_sequence(events: &[Event], previous: &mut Option<u64>) -> Result<(), String> {
    for event in events {
        let seq = event.seq();
        if previous.is_some_and(|value| seq <= value) { return Err(format!("事件 seq 未严格递增：{previous:?} -> {seq}")); }
        *previous = Some(seq);
    }
    Ok(())
}

fn projection(events: &[Event]) -> Result<Vec<Piece>, String> {
    let mut pieces = Vec::new();
    let mut segment = Segment::default();
    for event in events {
        let scope = match event {
            Event::Trade {code, maker, taker, ..} => Some((code.0, vec![maker.0, taker.0])),
            Event::OrderAccepted {code, account, ..} | Event::OrderCanceled {code, account, ..}
            | Event::IntentRejected {code, account, ..} | Event::SettlementError {code, account, ..} => Some((code.0, vec![account.0])),
            Event::AuctionTick {code, ..} | Event::AuctionCompleted {code, ..} | Event::PriceTick {code, ..} => Some((code.0, Vec::new())),
            Event::DayBoundary {..} | Event::CivilDateAdvanced {..} | Event::CompanyDisclosurePublished {..} => None,
        };
        let mut value = serde_json::to_value(event).map_err(|error| format!("事件序列化失败：{error}"))?;
        let object = value.as_object_mut().ok_or("事件不是对象")?;
        if object.len() != 1 { return Err("事件变体数量错误".into()); }
        let payload = object.values_mut().next().ok_or("事件变体缺失")?.as_object_mut().ok_or("事件内容不是对象")?;
        if payload.remove("seq").is_none() { return Err("事件缺少 seq".into()); }
        if let Some((stock, mut accounts)) = scope {
            segment.stocks.entry(stock).or_default().push(value.clone());
            accounts.sort_unstable(); accounts.dedup();
            for account in accounts { segment.accounts.entry(account).or_default().push(value.clone()); }
        } else {
            pieces.push(Piece::Local(std::mem::take(&mut segment)));
            pieces.push(Piece::Barrier(value));
        }
    }
    pieces.push(Piece::Local(segment));
    Ok(pieces)
}

/// 全局屏障以及同股和同账户的可见事件顺序均须一致；跨股独立事件允许交错不同。
pub fn same_local_order(left: &[Event], right: &[Event]) -> Result<bool, String> {
    check_sequence(left, &mut None)?;
    check_sequence(right, &mut None)?;
    Ok(projection(left)? == projection(right)?)
}
