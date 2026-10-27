//! Session 唯一 Calendar owner 与按自然日复用的派生上下文。

use std::sync::Arc;

use super::exchange_calendar_model::{SessionTickLayout, SharedClockInput, SharedDayProgress};
use super::market_calendar_context::{
    MarketCalendarContextError, MarketCalendarSource, MarketDayContext, MarketTickContext,
};
use super::{CivilClock, CivilClockError, GameSession, SessionError, SessionSetup, StepFatal};
use crate::calendar::CalendarExchange;
use crate::stock::{StockCatalog, StockId};

impl GameSession {
    pub(super) fn calendar_source_for(
        setup: &SessionSetup,
        catalog: &StockCatalog,
        clock: &CivilClock,
    ) -> Result<Arc<MarketCalendarSource>, SessionError> {
        let bindings = Self::calendar_bindings_for(setup, catalog).map_err(|detail| {
            SessionError::CivilClock(CivilClockError::SaveInconsistent { detail })
        })?;
        MarketCalendarSource::new(clock.calendar().clone(), setup.start_date, bindings).map_err(
            |error| {
                SessionError::CivilClock(CivilClockError::SaveInconsistent {
                    detail: format!("构造市场 Calendar owner 失败：{error}"),
                })
            },
        )
    }

    fn calendar_bindings_for(
        setup: &SessionSetup,
        catalog: &StockCatalog,
    ) -> Result<Vec<(StockId, CalendarExchange)>, String> {
        if setup.stocks.len() != catalog.iter().count() {
            return Err("setup 与 StockCatalog 股数不一致".to_owned());
        }
        setup
            .stocks
            .iter()
            .map(|stock| {
                let id = catalog
                    .id(&stock.code.0)
                    .ok_or_else(|| format!("Calendar binding 缺少证券代码 {}", stock.code.0))?;
                Ok((
                    id,
                    super::civil_clock::session_calendar_exchange(stock.exchange),
                ))
            })
            .collect()
    }

    pub(crate) fn market_calendar_source(&self) -> &Arc<MarketCalendarSource> {
        &self.market_calendar_source
    }

    pub(crate) fn market_calendar_bindings(
        &self,
    ) -> Result<Vec<(StockId, CalendarExchange)>, StepFatal> {
        let bindings = Self::calendar_bindings_for(&self.state.setup, &self.state.stocks).map_err(
            |description| StepFatal::InvariantViolation {
                location: "GameSession::market_calendar_bindings".to_owned(),
                description,
            },
        )?;
        if bindings.len() != self.state.markets.len()
            || bindings
                .iter()
                .any(|(stock, _)| !self.state.markets.contains_key(stock))
        {
            return Err(StepFatal::InvariantViolation {
                location: "GameSession::market_calendar_bindings".to_owned(),
                description: "Calendar catalog 与实际 markets 不一一对应".to_owned(),
            });
        }
        Ok(bindings)
    }

    pub(crate) fn stock_calendar_minute(&self, stock: StockId) -> Result<Option<u64>, StepFatal> {
        let (_, tick) = self.prepare_market_calendar_tick()?;
        let row = tick.before_projection().stock(stock).map_err(|error| {
            StepFatal::InvariantViolation {
                location: "GameSession::stock_calendar_minute".to_owned(),
                description: error.to_string(),
            }
        })?;
        Ok(row
            .date_session_ordinal()
            .map(|_| row.completed_minute_count()))
    }

    pub(crate) fn stock_calendar_day(&self, stock: StockId) -> Result<Option<u64>, StepFatal> {
        let day = self.prepare_market_calendar_day()?;
        let row = day
            .day_model()
            .stock(stock)
            .map_err(|error| StepFatal::InvariantViolation {
                location: "GameSession::stock_calendar_day".to_owned(),
                description: error.to_string(),
            })?;
        Ok(matches!(row.status(), crate::calendar::DayStatus::Trading)
            .then_some(row.sessions_before_date()))
    }

    pub(crate) fn stock_calendar_days(
        &self,
    ) -> Result<std::collections::BTreeMap<StockId, Option<u64>>, StepFatal> {
        let day = self.prepare_market_calendar_day()?;
        Ok(day
            .day_model()
            .stocks()
            .iter()
            .map(|(&stock, row)| {
                let ordinal = matches!(row.status(), crate::calendar::DayStatus::Trading)
                    .then_some(row.sessions_before_date());
                (stock, ordinal)
            })
            .collect())
    }

    pub(crate) fn prepare_market_calendar_day(&self) -> Result<Arc<MarketDayContext>, StepFatal> {
        let fail = |error: MarketCalendarContextError| StepFatal::InvariantViolation {
            location: "GameSession::prepare_market_calendar_day".to_owned(),
            description: error.to_string(),
        };
        let day = match &self.market_calendar_day {
            Some(day) => Arc::clone(day),
            None => self
                .market_calendar_source
                .day(self.civil_date())
                .map_err(fail)?,
        };
        day.validate_for(
            &self.market_calendar_source,
            &self.market_calendar_bindings()?,
            self.civil_date(),
        )
        .map_err(fail)?;
        Ok(day)
    }

    /// 只准备，不安装；任何失败都发生在 ingress 截取、ID 分配和 Start 之前。
    pub(crate) fn prepare_market_calendar_tick(
        &self,
    ) -> Result<(Arc<MarketDayContext>, Arc<MarketTickContext>), StepFatal> {
        let fail = |error: MarketCalendarContextError| StepFatal::InvariantViolation {
            location: "GameSession::prepare_market_calendar_tick".to_owned(),
            description: error.to_string(),
        };
        let day = self.prepare_market_calendar_day()?;
        let layout = SessionTickLayout {
            ticks_per_day: self.state.setup.ticks_per_day,
            auction_ticks: self.state.setup.auction_ticks,
            closing_auction_ticks: self.state.setup.closing_auction_ticks,
        };
        let model = day.day_model();
        let before_sessions = model.shared_sessions_before_date();
        let base = before_sessions
            .checked_mul(layout.ticks_per_day)
            .ok_or_else(|| {
                fail(
                    super::exchange_calendar_model::CalendarClockModelError::ArithmeticOverflow
                        .into(),
                )
            })?;
        let end = base.checked_add(layout.ticks_per_day).ok_or_else(|| {
            fail(super::exchange_calendar_model::CalendarClockModelError::ArithmeticOverflow.into())
        })?;
        let progress = if !model.shared_trading_on_date() {
            SharedDayProgress::ClosedDay
        } else if self.state.tick == end {
            SharedDayProgress::TradingCompleted
        } else {
            let completed_ticks = self.state.tick.checked_sub(base).ok_or_else(|| {
                fail(
                    super::exchange_calendar_model::CalendarClockModelError::CountersMismatch {
                        expected_tick: base,
                        actual_tick: self.state.tick,
                        expected_shared_sessions: before_sessions,
                        actual_shared_sessions: u64::from(self.state.day),
                    }
                    .into(),
                )
            })?;
            SharedDayProgress::Trading { completed_ticks }
        };
        let context = day
            .tick(
                layout,
                SharedClockInput {
                    tick: self.state.tick,
                    completed_shared_sessions: u64::from(self.state.day),
                    progress,
                },
            )
            .map_err(fail)?;
        Ok((day, context))
    }

    /// 只接收前置准备并已随本拍成功提交的同 owner cache。
    pub(crate) fn install_market_calendar_day(&mut self, day: Arc<MarketDayContext>) {
        self.market_calendar_day = Some(day);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> GameSession {
        let mut setup = super::super::npc_working_quote_tests::retail_quote_setup();
        setup.start_date = crate::calendar::CivilDate::from_iso("2030-01-02").unwrap();
        setup.ticks_per_day = 2;
        setup.auction_ticks = 0;
        setup.closing_auction_ticks = 0;
        GameSession::new(setup, 931).unwrap()
    }

    #[test]
    fn preparing_live_context_does_not_publish_cache_or_mutate_business_state() {
        let session = session();
        let before = session.business_state_hash().unwrap();
        let (_, tick) = session.prepare_market_calendar_tick().unwrap();
        assert_eq!(tick.before_input().tick, 0);
        assert_eq!(tick.after_input().tick, 1);
        assert_eq!(tick.before_projection().completed_shared_minute_count(), 0);
        assert_eq!(tick.after_projection().completed_shared_minute_count(), 120);
        assert!(session.market_calendar_day.is_none());
        assert_eq!(session.business_state_hash().unwrap(), before);
    }

    #[test]
    fn cached_day_and_tick_shadow_keep_same_source_owner_and_day_arc() {
        let mut session = session();
        let (day, _) = session.prepare_market_calendar_tick().unwrap();
        session.install_market_calendar_day(Arc::clone(&day));
        let (reused, tick) = session.prepare_market_calendar_tick().unwrap();
        assert!(Arc::ptr_eq(&day, &reused));
        assert!(Arc::ptr_eq(&day, tick.day()));
        let shadow = session.clone_for_tick_shadow().unwrap();
        assert!(Arc::ptr_eq(
            session.market_calendar_source(),
            shadow.market_calendar_source()
        ));
        session.commit_tick_shadow(shadow);
        assert!(Arc::ptr_eq(
            session.market_calendar_day.as_ref().unwrap(),
            &day
        ));
    }

    #[test]
    fn live_calendar_refuses_clock_drift_before_input_consumption() {
        let mut session = session();
        let seq = session.current_seq();
        session.state.day = 1;
        let error = session.prepare_market_calendar_tick().unwrap_err();
        assert!(error.to_string().contains("计数不一致"));
        assert_eq!(session.current_seq(), seq);
        assert!(session.market_calendar_day.is_none());
        assert!(session.runtime_driver.is_none());
    }

    #[test]
    fn restore_creates_new_owner_and_old_context_cannot_cross_sessions() {
        let session = session();
        let (old, _) = session.prepare_market_calendar_tick().unwrap();
        let restored = GameSession::restore(&session.save().unwrap()).unwrap();
        assert!(!Arc::ptr_eq(
            session.market_calendar_source(),
            restored.market_calendar_source()
        ));
        assert!(matches!(
            old.validate_for(
                restored.market_calendar_source(),
                &restored.market_calendar_bindings().unwrap(),
                restored.civil_date()
            ),
            Err(MarketCalendarContextError::SourceMismatch)
        ));
    }
}
