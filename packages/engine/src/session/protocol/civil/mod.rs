use super::{validate_sequence, ProtocolError, TickBatch, TickFrame};
use crate::session::{Event, GameSession, SessionError, Snapshot, StockSpec};
mod boundary;
#[cfg(test)]
mod publication_tests;
mod session;
use super::{attach_facts, EventFact};
pub use boundary::CivilBoundary;
pub use session::{ProtocolCheckpoint, ProtocolSession};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub enum CivilUpdateKind {
    AfterClose,
    BeforeOpen,
    CivilAdvance,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct CivilRefresh {
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub ticks_per_day: u64,
    pub snapshot: Snapshot,
    pub securities: Vec<StockSpec>,
    pub intraday: Vec<TickFrame>,
    pub public_publication_ids: Vec<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct CivilUpdate {
    pub boundary: CivilBoundary,
    pub kinds: Vec<CivilUpdateKind>,
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub tick: u64,
    pub civil_date: String,
    pub events: Vec<Event>,
    pub facts: Vec<EventFact>,
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub seq_from: u64,
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub seq_to: u64,
    pub refresh: CivilRefresh,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub enum EngineUpdate {
    TickBatch(TickBatch),
    CivilUpdate(Box<CivilUpdate>),
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS,
)]
#[ts(export)]
pub struct PausePreferences {
    pub pause_after_close: bool,
    pub pause_before_open: bool,
}

impl PausePreferences {
    pub fn pauses(self, update: &CivilUpdate) -> bool {
        update.kinds.iter().any(|kind| match kind {
            CivilUpdateKind::AfterClose => self.pause_after_close,
            CivilUpdateKind::BeforeOpen => self.pause_before_open,
            CivilUpdateKind::CivilAdvance => false,
        })
    }
}

impl CivilUpdate {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        super::facts::validate_facts(&self.facts, &self.events)?;
        self.boundary.validate(self)?;
        validate_sequence(&self.events, self.seq_from, self.seq_to)?;
        if self.refresh.snapshot.tick != self.tick || self.refresh.snapshot.seq != self.seq_to {
            return Err(ProtocolError::SnapshotMismatch);
        }
        let advances: Vec<_> = self
            .events
            .iter()
            .filter_map(|event| match event {
                Event::CivilDateAdvanced {
                    next_date,
                    next_status,
                    ..
                } => Some((next_date, next_status)),
                _ => None,
            })
            .collect();
        let [(next_date, next_status)] = advances.as_slice() else {
            return Err(ProtocolError::CivilBoundary);
        };
        if next_date.to_iso() != self.civil_date
            || self.kinds.is_empty()
            || self
                .kinds
                .iter()
                .enumerate()
                .any(|(index, kind)| self.kinds[..index].contains(kind))
            || self.kinds.contains(&CivilUpdateKind::BeforeOpen)
                != matches!(next_status, crate::DayStatus::Trading)
        {
            return Err(ProtocolError::CivilBoundary);
        }
        if self.refresh.ticks_per_day == 0
            || (self.kinds.contains(&CivilUpdateKind::CivilAdvance) && self.kinds.len() != 1)
        {
            return Err(ProtocolError::CivilBoundary);
        }
        let codes: std::collections::BTreeSet<_> = self
            .refresh
            .securities
            .iter()
            .map(|stock| &stock.code)
            .collect();
        if codes.is_empty()
            || codes.len() != self.refresh.securities.len()
            || codes != self.refresh.snapshot.markets.keys().collect()
        {
            return Err(ProtocolError::CivilBoundary);
        }
        let reports: std::collections::BTreeSet<_> =
            self.refresh.public_publication_ids.iter().collect();
        if reports.len() != self.refresh.public_publication_ids.len()
            || self.events.iter().any(|event| match event {
                Event::CompanyDisclosurePublished { publication_id, .. } => {
                    !reports.contains(&publication_id.value().to_string())
                }
                _ => false,
            })
        {
            return Err(ProtocolError::CivilBoundary);
        }
        if self.kinds.contains(&CivilUpdateKind::AfterClose) {
            let history = TickBatch {
                frames: self.refresh.intraday.clone(),
                runtime_snapshot: None,
            };
            history.validate()?;
            if u64::try_from(history.frames.len()).ok() != Some(self.refresh.ticks_per_day)
                || history
                    .frames
                    .last()
                    .map(|frame| (frame.tick, frame.seq_to))
                    != Some((self.tick, self.seq_from))
            {
                return Err(ProtocolError::CivilBoundary);
            }
        } else if !self.refresh.intraday.is_empty() {
            return Err(ProtocolError::CivilBoundary);
        }
        Ok(())
    }
}

impl GameSession {
    fn end_civil_day_update(
        &mut self,
        intraday: &[TickFrame],
    ) -> Result<CivilUpdate, SessionError> {
        let settled_phase = self.civil_clock().phase();
        let after_close = settled_phase != crate::session::CivilPhase::ClosedDay;
        if after_close {
            let batch = TickBatch {
                frames: intraday.to_vec(),
                runtime_snapshot: None,
            };
            batch
                .validate()
                .map_err(|error| SessionError::InvalidSave(error.to_string()))?;
            let expected_start = self
                .tick()
                .checked_sub(self.setup.ticks_per_day)
                .and_then(|tick| tick.checked_add(1));
            if intraday.first().map(|frame| frame.tick) != expected_start
                || intraday.last().map(|frame| (frame.tick, frame.seq_to))
                    != Some((self.tick(), self.seq()))
            {
                return Err(SessionError::InvalidSave(
                    "civil refresh requires the complete closing session".into(),
                ));
            }
        }
        let seq_from = self.seq();
        let report = self.end_civil_day()?;
        let mut kinds = Vec::new();
        if after_close {
            kinds.push(CivilUpdateKind::AfterClose);
        }
        if matches!(report.next_status, crate::DayStatus::Trading) {
            kinds.push(CivilUpdateKind::BeforeOpen);
        }
        if kinds.is_empty() {
            kinds.push(CivilUpdateKind::CivilAdvance);
        }
        Ok(CivilUpdate {
            boundary: CivilBoundary {
                settled_date: report.settled_date,
                settled_phase,
                next_date: report.next_date,
                next_status: report.next_status.clone(),
            },
            facts: attach_facts(&report.events)
                .map_err(|error| SessionError::InvalidSave(error.to_string()))?,
            kinds,
            tick: self.tick(),
            civil_date: self.civil_date().to_iso(),
            events: report.events,
            seq_from,
            seq_to: self.seq(),
            refresh: CivilRefresh {
                ticks_per_day: self.setup.ticks_per_day,
                snapshot: self.snapshot(),
                securities: self.setup.stocks.clone(),
                intraday: intraday.to_vec(),
                public_publication_ids: self
                    .library
                    .all_publication_ids()
                    .map_err(|error| SessionError::InvalidSave(error.to_string()))?
                    .into_iter()
                    .map(|id| id.value().to_string())
                    .collect(),
            },
        })
    }
}
