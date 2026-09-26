//! Committed tick transport. Sequence bounds use the half-open cursor interval
//! `(seq_from, seq_to]`; an empty frame has equal cursors.

use super::{DailyCandle, Event, Snapshot, StockCode, TradingPhase};
use std::collections::{BTreeMap, BTreeSet};
mod civil;
mod commit;
mod facts;
mod optional_u64;
mod replay;
pub use civil::*;
pub use commit::project_timeseries;
pub(crate) use facts::attach_facts_after;
pub(super) use facts::attach_facts_with_keys;
pub use facts::{attach_facts, EventFact};
pub use replay::*;

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct TickTimeseriesPayload {
    pub markets: BTreeMap<StockCode, super::MarketSnap>,
    pub active_daily_candles: BTreeMap<StockCode, DailyCandle>,
    pub closed_daily_candles: BTreeMap<StockCode, DailyCandle>,
    pub auction_points: BTreeMap<StockCode, Vec<AuctionPoint>>,
    pub continuous_points: BTreeMap<StockCode, ContinuousPoint>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct AuctionPoint {
    pub key: super::pipeline::EventStableKey,
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub tick: u64,
    pub kind: AuctionPointKind,
    pub phase: TradingPhase,
    pub indicative_price: Option<crate::Money>,
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub matched_volume: u64,
    #[ts(type = "number | null")]
    #[serde(with = "optional_u64")]
    pub imbalance: Option<u64>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub enum AuctionPointKind {
    Indication,
    Completion,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct ContinuousPoint {
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub tick: u64,
    pub phase: TradingPhase,
    pub last_price: crate::Money,
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub cumulative_volume: u64,
    #[serde(with = "super::js_safe_depth")]
    #[ts(type = "Array<[Money, number]>")]
    pub bids: Vec<(crate::Money, u64)>,
    #[serde(with = "super::js_safe_depth")]
    #[ts(type = "Array<[Money, number]>")]
    pub asks: Vec<(crate::Money, u64)>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct TickFrame {
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub tick: u64,
    pub events: Vec<Event>,
    pub facts: Vec<EventFact>,
    pub timeseries_payload: TickTimeseriesPayload,
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub seq_from: u64,
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub seq_to: u64,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct TickBatch {
    pub frames: Vec<TickFrame>,
    pub runtime_snapshot: Option<Snapshot>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProtocolError {
    #[error("event identity or canonical payload mismatch")]
    FactIdentity,
    #[error("update replay mutation or cursor gap")]
    ReplayMismatch,
    #[error("event sequence coverage is not exactly ({from}, {to}]")]
    SequenceCoverage { from: u64, to: u64 },
    #[error("tick batch must contain at least one frame")]
    EmptyBatch,
    #[error("tick frames are not consecutive at tick {tick}")]
    TickContinuity { tick: u64 },
    #[error("runtime snapshot does not match the final frame tick and seq")]
    SnapshotMismatch,
    #[error("civil update boundary metadata or closing history is inconsistent")]
    CivilBoundary,
}

pub fn validate_sequence(events: &[Event], from: u64, to: u64) -> Result<(), ProtocolError> {
    let invalid = || ProtocolError::SequenceCoverage { from, to };
    let width = to.checked_sub(from).ok_or_else(invalid)?;
    if u64::try_from(events.len()).map_err(|_| invalid())? != width {
        return Err(invalid());
    }
    let mut seen = BTreeSet::new();
    for event in events {
        let seq = event.seq();
        if seq <= from || seq > to || !seen.insert(seq) {
            return Err(invalid());
        }
    }
    Ok(())
}

impl TickFrame {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        facts::validate_facts(&self.facts, &self.events)?;
        validate_sequence(&self.events, self.seq_from, self.seq_to)
    }
}

impl TickBatch {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        let last = self.frames.last().ok_or(ProtocolError::EmptyBatch)?;
        for frame in &self.frames {
            frame.validate()?;
        }
        for adjacent in self.frames.windows(2) {
            let previous = &adjacent[0];
            let next = &adjacent[1];
            if previous.tick.checked_add(1) != Some(next.tick) {
                return Err(ProtocolError::TickContinuity { tick: next.tick });
            }
            if previous.seq_to != next.seq_from {
                return Err(ProtocolError::SequenceCoverage {
                    from: previous.seq_to,
                    to: next.seq_from,
                });
            }
        }
        if self
            .runtime_snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.tick != last.tick || snapshot.seq != last.seq_to)
        {
            return Err(ProtocolError::SnapshotMismatch);
        }
        Ok(())
    }
}
