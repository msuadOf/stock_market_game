//! 每客户端 Publisher 的帧缓冲与行情压缩。
//!
//! 引擎事件序列仍是权威历史；这里只允许覆盖同一 UI 采样槽中的行情事件。
//! 委托、错误、集合竞价结束与日界事件永不因帧覆盖而消失；成交带与 UI 一致只保留最新 100 条。

use std::collections::{HashMap, HashSet};

use engine::{Event, Snapshot, StockCode};
use serde::Serialize;

use crate::actor::EngineUpdate;

const AUCTION_SLOT_TICKS: u64 = 6;
const CONTINUOUS_MINUTE_TICKS: u64 = 60;
const VISIBLE_TRADE_LIMIT: usize = 100;
/// 防止 pull 客户端停止取帧后仍无限占用服务端内存。达到预算时由网关要求客户端重同步。
pub const MAX_BUFFERED_EVENTS_PER_CLIENT: usize = 65_536;

#[derive(Debug, Clone, Serialize)]
pub struct PublisherFrame {
    pub from_seq: u64,
    pub to_seq: u64,
    pub events: Vec<Event>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_snapshot: Option<Snapshot>,
}

#[derive(Debug, thiserror::Error)]
pub enum FrameBufferError {
    #[error("ticks_per_day 必须大于 0")]
    InvalidTicksPerDay,
    #[error("auction_ticks 必须小于 ticks_per_day")]
    InvalidAuctionTicks,
    #[error("Publisher 收到空的 EngineUpdate")]
    EmptyUpdate,
    #[error("Publisher 事件 seq 不连续：期望 {expected}，收到 {actual}")]
    NonContiguous { expected: u64, actual: u64 },
    #[error("Publisher 事件 seq 已达到 u64 上限，无法验证下一事件")]
    SequenceOverflow,
    #[error("Publisher 的运行快照 seq {snapshot_seq} 与更新末尾 {update_seq} 不一致")]
    SnapshotSequenceMismatch { snapshot_seq: u64, update_seq: u64 },
    #[error("Publisher 客户端缓冲超过 {limit} 个原始事件，必须从权威快照重同步")]
    BufferCapacityExceeded { limit: usize },
}

/// 一个 WS 客户端独享一个缓冲；快客户端与慢客户端不会共享发送节拍或覆盖状态。
pub struct ClientFrameBuffer {
    ticks_per_day: u64,
    auction_ticks: u64,
    raw_events: Vec<Event>,
    runtime_snapshot: Option<Snapshot>,
    from_seq: Option<u64>,
    to_seq: Option<u64>,
}

impl ClientFrameBuffer {
    pub fn new(ticks_per_day: u64, auction_ticks: u64) -> Result<Self, FrameBufferError> {
        if ticks_per_day == 0 {
            return Err(FrameBufferError::InvalidTicksPerDay);
        }
        if auction_ticks >= ticks_per_day {
            return Err(FrameBufferError::InvalidAuctionTicks);
        }
        Ok(Self {
            ticks_per_day,
            auction_ticks,
            raw_events: Vec::new(),
            runtime_snapshot: None,
            from_seq: None,
            to_seq: None,
        })
    }

    pub fn push(&mut self, update: EngineUpdate) -> Result<(), FrameBufferError> {
        let Some(first) = update.events.first().map(Event::seq) else {
            return Err(FrameBufferError::EmptyUpdate);
        };
        let mut expected = self.to_seq.map_or(first, |seq| seq.saturating_add(1));
        for event in &update.events {
            let actual = event.seq();
            if actual != expected {
                return Err(FrameBufferError::NonContiguous { expected, actual });
            }
            expected = actual
                .checked_add(1)
                .ok_or(FrameBufferError::SequenceOverflow)?;
        }
        let last = update.events.last().map(Event::seq).expect("非空已校验");
        if let Some(snapshot) = &update.runtime_snapshot {
            if snapshot.seq != last {
                return Err(FrameBufferError::SnapshotSequenceMismatch {
                    snapshot_seq: snapshot.seq,
                    update_seq: last,
                });
            }
        }
        if self.raw_events.len().saturating_add(update.events.len())
            > MAX_BUFFERED_EVENTS_PER_CLIENT
        {
            return Err(FrameBufferError::BufferCapacityExceeded {
                limit: MAX_BUFFERED_EVENTS_PER_CLIENT,
            });
        }
        self.from_seq.get_or_insert(first);
        self.to_seq = Some(last);
        self.raw_events.extend(update.events);
        if update.runtime_snapshot.is_some() {
            self.runtime_snapshot = update.runtime_snapshot;
        }
        Ok(())
    }

    pub fn take(&mut self) -> Option<PublisherFrame> {
        let from_seq = self.from_seq.take()?;
        let pending_to_seq = self.to_seq.take().expect("from_seq 与 to_seq 同步维护");
        // 如果最新权威快照落在积累区间中部，先在该 seq 截断一帧。否则把旧快照
        // 放到更晚事件之后应用，会让客户端状态与事件顺序倒退。
        let snapshot_seq = self.runtime_snapshot.as_ref().map(|snapshot| snapshot.seq);
        let to_seq = snapshot_seq
            .filter(|seq| *seq < pending_to_seq)
            .unwrap_or(pending_to_seq);
        let split_at = self
            .raw_events
            .partition_point(|event| event.seq() <= to_seq);
        let remaining = self.raw_events.split_off(split_at);
        let events = compact_events(
            std::mem::take(&mut self.raw_events),
            self.ticks_per_day,
            self.auction_ticks,
        );
        let runtime_snapshot = self.runtime_snapshot.take();
        if let (Some(first), Some(last)) = (remaining.first(), remaining.last()) {
            self.from_seq = Some(first.seq());
            self.to_seq = Some(last.seq());
        }
        self.raw_events = remaining;
        Some(PublisherFrame {
            from_seq,
            to_seq,
            events,
            runtime_snapshot,
        })
    }

    pub fn clear(&mut self) {
        self.raw_events.clear();
        self.runtime_snapshot = None;
        self.from_seq = None;
        self.to_seq = None;
    }
}

fn compact_events(events: Vec<Event>, ticks_per_day: u64, auction_ticks: u64) -> Vec<Event> {
    let mut keep = HashSet::new();
    let mut active_slots: HashMap<(StockCode, u64), usize> = HashMap::new();
    let mut auction_slots: HashMap<(StockCode, u64), usize> = HashMap::new();
    let mut trade_indices = Vec::new();

    for (index, event) in events.iter().enumerate() {
        match event {
            Event::DayBoundary { .. } => {
                keep.insert(index);
                active_slots.clear();
                auction_slots.clear();
            }
            Event::AuctionTick { tick, code, .. } => {
                let day_tick = tick.saturating_sub(1) % ticks_per_day;
                auction_slots.insert((code.clone(), day_tick / AUCTION_SLOT_TICKS), index);
            }
            Event::PriceTick { tick, code, .. } => {
                let day_tick = tick.saturating_sub(1) % ticks_per_day;
                let minute = day_tick.saturating_sub(auction_ticks) / CONTINUOUS_MINUTE_TICKS;
                active_slots.insert((code.clone(), minute), index);
            }
            Event::Trade { .. } => trade_indices.push(index),
            _ => {
                keep.insert(index);
            }
        }
    }
    keep.extend(active_slots.into_values());
    keep.extend(auction_slots.into_values());
    keep.extend(trade_indices.into_iter().rev().take(VISIBLE_TRADE_LIMIT));

    events
        .into_iter()
        .enumerate()
        .filter_map(|(index, event)| keep.contains(&index).then_some(event))
        .collect()
}
