use crate::CorpusError;
use engine::{Event, GameSession};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Serialize)]
pub struct Fact {
    identity: (u64, &'static str, String, String, u32),
    canonical_session_ordinal: Option<u32>,
    event: Event,
}

pub fn variant(event: &Event) -> (&'static str, String, String) {
    match event {
        Event::Trade {
            code, maker, taker, ..
        } => (
            "Trade",
            format!("Stock:{}", code.0),
            format!("{}:{}", maker.0, taker.0),
        ),
        Event::AuctionTick { code, .. } => {
            ("AuctionTick", format!("Stock:{}", code.0), code.0.clone())
        }
        Event::AuctionCompleted { code, .. } => (
            "AuctionCompleted",
            format!("Stock:{}", code.0),
            code.0.clone(),
        ),
        Event::PriceTick { code, .. } => ("PriceTick", format!("Stock:{}", code.0), code.0.clone()),
        Event::DayBoundary { day, .. } => ("DayBoundary", "Session".to_owned(), day.to_string()),
        Event::CivilDateAdvanced { settled_date, .. } => (
            "CivilDateAdvanced",
            "Session".to_owned(),
            format!("{settled_date:?}"),
        ),
        Event::CompanyDisclosurePublished { publication_id, .. } => (
            "CompanyDisclosurePublished",
            "Session".to_owned(),
            format!("{publication_id:?}"),
        ),
        Event::ResourceLimit { resource, .. } => (
            "ResourceLimit",
            "Session".to_owned(),
            format!("{resource:?}"),
        ),
        Event::OrderAccepted { account, id, .. } => (
            "OrderAccepted",
            format!("Account:{}", account.0),
            id.0.to_string(),
        ),
        Event::OrderCanceled { account, id, .. } => (
            "OrderCanceled",
            format!("Account:{}", account.0),
            id.0.to_string(),
        ),
        Event::IntentRejected { account, code, .. } => (
            "IntentRejected",
            format!("Account:{}", account.0),
            code.0.clone(),
        ),
        Event::SettlementError { account, code, .. } => (
            "SettlementError",
            format!("Account:{}", account.0),
            code.0.clone(),
        ),
    }
}

pub fn project(tick: u64, events: Vec<Event>) -> Result<Vec<Fact>, CorpusError> {
    let mut scopes = BTreeMap::new();
    let mut session_ordinal = 0_u32;
    let mut facts = Vec::new();
    for event in events {
        let (tag, entity, stable) = variant(&event);
        let ordinal = scopes
            .entry((tag, entity.clone(), stable.clone()))
            .or_insert(0_u32);
        let canonical_session_ordinal = match &event {
            Event::CivilDateAdvanced { .. }
            | Event::CompanyDisclosurePublished { .. }
            | Event::ResourceLimit { .. } => {
                let value = session_ordinal;
                session_ordinal = session_ordinal
                    .checked_add(1)
                    .ok_or_else(|| CorpusError::Invariant("session ordinal overflow".to_owned()))?;
                Some(value)
            }
            Event::Trade { .. }
            | Event::AuctionTick { .. }
            | Event::AuctionCompleted { .. }
            | Event::PriceTick { .. }
            | Event::DayBoundary { .. }
            | Event::OrderAccepted { .. }
            | Event::OrderCanceled { .. }
            | Event::IntentRejected { .. }
            | Event::SettlementError { .. } => None,
        };
        facts.push(Fact {
            identity: (tick, tag, entity, stable, *ordinal),
            canonical_session_ordinal,
            event,
        });
        *ordinal = ordinal
            .checked_add(1)
            .ok_or_else(|| CorpusError::Invariant("event ordinal overflow".to_owned()))?;
    }
    Ok(facts)
}

pub fn step(session: &mut GameSession) -> Result<Value, CorpusError> {
    let seq_from = session.seq() + 1;
    let events = session.step()?;
    let snapshot = session.snapshot();
    let save = session.save()?;
    let mut series = BTreeMap::new();
    for event in &events {
        match event {
            Event::AuctionTick { code, .. }
            | Event::AuctionCompleted { code, .. }
            | Event::PriceTick { code, .. } => {
                let (name, _, _) = variant(event);
                let mut data = serde_json::to_value(event)?;
                if let Some(payload) = data.get_mut(name).and_then(Value::as_object_mut) {
                    payload.remove("seq");
                }
                series.insert(format!("{}:{name}", code.0), data);
            }
            Event::DayBoundary { .. } => {
                let mut data = serde_json::to_value(event)?;
                if let Some(payload) = data.get_mut("DayBoundary").and_then(Value::as_object_mut) {
                    payload.remove("seq");
                }
                series.insert("DayBoundary".to_owned(), data);
            }
            Event::Trade { .. }
            | Event::CivilDateAdvanced { .. }
            | Event::CompanyDisclosurePublished { .. }
            | Event::ResourceLimit { .. }
            | Event::OrderAccepted { .. }
            | Event::OrderCanceled { .. }
            | Event::IntentRejected { .. }
            | Event::SettlementError { .. } => {}
        }
    }
    Ok(
        json!({ "kind": "TickFrame", "tick": session.tick(), "seq_from": seq_from,
            "seq_to": session.seq(), "events": project(session.tick(), events)?,
            "timeseries_payload": { "points": series, "markets": snapshot.markets,
                "active_daily_candles": snapshot.active_daily_candles, "daily_candles": snapshot.daily_candles },
            "snapshot": snapshot, "orders": { "resting": save.resting_orders, "auction": save.auction_orders },
            "rng_state": save.rng_state.to_string(), "next_order_id": save.next_order_id.to_string(),
            "plans": save.plans, "pending_plan_events": save.pending_plan_events,
            "strategy_profiles": save.strategy_profiles, "pending_player": save.pending_player,
        }),
    )
}
