//! Committed tick transport. Sequence bounds use the half-open cursor interval
//! `(seq_from, seq_to]`; an empty frame has equal cursors.

use super::{DailyCandle, Event, Snapshot, StockId, TradingPhase};
use std::collections::{BTreeMap, BTreeSet};
mod civil;
mod commit;
mod delta;
mod facts;
mod optional_u64;
mod player_orders;
pub use player_orders::{FrozenAsset, OrderVenue, PlayerWorkingOrder, SaveCandidateKey};
mod replay;
pub use civil::*;
pub use commit::project_timeseries;
pub(in crate::session) use commit::TickCandidateProjection;
pub use delta::{PlayerOrderDelta, PublicRuntimeState, RuntimeDelta};
pub use facts::{EventFact, SessionFactCursor};
pub use replay::*;

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct TickTimeseriesPayload {
    pub markets: BTreeMap<StockId, super::MarketSnap>,
    pub active_daily_candles: BTreeMap<StockId, DailyCandle>,
    pub closed_daily_candles: BTreeMap<StockId, DailyCandle>,
    pub auction_points: BTreeMap<StockId, Vec<AuctionPoint>>,
    pub continuous_points: BTreeMap<StockId, ContinuousPoint>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
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

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub enum AuctionPointKind {
    Indication,
    Completion,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
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

/// 一个已提交 tick 的发布帧。事件只随 `facts` 记录出现一次（架构项 #6）；
/// 需要纯事件视图的消费方在边界做 `facts.iter().map(|fact| &fact.event)` 投影，
/// wire 不再重复序列化独立 events 数组。
#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct TickFrame {
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub tick: u64,
    pub facts: Vec<EventFact>,
    pub timeseries_payload: TickTimeseriesPayload,
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub seq_from: u64,
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub seq_to: u64,
}

#[cfg(test)]
thread_local! {
    static FRAME_CLONES_FOR_TEST: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) };
}

#[cfg(test)]
pub(in crate::session::protocol) fn take_frame_clones_for_test() -> (usize, usize) {
    FRAME_CLONES_FOR_TEST.with(|count| count.replace((0, 0)))
}

#[cfg(test)]
impl Clone for TickFrame {
    fn clone(&self) -> Self {
        FRAME_CLONES_FOR_TEST.with(|count| {
            let (frames, facts) = count.get();
            count.set((frames + 1, facts + self.facts.len()));
        });
        Self { tick: self.tick, facts: self.facts.clone(), timeseries_payload: self.timeseries_payload.clone(),
            seq_from: self.seq_from, seq_to: self.seq_to }
    }
}

impl TickFrame {
    /// 事件投影：借用事实记录中的事件，不复制身份或第二份权威。
    pub fn events(&self) -> impl DoubleEndedIterator<Item = &Event> + ExactSizeIterator {
        self.facts.iter().map(|fact| &fact.event)
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct TickBatch {
    pub frames: Vec<TickFrame>,
    pub runtime_snapshot: Option<Snapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub runtime_delta: Option<RuntimeDelta>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProtocolError {
    #[error("event identity or sequence mismatch")]
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
    #[error(
        "runtime delta authority, cursor, player account or working order patch is inconsistent"
    )]
    RuntimeDeltaMismatch,
    #[error("civil update boundary metadata or closing history is inconsistent")]
    CivilBoundary,
}

pub fn validate_sequence(
    events: impl Iterator<Item = u64>,
    len: usize,
    from: u64,
    to: u64,
) -> Result<(), ProtocolError> {
    let invalid = || ProtocolError::SequenceCoverage { from, to };
    if from > crate::orderbook::js_safe_u64::MAX || to > crate::orderbook::js_safe_u64::MAX {
        return Err(invalid());
    }
    let width = to.checked_sub(from).ok_or_else(invalid)?;
    if u64::try_from(len).map_err(|_| invalid())? != width {
        return Err(invalid());
    }
    let mut seen = BTreeSet::new();
    for seq in events {
        if seq <= from || seq > to || !seen.insert(seq) {
            return Err(invalid());
        }
    }
    Ok(())
}

impl TickFrame {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.tick > crate::orderbook::js_safe_u64::MAX {
            return Err(ProtocolError::TickContinuity { tick: self.tick });
        }
        facts::validate_facts(&self.facts)?;
        validate_sequence(
            self.facts.iter().map(|fact| fact.event.seq()),
            self.facts.len(),
            self.seq_from,
            self.seq_to,
        )
    }
}

/// market/Civil 共用借用校验，不为检查连续性构造临时 TickBatch 或复制事实记录。
pub fn validate_frames(frames: &[TickFrame]) -> Result<&TickFrame, ProtocolError> {
    let last = frames.last().ok_or(ProtocolError::EmptyBatch)?;
    for frame in frames { frame.validate()?; }
    for adjacent in frames.windows(2) {
        let previous = &adjacent[0];
        let next = &adjacent[1];
        if previous.tick.checked_add(1) != Some(next.tick) { return Err(ProtocolError::TickContinuity { tick: next.tick }); }
        if previous.seq_to != next.seq_from {
            return Err(ProtocolError::SequenceCoverage { from: previous.seq_to, to: next.seq_from });
        }
    }
    Ok(last)
}

impl TickBatch {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        let last = validate_frames(&self.frames)?;
        if self
            .runtime_snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.tick != last.tick || snapshot.seq != last.seq_to)
        {
            return Err(ProtocolError::SnapshotMismatch);
        }
        if let Some(delta) = &self.runtime_delta {
            let first = self.frames.first().ok_or(ProtocolError::EmptyBatch)?;
            if self.runtime_snapshot.is_some()
                || delta.seq_from != first.seq_from
                || delta.seq_to != last.seq_to
                || delta.tick != last.tick
            {
                return Err(ProtocolError::RuntimeDeltaMismatch);
            }
            delta.validate()?;
        }
        Ok(())
    }
}
