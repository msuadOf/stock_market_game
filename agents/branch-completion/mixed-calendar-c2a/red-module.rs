//! 同一 source owner 的不可变日历上下文；本模块不接入 live 市场。

use std::sync::Arc;

use crate::calendar::{CalendarExchange, CivilDate, TradingCalendar};
use crate::stock::StockId;

use super::exchange_calendar_model::{
    calendar_day_model, CalendarClockModelError, CalendarClockProjection, CalendarDayModel,
    SessionTickLayout, SharedClockInput, SharedDayProgress,
};

#[derive(Debug, thiserror::Error)]
pub(crate) enum MarketCalendarContextError {
    #[error("Calendar context 投影失败：{0}")]
    Model(#[from] CalendarClockModelError),
    #[error("Calendar context 不属于接收方持有的 source owner")]
    SourceMismatch,
    #[error("接收方 bindings 重复 StockId：{stock:?}")]
    DuplicateExpectedStock { stock: StockId },
    #[error("StockId {stock:?} 绑定不一致：expected={expected:?}, actual={actual:?}")]
    BindingMismatch {
        stock: StockId,
        expected: Option<CalendarExchange>,
        actual: Option<CalendarExchange>,
    },
    #[error("civil date 不一致：expected={expected}, actual={actual}")]
    DateMismatch {
        expected: CivilDate,
        actual: CivilDate,
    },
    #[error("tick layout 不一致：expected={expected:?}, actual={actual:?}")]
    TickLayoutMismatch {
        expected: SessionTickLayout,
        actual: SessionTickLayout,
    },
    #[error("before clock 不一致：expected={expected:?}, actual={actual:?}")]
    BeforeClockMismatch {
        expected: SharedClockInput,
        actual: SharedClockInput,
    },
    #[error("市场拍要求 Trading 起点，实际为 {progress:?}")]
    MarketTickRequiresTrading { progress: SharedDayProgress },
}

#[derive(Debug)]
pub(crate) struct MarketCalendarSource {
    calendar: TradingCalendar,
    start_date: CivilDate,
    bindings: Vec<(StockId, CalendarExchange)>,
}

#[derive(Debug)]
pub(crate) struct MarketDayContext {
    source: Arc<MarketCalendarSource>,
    model: CalendarDayModel,
}

#[derive(Debug)]
pub(crate) struct MarketTickContext {
    day: Arc<MarketDayContext>,
    layout: SessionTickLayout,
    before: SharedClockInput,
    after: SharedClockInput,
    before_projection: CalendarClockProjection,
    after_projection: CalendarClockProjection,
}

impl MarketCalendarSource {
    pub(crate) fn new(
        calendar: TradingCalendar,
        start_date: CivilDate,
        bindings: Vec<(StockId, CalendarExchange)>,
    ) -> Result<Arc<Self>, MarketCalendarContextError> {
        calendar_day_model(&calendar, start_date, start_date, &bindings)?;
        Ok(Arc::new(Self {
            calendar,
            start_date,
            bindings,
        }))
    }

    pub(crate) fn day(
        self: &Arc<Self>,
        date: CivilDate,
    ) -> Result<Arc<MarketDayContext>, MarketCalendarContextError> {
        let model = calendar_day_model(&self.calendar, self.start_date, date, &self.bindings)?;
        Ok(Arc::new(MarketDayContext {
            source: Arc::clone(self),
            model,
        }))
    }
}

impl MarketDayContext {
    pub(crate) fn day_model(&self) -> &CalendarDayModel {
        &self.model
    }

    pub(crate) fn validate_for(
        &self,
        _expected_source: &Arc<MarketCalendarSource>,
        _expected_bindings: &[(StockId, CalendarExchange)],
        _expected_date: CivilDate,
    ) -> Result<(), MarketCalendarContextError> {
        Ok(())
    }

    pub(crate) fn tick(
        self: &Arc<Self>,
        _layout: SessionTickLayout,
        before: SharedClockInput,
    ) -> Result<Arc<MarketTickContext>, MarketCalendarContextError> {
        Err(MarketCalendarContextError::MarketTickRequiresTrading {
            progress: before.progress,
        })
    }
}

impl MarketTickContext {
    pub(crate) fn day(&self) -> &Arc<MarketDayContext> {
        &self.day
    }
    pub(crate) fn layout(&self) -> SessionTickLayout {
        self.layout
    }
    pub(crate) fn before_input(&self) -> SharedClockInput {
        self.before
    }
    pub(crate) fn after_input(&self) -> SharedClockInput {
        self.after
    }
    pub(crate) fn before_projection(&self) -> &CalendarClockProjection {
        &self.before_projection
    }
    pub(crate) fn after_projection(&self) -> &CalendarClockProjection {
        &self.after_projection
    }

    pub(crate) fn validate_for(
        &self,
        _expected_source: &Arc<MarketCalendarSource>,
        _expected_bindings: &[(StockId, CalendarExchange)],
        _expected_date: CivilDate,
        _expected_layout: SessionTickLayout,
        _expected_before: SharedClockInput,
    ) -> Result<(), MarketCalendarContextError> {
        Ok(())
    }
}

#[cfg(test)]
#[path = "market_calendar_context_tests.rs"]
mod tests;
