use super::{validate_sequence, ProtocolError, TickBatch, TickFrame};
use crate::error::ValidationFault;
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
    TickBatch(Box<TickBatch>),
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
        let catalog = crate::stock::StockCatalog::from_metas(
            self.refresh
                .securities
                .iter()
                .map(|stock| crate::stock::StockMeta::new(stock.code.0.clone()))
                .collect(),
        )
        .map_err(|_| ProtocolError::CivilBoundary)?;
        let codes: std::collections::BTreeSet<_> = self
            .refresh
            .securities
            .iter()
            .map(|stock| {
                catalog
                    .id(&stock.code.0)
                    .expect("目录由同一批证券构造，必须命中")
            })
            .collect();
        if codes.is_empty()
            || codes.len() != self.refresh.securities.len()
            || codes != self.refresh.snapshot.markets.keys().copied().collect()
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
                runtime_delta: None,
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

/// ProtocolError → 数值化 fault（ADR 0041：InvalidSave 不携带字符串）。
/// 变体各占一码（3001-3008 按枚举声明序），并保留变体自身的数值上下文。
fn protocol_save_fault(error: ProtocolError) -> ValidationFault {
    match error {
        // fault3001: 事件身份或规范化载荷不匹配（原 Display "event identity or canonical payload mismatch"）。
        ProtocolError::FactIdentity => ValidationFault::bare(3001),
        // fault3002: 回放发生变更或游标缺口（原 Display "update replay mutation or cursor gap"）。
        ProtocolError::ReplayMismatch => ValidationFault::bare(3002),
        // fault3003: 事件序列覆盖区间不精确；ctx[0]=from，ctx[1]=to。
        ProtocolError::SequenceCoverage { from, to } => ValidationFault::new(3003, vec![from, to]),
        // fault3004: tick 批为空（原 Display "tick batch must contain at least one frame"）。
        ProtocolError::EmptyBatch => ValidationFault::bare(3004),
        // fault3005: tick 帧不连续；ctx[0]=断裂处的下一帧 tick。
        ProtocolError::TickContinuity { tick } => ValidationFault::new(3005, vec![tick]),
        // fault3006: 运行时快照与末帧 tick/seq 不一致。
        ProtocolError::SnapshotMismatch => ValidationFault::bare(3006),
        // fault3007: 运行时 delta 权威/游标/账户/在途委托补丁不一致。
        ProtocolError::RuntimeDeltaMismatch => ValidationFault::bare(3007),
        // fault3008: 自然日边界元数据或收盘历史不一致。
        ProtocolError::CivilBoundary => ValidationFault::bare(3008),
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
                runtime_delta: None,
            };
            // fault3001-3008: 收盘 intraday 批校验失败（ProtocolError 变体映射，见 protocol_save_fault）。
            batch
                .validate()
                .map_err(|error| SessionError::InvalidSave(protocol_save_fault(error)))?;
            let expected_start = self
                .tick()
                .checked_sub(self.state.setup.ticks_per_day)
                .and_then(|tick| tick.checked_add(1));
            if intraday.first().map(|frame| frame.tick) != expected_start
                || intraday.last().map(|frame| (frame.tick, frame.seq_to))
                    != Some((self.tick(), self.seq()))
            {
                // fault3009: civil refresh 必须覆盖完整收盘会话（帧首 tick/末帧 tick+seq 与会话权威不符；
                // 原文本 "civil refresh requires the complete closing session"）。
                return Err(SessionError::InvalidSave(ValidationFault::bare(3009)));
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
            // fault3001-3008: 日结事件挂靠 facts 失败（ProtocolError 变体映射，见 protocol_save_fault）。
            facts: attach_facts(&report.events)
                .map_err(|error| SessionError::InvalidSave(protocol_save_fault(error)))?,
            kinds,
            tick: self.tick(),
            civil_date: self.civil_date().to_iso(),
            events: report.events,
            seq_from,
            seq_to: self.seq(),
            refresh: CivilRefresh {
                ticks_per_day: self.state.setup.ticks_per_day,
                snapshot: self.snapshot(),
                securities: self.state.setup.stocks.clone(),
                intraday: intraday.to_vec(),
                public_publication_ids: self
                    .state
                    .library
                    .all_publication_ids()
                    // fault3010: 信息域公布 id 重复；ctx[0]=重复的 PublicationId 数值。
                    // fault3011: InformationError 其余变体兜底（all_publication_ids 当前只产
                    // DuplicatePublicationId，此臂防御式保留，不静默吞掉未来新增变体）。
                    .map_err(|error| match error {
                        crate::information::InformationError::DuplicatePublicationId { id } => {
                            SessionError::InvalidSave(ValidationFault::new(
                                3010,
                                vec![u64::from(id.value())],
                            ))
                        }
                        _ => SessionError::InvalidSave(ValidationFault::bare(3011)),
                    })?
                    .into_iter()
                    .map(|id| id.value().to_string())
                    .collect(),
            },
        })
    }
}
