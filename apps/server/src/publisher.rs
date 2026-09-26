use std::collections::VecDeque;

use engine::session::protocol::{EngineUpdate as ProtocolUpdate, ProtocolError};

use crate::actor::EngineUpdate;

pub const MAX_BUFFERED_EVENTS_PER_CLIENT: usize = 65_536;
pub type PublisherFrame = EngineUpdate;

#[derive(Debug, thiserror::Error)]
pub enum FrameBufferError {
    #[error("ticks_per_day must be positive")]
    InvalidTicksPerDay,
    #[error("auction_ticks must be less than ticks_per_day")]
    InvalidAuctionTicks,
    #[error("publisher requires a healthy protocol update")]
    EmptyUpdate,
    #[error(transparent)]
    InvalidProtocol(#[from] ProtocolError),
    #[error("publisher buffer exceeds {limit} units; resync required")]
    BufferCapacityExceeded { limit: usize },
    #[error("publisher metadata transition requires a flush")]
    MetadataTransition,
    #[error("publisher update cursor or generation is discontinuous")]
    CursorMismatch,
}

pub struct ClientFrameBuffer {
    pending: VecDeque<EngineUpdate>,
    units: usize,
    cursor: Option<(u64, u64, u64)>,
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
            pending: VecDeque::new(),
            units: 0,
            cursor: None,
        })
    }

    pub fn push(&mut self, update: EngineUpdate) -> Result<(), FrameBufferError> {
        let protocol = update
            .update
            .as_ref()
            .ok_or(FrameBufferError::EmptyUpdate)?;
        match protocol {
            ProtocolUpdate::TickBatch(batch) => batch.validate()?,
            ProtocolUpdate::CivilUpdate(civil) => civil.validate()?,
        }
        let (first_tick, tick, from, to, civil) = match protocol {
            ProtocolUpdate::TickBatch(batch) => {
                let first = batch.frames.first().ok_or(FrameBufferError::EmptyUpdate)?;
                let last = batch.frames.last().ok_or(FrameBufferError::EmptyUpdate)?;
                (first.tick, last.tick, first.seq_from, last.seq_to, false)
            }
            ProtocolUpdate::CivilUpdate(civil) => {
                (civil.tick, civil.tick, civil.seq_from, civil.seq_to, true)
            }
        };
        if let Some((previous_tick, previous_seq, generation)) = self.cursor {
            let expected_tick = if civil {
                Some(previous_tick)
            } else {
                previous_tick.checked_add(1)
            };
            if expected_tick != Some(first_tick)
                || previous_seq != from
                || generation != update.timeline_generation
            {
                return Err(FrameBufferError::CursorMismatch);
            }
        }
        let units = update_units(protocol);
        // A protocol update is indivisible. One valid large update must be
        // deliverable even when it exceeds the backlog limit by itself.
        if !self.pending.is_empty()
            && self.units.saturating_add(units) > MAX_BUFFERED_EVENTS_PER_CLIENT
        {
            return Err(FrameBufferError::BufferCapacityExceeded {
                limit: MAX_BUFFERED_EVENTS_PER_CLIENT,
            });
        }
        self.units += units;
        self.cursor = Some((tick, to, update.timeline_generation));
        self.pending.push_back(update);
        Ok(())
    }

    pub fn take(&mut self) -> Option<PublisherFrame> {
        let update = self.pending.pop_front()?;
        if let Some(protocol) = &update.update {
            self.units -= update_units(protocol);
        }
        Some(update)
    }

    pub fn clear(&mut self) {
        self.pending.clear();
        self.units = 0;
        self.cursor = None;
    }
}

fn update_units(update: &ProtocolUpdate) -> usize {
    match update {
        ProtocolUpdate::TickBatch(batch) => batch
            .frames
            .iter()
            .map(|frame| frame.events.len().max(1))
            .sum(),
        ProtocolUpdate::CivilUpdate(civil) => {
            civil.events.len().max(1)
                + civil
                    .refresh
                    .intraday
                    .iter()
                    .map(|frame| frame.events.len().max(1))
                    .sum::<usize>()
        }
    }
}
