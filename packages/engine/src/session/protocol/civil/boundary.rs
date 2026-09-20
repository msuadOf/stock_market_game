use super::{CivilUpdate, CivilUpdateKind};
use crate::session::{protocol::ProtocolError, CivilPhase};
use crate::{CivilDate, DayStatus, Event};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct CivilBoundary {
    pub settled_date: CivilDate,
    pub settled_phase: CivilPhase,
    pub next_date: CivilDate,
    pub next_status: DayStatus,
}

impl CivilBoundary {
    pub(super) fn validate(&self, update: &CivilUpdate) -> Result<(), ProtocolError> {
        let after = self.settled_phase == CivilPhase::IntradayTrading;
        let before = matches!(self.next_status, DayStatus::Trading);
        let expected = match (after, before) {
            (true, true) => vec![CivilUpdateKind::AfterClose, CivilUpdateKind::BeforeOpen],
            (true, false) => vec![CivilUpdateKind::AfterClose],
            (false, true) => vec![CivilUpdateKind::BeforeOpen],
            (false, false) => vec![CivilUpdateKind::CivilAdvance],
        };
        if update.kinds != expected || self.settled_date.next().ok() != Some(self.next_date)
            || self.next_date.to_iso() != update.civil_date
            || (after && (update.tick == 0 || update.refresh.ticks_per_day == 0 || !update.tick.is_multiple_of(update.refresh.ticks_per_day)))
            || !update.events.iter().any(|event| matches!(event, Event::CivilDateAdvanced { settled_date, next_date, next_status, .. }
                if *settled_date == self.settled_date && *next_date == self.next_date && *next_status == self.next_status))
        { return Err(ProtocolError::CivilBoundary); }
        Ok(())
    }
}
