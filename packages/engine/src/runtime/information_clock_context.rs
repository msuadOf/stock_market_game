//! INFO 只读来源封装：真实静点或本 authority 已提交市场拍；不序列化、不另建时钟。

use super::protocol::RuntimeError;
use super::world::CommittedCalendarTick;
use crate::calendar::{CalendarExchange, CivilDate, CivilInstant};
use crate::observation::information_clock::InformationTradingAnchor;
use crate::session::exchange_calendar_model::{
    CalendarClockProjection, SessionTickLayout, SharedClockInput,
};
use crate::session::market_calendar_context::{MarketCalendarSource, MarketDayContext};
use crate::StockId;
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct InformationClockContext {
    inner: InformationClockSource,
    cutoff: CivilInstant,
}

#[derive(Clone, Debug)]
enum InformationClockSource {
    AfterCommittedTick(CommittedCalendarTick),
    Static {
        day: Arc<MarketDayContext>,
        layout: SessionTickLayout,
        input: SharedClockInput,
        projection: Arc<CalendarClockProjection>,
    },
}

/// 接收方真实进度提供的 INFO 认证输入；不能从待校验 payload 自取。
pub(crate) struct ExpectedInformationClock<'a> {
    pub(crate) source: &'a Arc<MarketCalendarSource>,
    pub(crate) bindings: &'a [(StockId, CalendarExchange)],
    pub(crate) date: CivilDate,
    pub(crate) layout: SessionTickLayout,
    pub(crate) input: SharedClockInput,
    pub(crate) through: CivilInstant,
    pub(crate) anchors: &'a BTreeMap<StockId, InformationTradingAnchor>,
}

impl InformationClockContext {
    fn invalid(detail: impl Into<String>) -> RuntimeError {
        RuntimeError::ProtocolInvariant {
            detail: detail.into(),
        }
    }

    pub(crate) fn from_session(session: &crate::GameSession) -> Result<Self, RuntimeError> {
        let (day, projection) = session
            .prepare_market_calendar_projection()
            .map_err(|error| Self::invalid(error.to_string()))?;
        let setup = session.export_setup_for_runtime();
        let layout = SessionTickLayout {
            ticks_per_day: setup.ticks_per_day,
            auction_ticks: setup.auction_ticks,
            closing_auction_ticks: setup.closing_auction_ticks,
        };
        let input = crate::session::calendar_clock_input(
            &day,
            layout,
            session.tick(),
            u64::from(session.day()),
        )
        .map_err(|error| Self::invalid(error.to_string()))?;
        Ok(Self::from_static_projection(
            day,
            layout,
            input,
            projection,
            session.runtime_observation_instant(),
        ))
    }

    pub(crate) fn after_committed(record: CommittedCalendarTick) -> Self {
        let cutoff = record.calendar.observation_instant_after();
        Self {
            inner: InformationClockSource::AfterCommittedTick(record),
            cutoff,
        }
    }

    /// 只从原日期 Session 捕获真实投影；披露 cutoff 来自本次完整 civil report。
    pub(crate) fn for_civil_disclosure(
        session: &crate::GameSession,
        cutoff: CivilInstant,
    ) -> Result<Self, crate::session::StepFatal> {
        let fail = |description| crate::session::StepFatal::InvariantViolation {
            location: "InformationClockContext::for_civil_disclosure".to_owned(),
            description,
        };
        if cutoff.date() != session.civil_date() {
            return Err(fail("civil 披露 cutoff 与原 Session 日期不一致".to_owned()));
        }
        let (day, projection) = session.prepare_market_calendar_projection()?;
        let setup = session.export_setup_for_runtime();
        let layout = SessionTickLayout {
            ticks_per_day: setup.ticks_per_day,
            auction_ticks: setup.auction_ticks,
            closing_auction_ticks: setup.closing_auction_ticks,
        };
        let input = crate::session::calendar_clock_input(
            &day,
            layout,
            session.tick(),
            u64::from(session.day()),
        )
        .map_err(|error| fail(error.to_string()))?;
        Ok(Self::from_static_projection(
            day, layout, input, projection, cutoff,
        ))
    }

    fn from_static_projection(
        day: Arc<MarketDayContext>,
        layout: SessionTickLayout,
        input: SharedClockInput,
        projection: CalendarClockProjection,
        cutoff: CivilInstant,
    ) -> Self {
        Self {
            inner: InformationClockSource::Static {
                day,
                layout,
                input,
                projection: Arc::new(projection),
            },
            cutoff,
        }
    }

    /// 证明绑定原输入身份；不把内容相同的异 owner/异观察阶段 context 当作同一来源。
    pub(crate) fn same_source(&self, other: &Self) -> bool {
        if self.cutoff != other.cutoff {
            return false;
        }
        match (&self.inner, &other.inner) {
            (
                InformationClockSource::AfterCommittedTick(left),
                InformationClockSource::AfterCommittedTick(right),
            ) => left.source == right.source && Arc::ptr_eq(&left.calendar, &right.calendar),
            (
                InformationClockSource::Static {
                    day: left_day,
                    projection: left,
                    ..
                },
                InformationClockSource::Static {
                    day: right_day,
                    projection: right,
                    ..
                },
            ) => Arc::ptr_eq(left_day, right_day) && Arc::ptr_eq(left, right),
            _ => false,
        }
    }

    pub(crate) fn day(&self) -> &Arc<MarketDayContext> {
        match &self.inner {
            InformationClockSource::AfterCommittedTick(record) => record.calendar.day(),
            InformationClockSource::Static { day, .. } => day,
        }
    }
    #[cfg(test)]
    pub(crate) fn date(&self) -> CivilDate {
        self.day().day_model().current_date()
    }
    pub(crate) fn cutoff(&self) -> CivilInstant {
        self.cutoff
    }
    pub(crate) fn layout(&self) -> SessionTickLayout {
        match &self.inner {
            InformationClockSource::AfterCommittedTick(record) => record.calendar.layout(),
            InformationClockSource::Static { layout, .. } => *layout,
        }
    }
    pub(crate) fn input(&self) -> SharedClockInput {
        match &self.inner {
            InformationClockSource::AfterCommittedTick(record) => record.calendar.after_input(),
            InformationClockSource::Static { input, .. } => *input,
        }
    }
    pub(crate) fn committed_record(&self) -> Option<&CommittedCalendarTick> {
        match &self.inner {
            InformationClockSource::AfterCommittedTick(record) => Some(record),
            InformationClockSource::Static { .. } => None,
        }
    }
    pub(crate) fn anchors(
        &self,
    ) -> Result<BTreeMap<StockId, InformationTradingAnchor>, RuntimeError> {
        let projection = match &self.inner {
            InformationClockSource::AfterCommittedTick(record) => {
                record.calendar.after_projection()
            }
            InformationClockSource::Static { projection, .. } => projection,
        };
        crate::session::information_anchor::information_trading_anchors(
            self.day().day_model(),
            projection,
        )
        .map_err(|error| Self::invalid(error.to_string()))
    }

    /// expected_* 必须由接收方真实进度提供，不能从 payload 自取作认证。
    pub(crate) fn validate(
        &self,
        expected: ExpectedInformationClock<'_>,
    ) -> Result<(), RuntimeError> {
        let ExpectedInformationClock {
            source,
            bindings,
            date,
            layout,
            input,
            through,
            anchors,
        } = expected;
        self.day()
            .validate_for(source, bindings, date)
            .map_err(|error| Self::invalid(error.to_string()))?;
        let actual_layout = self.layout();
        let actual = self.input();
        if (
            actual_layout.ticks_per_day,
            actual_layout.auction_ticks,
            actual_layout.closing_auction_ticks,
        ) != (
            layout.ticks_per_day,
            layout.auction_ticks,
            layout.closing_auction_ticks,
        ) || actual.tick != input.tick
            || actual.completed_shared_sessions != input.completed_shared_sessions
            || actual.progress != input.progress
        {
            return Err(Self::invalid(
                "INFO Calendar 与接收方真实 layout/clock 不一致",
            ));
        }
        if through != self.cutoff || through.date() != date {
            return Err(Self::invalid("INFO through 不属于本次真实观察 cutoff/date"));
        }
        if self.anchors()? != *anchors {
            return Err(Self::invalid(
                "INFO typed anchor map 与真实完整 Calendar projection 不一致",
            ));
        }
        Ok(())
    }

    /// 仅合成 Admin 机制 fixture；不认证生产 dispatcher 资格。
    #[cfg(test)]
    pub(crate) fn fixture_static(
        day: Arc<MarketDayContext>,
        layout: SessionTickLayout,
        input: SharedClockInput,
        cutoff: CivilInstant,
    ) -> Self {
        let projection = crate::session::exchange_calendar_model::calendar_clock_projection(
            day.day_model(),
            layout,
            input,
        )
        .expect("fixture 必须使用真实且相容的 Calendar 计数");
        Self::from_static_projection(day, layout, input, projection, cutoff)
    }
}

#[cfg(test)]
mod tests;
