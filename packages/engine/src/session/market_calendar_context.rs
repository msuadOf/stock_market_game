//! 同一 source owner 的不可变日历上下文；本模块不接入 live 市场。

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::calendar::{CalendarExchange, CivilDate, TradingCalendar};
use crate::stock::StockId;

use super::exchange_calendar_model::{
    calendar_clock_projection, calendar_day_model, CalendarClockModelError,
    CalendarClockProjection, CalendarDayModel, SessionTickLayout, SharedClockInput,
    SharedDayProgress,
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
        expected_source: &Arc<MarketCalendarSource>,
        expected_bindings: &[(StockId, CalendarExchange)],
        expected_date: CivilDate,
    ) -> Result<(), MarketCalendarContextError> {
        if !Arc::ptr_eq(&self.source, expected_source) {
            return Err(MarketCalendarContextError::SourceMismatch);
        }
        let mut expected = BTreeMap::new();
        for &(stock, exchange) in expected_bindings {
            if expected.insert(stock, exchange).is_some() {
                return Err(MarketCalendarContextError::DuplicateExpectedStock { stock });
            }
        }
        // 双向检查完整 StockId 映射，避免同长度替换、缺股或多股被截断比较掩盖。
        for (&stock, &exchange) in &expected {
            let actual = self.model.stocks().get(&stock).map(|row| row.exchange());
            if actual != Some(exchange) {
                return Err(MarketCalendarContextError::BindingMismatch {
                    stock,
                    expected: Some(exchange),
                    actual,
                });
            }
        }
        for (&stock, row) in self.model.stocks() {
            if !expected.contains_key(&stock) {
                return Err(MarketCalendarContextError::BindingMismatch {
                    stock,
                    expected: None,
                    actual: Some(row.exchange()),
                });
            }
        }
        if expected_date != self.model.current_date() {
            return Err(MarketCalendarContextError::DateMismatch {
                expected: expected_date,
                actual: self.model.current_date(),
            });
        }
        Ok(())
    }

    pub(crate) fn tick(
        self: &Arc<Self>,
        layout: SessionTickLayout,
        before: SharedClockInput,
    ) -> Result<Arc<MarketTickContext>, MarketCalendarContextError> {
        let before_projection = calendar_clock_projection(&self.model, layout, before)?;
        let SharedDayProgress::Trading { completed_ticks } = before.progress else {
            return Err(MarketCalendarContextError::MarketTickRequiresTrading {
                progress: before.progress,
            });
        };
        let tick = before
            .tick
            .checked_add(1)
            .ok_or(CalendarClockModelError::ArithmeticOverflow)?;
        let completed_ticks = completed_ticks
            .checked_add(1)
            .ok_or(CalendarClockModelError::ArithmeticOverflow)?;
        // 末拍 after 仍属于同一 civil date；完成会话的增加只在此边界发生一次。
        let (completed_shared_sessions, progress) = if completed_ticks == layout.ticks_per_day {
            (
                before
                    .completed_shared_sessions
                    .checked_add(1)
                    .ok_or(CalendarClockModelError::ArithmeticOverflow)?,
                SharedDayProgress::TradingCompleted,
            )
        } else {
            (
                before.completed_shared_sessions,
                SharedDayProgress::Trading { completed_ticks },
            )
        };
        let after = SharedClockInput {
            tick,
            completed_shared_sessions,
            progress,
        };
        let after_projection = calendar_clock_projection(&self.model, layout, after)?;
        Ok(Arc::new(MarketTickContext {
            day: Arc::clone(self),
            layout,
            before,
            after,
            before_projection,
            after_projection,
        }))
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
        expected_source: &Arc<MarketCalendarSource>,
        expected_bindings: &[(StockId, CalendarExchange)],
        expected_date: CivilDate,
        expected_layout: SessionTickLayout,
        expected_before: SharedClockInput,
    ) -> Result<(), MarketCalendarContextError> {
        self.day
            .validate_for(expected_source, expected_bindings, expected_date)?;
        if expected_layout.ticks_per_day != self.layout.ticks_per_day
            || expected_layout.auction_ticks != self.layout.auction_ticks
            || expected_layout.closing_auction_ticks != self.layout.closing_auction_ticks
        {
            return Err(MarketCalendarContextError::TickLayoutMismatch {
                expected: expected_layout,
                actual: self.layout,
            });
        }
        if expected_before.tick != self.before.tick
            || expected_before.completed_shared_sessions != self.before.completed_shared_sessions
            || expected_before.progress != self.before.progress
        {
            return Err(MarketCalendarContextError::BeforeClockMismatch {
                expected: expected_before,
                actual: self.before,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "market_calendar_context_tests.rs"]
mod tests;
