//! raw 单元 fixture 显式构造合法 Calendar 上下文；不在生产路径补默认值。

use crate::calendar::{CalendarExchange, CivilDate, TradingCalendar};
use crate::session::exchange_calendar_model::{
    SessionTickLayout, SharedClockInput, SharedDayProgress,
};
use crate::session::market_calendar_context::MarketCalendarSource;
use std::sync::Arc;

pub(super) fn open_tick(
    tick: u64,
    ticks_per_day: u64,
    auction_ticks: u64,
    closing_auction_ticks: u64,
    stocks: usize,
    date: CivilDate,
) -> Arc<super::MarketTickContext> {
    let calendar = TradingCalendar::current_default_calendar().expect("fixture 官方 Calendar 合法");
    assert!(
        calendar
            .is_trading_day(CalendarExchange::Sse, date)
            .unwrap(),
        "fixture 明确要求 SSE open date"
    );
    let before_tick = tick.checked_sub(1).expect("Start tick 从 1 起");
    let before_sessions = before_tick / ticks_per_day;
    let mut start_date = date;
    for _ in 0..before_sessions {
        start_date = calendar
            .previous_trading_day(CalendarExchange::Sse, start_date)
            .unwrap();
    }
    let bindings = (0..stocks)
        .map(|index| {
            (
                crate::StockId(u32::try_from(index).unwrap()),
                CalendarExchange::Sse,
            )
        })
        .collect();
    let source = MarketCalendarSource::new(calendar, start_date, bindings).unwrap();
    source
        .day(date)
        .unwrap()
        .tick(
            SessionTickLayout {
                ticks_per_day,
                auction_ticks,
                closing_auction_ticks,
            },
            SharedClockInput {
                tick: before_tick,
                completed_shared_sessions: before_sessions,
                progress: SharedDayProgress::Trading {
                    completed_ticks: before_tick % ticks_per_day,
                },
            },
        )
        .unwrap()
}
