use super::{EngineUpdate, ProtocolError};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplayDecision {
    Applied,
    ExactRetry,
}

pub struct ReplayGuard {
    tick: u64,
    seq: u64,
    accepted: BTreeMap<(u64, u64), String>,
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
        let mut normalized = update.clone();
        let (first_tick, tick, from, to, civil) = match &mut normalized {
            EngineUpdate::TickBatch(batch) => {
                batch.validate()?;
                for frame in &mut batch.frames {
                    frame.facts.sort_by(|left, right| left.key.cmp(&right.key));
                    frame.events.sort_by_key(crate::Event::seq);
                }
                let first = batch.frames.first().ok_or(ProtocolError::EmptyBatch)?;
                let last = batch.frames.last().ok_or(ProtocolError::EmptyBatch)?;
                (first.tick, last.tick, first.seq_from, last.seq_to, false)
            }
            EngineUpdate::CivilUpdate(update) => {
                update.validate()?;
                update.facts.sort_by(|left, right| left.key.cmp(&right.key));
                update.events.sort_by_key(crate::Event::seq);
                for frame in &mut update.refresh.intraday {
                    frame.facts.sort_by(|left, right| left.key.cmp(&right.key));
                    frame.events.sort_by_key(crate::Event::seq);
                }
                (
                    update.tick,
                    update.tick,
                    update.seq_from,
                    update.seq_to,
                    true,
                )
            }
        };
        let bytes = super::facts::canonical(&normalized)?;
        if let Some(previous) = self.accepted.get(&(tick, to)) {
            return if previous == &bytes {
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
        self.accepted.insert((tick, to), bytes);
        self.tick = tick;
        self.seq = to;
        Ok(ReplayDecision::Applied)
    }
}
