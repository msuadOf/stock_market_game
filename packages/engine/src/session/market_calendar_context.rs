//! 同一 source owner 的不可变日历上下文；生产调用方必须持有同一 source owner。

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
pub struct MarketDayContext {
    source: Arc<MarketCalendarSource>,
    model: CalendarDayModel,
}

#[derive(Debug)]
pub struct MarketTickContext {
    day: Arc<MarketDayContext>,
    layout: SessionTickLayout,
    before: SharedClockInput,
    after: SharedClockInput,
    before_projection: CalendarClockProjection,
    after_projection: CalendarClockProjection,
}

impl PartialEq for MarketTickContext {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}
impl Eq for MarketTickContext {}

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
    pub(crate) fn source(&self) -> &Arc<MarketCalendarSource> {
        &self.source
    }
    pub(crate) fn day_model(&self) -> &CalendarDayModel {
        &self.model
    }

    pub(crate) fn validate_for_stock(
        &self,
        expected_source: &Arc<MarketCalendarSource>,
        expected_stock: StockId,
        expected_exchange: CalendarExchange,
        expected_date: CivilDate,
    ) -> Result<(), MarketCalendarContextError> {
        // 完整目录由 World 前置门校验；desk 只点查 spawn 已核的自身绑定。
        self.validate_owner(expected_source)?;
        self.validate_stock_binding(expected_stock, expected_exchange)?;
        self.validate_date(expected_date)
    }

    pub(crate) fn validate_for(
        &self,
        expected_source: &Arc<MarketCalendarSource>,
        expected_bindings: &[(StockId, CalendarExchange)],
        expected_date: CivilDate,
    ) -> Result<(), MarketCalendarContextError> {
        self.validate_owner(expected_source)?;
        let mut expected = BTreeMap::new();
        for &(stock, exchange) in expected_bindings {
            if expected.insert(stock, exchange).is_some() {
                return Err(MarketCalendarContextError::DuplicateExpectedStock { stock });
            }
        }
        // 双向检查完整 StockId 映射，避免同长度替换、缺股或多股被截断比较掩盖。
        for (&stock, &exchange) in &expected {
            self.validate_stock_binding(stock, exchange)?;
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
        self.validate_date(expected_date)
    }

    fn validate_owner(
        &self,
        expected_source: &Arc<MarketCalendarSource>,
    ) -> Result<(), MarketCalendarContextError> {
        if !Arc::ptr_eq(&self.source, expected_source) {
            return Err(MarketCalendarContextError::SourceMismatch);
        }
        Ok(())
    }

    fn validate_stock_binding(
        &self,
        expected_stock: StockId,
        expected_exchange: CalendarExchange,
    ) -> Result<(), MarketCalendarContextError> {
        let actual = match self.model.stock(expected_stock) {
            Ok(row) => Some(row.exchange()),
            Err(CalendarClockModelError::UnknownStock { .. }) => None,
            Err(error) => return Err(error.into()),
        };
        if actual != Some(expected_exchange) {
            return Err(MarketCalendarContextError::BindingMismatch {
                stock: expected_stock,
                expected: Some(expected_exchange),
                actual,
            });
        }
        Ok(())
    }

    fn validate_date(&self, expected_date: CivilDate) -> Result<(), MarketCalendarContextError> {
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
    /// 市场受理/复审相位只使用本拍 before 进度；末拍不会回到下一日开盘。
    pub(crate) fn phase_before(&self) -> crate::TradingPhase {
        let SharedDayProgress::Trading { completed_ticks } = self.before.progress else {
            unreachable!("MarketTickContext 只能由 Trading before 构造")
        };
        let entry = self.layout.auction_ticks - self.layout.auction_ticks / 3;
        if completed_ticks < entry {
            crate::TradingPhase::CallAuction
        } else if completed_ticks < self.layout.auction_ticks {
            crate::TradingPhase::PreOpen
        } else if self.layout.closing_auction_ticks > 0
            && completed_ticks >= self.layout.ticks_per_day - self.layout.closing_auction_ticks
        {
            crate::TradingPhase::ClosingAuction
        } else {
            crate::TradingPhase::Continuous
        }
    }

    pub(crate) fn observation_instant_before(&self) -> crate::CivilInstant {
        let SharedDayProgress::Trading { completed_ticks } = self.before.progress else {
            unreachable!("市场拍必须由 Trading 开始")
        };
        crate::CivilInstant::new(
            self.day.day_model().current_date(),
            super::exchange_calendar_model::intraday_observation_second(
                completed_ticks,
                self.layout.ticks_per_day,
                self.layout.auction_ticks,
                self.layout.closing_auction_ticks,
            ),
        )
        .expect("已验证市场日历时点")
    }

    /// observe 在真实完成本拍后的日内时点运行；civil date 保持当日，末拍为 15:00。
    pub(crate) fn observation_instant_after(&self) -> crate::calendar::CivilInstant {
        let completed_ticks = match self.after.progress {
            SharedDayProgress::Trading { completed_ticks } => completed_ticks,
            SharedDayProgress::TradingCompleted => self.layout.ticks_per_day,
            SharedDayProgress::ClosedDay => unreachable!("市场拍不能以 ClosedDay 完成"),
        };
        let second = super::exchange_calendar_model::intraday_observation_second(
            completed_ticks,
            self.layout.ticks_per_day,
            self.layout.auction_ticks,
            self.layout.closing_auction_ticks,
        );
        crate::calendar::CivilInstant::new(self.day.day_model().current_date(), second)
            .expect("已验证 Calendar context 的日内观察时点合法")
    }

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

    pub(crate) fn validate_for_stock(
        &self,
        expected_source: &Arc<MarketCalendarSource>,
        expected_stock: StockId,
        expected_exchange: CalendarExchange,
        expected_date: CivilDate,
        expected_layout: SessionTickLayout,
        expected_before: SharedClockInput,
    ) -> Result<(), MarketCalendarContextError> {
        self.day.validate_for_stock(
            expected_source,
            expected_stock,
            expected_exchange,
            expected_date,
        )?;
        self.validate_clock(expected_layout, expected_before)
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
        self.validate_clock(expected_layout, expected_before)
    }

    fn validate_clock(
        &self,
        expected_layout: SessionTickLayout,
        expected_before: SharedClockInput,
    ) -> Result<(), MarketCalendarContextError> {
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
