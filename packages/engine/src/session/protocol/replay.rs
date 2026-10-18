use super::{EngineUpdate, EventFact, ProtocolError, TickFrame};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplayDecision {
    Applied,
    ExactRetry,
}

/// 传输边界的已接收记录：保留原有 retry 守卫，但使用 typed 更新而非 JSON 字符串。
/// 身份由 producer 的数值 key 决定；比较只借用记录，不重建事件或序列化 payload。
pub struct ReplayGuard {
    tick: u64,
    seq: u64,
    accepted: BTreeMap<(u64, u64), EngineUpdate>,
}

impl ReplayGuard {
    pub fn new(tick: u64, seq: u64) -> Self {
        Self {
            tick,
            seq,
            accepted: BTreeMap::new(),
        }
    }

    pub fn ingest(&mut self, update: &EngineUpdate) -> Result<ReplayDecision, ProtocolError> {
        let (first_tick, tick, from, to, civil) = match update {
            EngineUpdate::TickBatch(batch) => {
                batch.validate()?;
                let first = batch.frames.first().ok_or(ProtocolError::EmptyBatch)?;
                let last = batch.frames.last().ok_or(ProtocolError::EmptyBatch)?;
                (first.tick, last.tick, first.seq_from, last.seq_to, false)
            }
            EngineUpdate::CivilUpdate(update) => {
                update.validate()?;
                (
                    update.tick,
                    update.tick,
                    update.seq_from,
                    update.seq_to,
                    true,
                )
            }
        };
        if let Some(previous) = self.accepted.get(&(tick, to)) {
            return if same_update(previous, update) {
                Ok(ReplayDecision::ExactRetry)
            } else {
                Err(ProtocolError::ReplayMismatch)
            };
        }
        let expected = if civil {
            Some(self.tick)
        } else {
            self.tick.checked_add(1)
        };
        if expected != Some(first_tick) || from != self.seq {
            return Err(ProtocolError::ReplayMismatch);
        }
        // retry 见证只在首次接收时保留；不是运行时权威或新身份缓存。
        self.accepted.insert((tick, to), update.clone());
        self.tick = tick;
        self.seq = to;
        Ok(ReplayDecision::Applied)
    }
}

fn same_facts(left: &[EventFact], right: &[EventFact]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let right_by_key: BTreeMap<_, _> = right.iter().map(|fact| (&fact.key, &fact.event)).collect();
    left.iter().all(|fact| {
        right_by_key
            .get(&fact.key)
            .is_some_and(|event| **event == fact.event)
    })
}

fn same_frame(left: &TickFrame, right: &TickFrame) -> bool {
    left.tick == right.tick
        && left.seq_from == right.seq_from
        && left.seq_to == right.seq_to
        && left.timeseries_payload == right.timeseries_payload
        && same_facts(&left.facts, &right.facts)
}

fn same_frames(left: &[TickFrame], right: &[TickFrame]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| same_frame(left, right))
}

fn same_update(left: &EngineUpdate, right: &EngineUpdate) -> bool {
    match (left, right) {
        (EngineUpdate::TickBatch(left), EngineUpdate::TickBatch(right)) => {
            left.runtime_snapshot == right.runtime_snapshot
                && left.runtime_delta == right.runtime_delta
                && same_frames(&left.frames, &right.frames)
        }
        (EngineUpdate::CivilUpdate(left), EngineUpdate::CivilUpdate(right)) => {
            left.boundary == right.boundary
                && left.kinds == right.kinds
                && left.tick == right.tick
                && left.civil_date == right.civil_date
                && left.seq_from == right.seq_from
                && left.seq_to == right.seq_to
                && same_facts(&left.facts, &right.facts)
                && left.refresh.ticks_per_day == right.refresh.ticks_per_day
                && left.refresh.snapshot == right.refresh.snapshot
                && left.refresh.securities == right.refresh.securities
                && left.refresh.public_publication_ids == right.refresh.public_publication_ids
                && same_frames(&left.refresh.intraday, &right.refresh.intraday)
        }
        _ => false,
    }
}
