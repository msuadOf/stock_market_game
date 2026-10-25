//! 冻结 Calendar 与显式会话进度的冷路径纯投影；不读取 live 行情或存档。

use std::collections::{BTreeMap, BTreeSet};

use crate::calendar::{CalendarError, CalendarExchange, CivilDate, DayStatus, TradingCalendar};
use crate::observation::{completed_market_minute_count, ObservationError};
use crate::stock::StockId;

#[derive(Clone, Copy, Debug)]
pub(crate) struct SessionTickLayout {
    pub ticks_per_day: u64,
    pub auction_ticks: u64,
    pub closing_auction_ticks: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SharedDayProgress {
    ClosedDay,
    Trading { completed_ticks: u64 },
    TradingCompleted,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SharedClockInput {
    pub tick: u64,
    pub completed_shared_sessions: u64,
    pub progress: SharedDayProgress,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CalendarClockModelError {
    #[error("Calendar 查询失败：{0}")]
    Calendar(#[from] CalendarError),
    #[error("证券绑定不能为空")]
    EmptyBindings,
    #[error("重复 StockId：{stock:?}")]
    DuplicateStock { stock: StockId },
    #[error("current_date {current} 早于 start_date {start}")]
    CurrentBeforeStart {
        start: CivilDate,
        current: CivilDate,
    },
    #[error("无效 tick 布局：total={ticks_per_day}, auction={auction_ticks}, closing={closing_auction_ticks}")]
    InvalidTickLayout {
        ticks_per_day: u64,
        auction_ticks: u64,
        closing_auction_ticks: u64,
    },
    #[error("进度 {progress:?} 与 shared_open={shared_open} 不一致")]
    ProgressCalendarMismatch {
        progress: SharedDayProgress,
        shared_open: bool,
    },
    #[error("当日完成 tick {completed_ticks} 不在 0..{ticks_per_day}")]
    TradingTicksOutsideSession {
        completed_ticks: u64,
        ticks_per_day: u64,
    },
    #[error("计数不一致：tick={actual_tick}（应为 {expected_tick}），shared_sessions={actual_shared_sessions}（应为 {expected_shared_sessions}）")]
    CountersMismatch {
        expected_tick: u64,
        actual_tick: u64,
        expected_shared_sessions: u64,
        actual_shared_sessions: u64,
    },
    #[error("日历时钟算术溢出")]
    ArithmeticOverflow,
    #[error("分钟映射失败：{0}")]
    Observation(#[from] ObservationError),
    #[error("未知 StockId：{stock:?}")]
    UnknownStock { stock: StockId },
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct StockCalendarDay {
    exchange: CalendarExchange,
    status: DayStatus,
    sessions_before_date: u64,
}

impl StockCalendarDay {
    pub(crate) fn exchange(&self) -> CalendarExchange {
        self.exchange
    }
    pub(crate) fn status(&self) -> &DayStatus {
        &self.status
    }
    pub(crate) fn sessions_before_date(&self) -> u64 {
        self.sessions_before_date
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct CalendarDayModel {
    start_date: CivilDate,
    current_date: CivilDate,
    shared_sessions_before_date: u64,
    shared_trading_on_date: bool,
    stocks: BTreeMap<StockId, StockCalendarDay>,
}

impl CalendarDayModel {
    pub(crate) fn start_date(&self) -> CivilDate {
        self.start_date
    }
    pub(crate) fn current_date(&self) -> CivilDate {
        self.current_date
    }
    /// 半开区间 [start_date, current_date) 的配置交易所 union 会话数量。
    pub(crate) fn shared_sessions_before_date(&self) -> u64 {
        self.shared_sessions_before_date
    }
    pub(crate) fn shared_trading_on_date(&self) -> bool {
        self.shared_trading_on_date
    }
    pub(crate) fn stocks(&self) -> &BTreeMap<StockId, StockCalendarDay> {
        &self.stocks
    }
    pub(crate) fn stock(
        &self,
        stock: StockId,
    ) -> Result<&StockCalendarDay, CalendarClockModelError> {
        self.stocks
            .get(&stock)
            .ok_or(CalendarClockModelError::UnknownStock { stock })
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct StockCalendarClock {
    sessions_before_date: u64,
    trading_on_date: bool,
    session_completed: bool,
    current_date_completed_minutes: u16,
    completed_minutes: u64,
}

impl StockCalendarClock {
    pub(crate) fn completed_session_count(&self) -> u64 {
        self.sessions_before_date + u64::from(self.session_completed)
    }
    /// 当前 civil date 开市时的 0-based 会话 ordinal；收盘后仍指向当日会话。
    pub(crate) fn date_session_ordinal(&self) -> Option<u64> {
        self.trading_on_date.then_some(self.sessions_before_date)
    }
    pub(crate) fn current_date_completed_minute_count(&self) -> u16 {
        self.current_date_completed_minutes
    }
    pub(crate) fn completed_minute_count(&self) -> u64 {
        self.completed_minutes
    }
    /// 当前日期闭市则无新分钟键；开市时为当日第一根 0-based 绝对分钟键。
    pub(crate) fn current_date_first_minute_key(&self) -> Option<u64> {
        self.trading_on_date
            .then_some(self.completed_minutes - u64::from(self.current_date_completed_minutes))
    }
    /// 闭市时可指向上一真实会话末分钟，不表示当前日期产生新样本。
    pub(crate) fn last_completed_minute_key(&self) -> Option<u64> {
        self.completed_minutes.checked_sub(1)
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct CalendarClockProjection {
    completed_shared_sessions: u64,
    completed_shared_minutes: u64,
    stocks: BTreeMap<StockId, StockCalendarClock>,
}

impl CalendarClockProjection {
    pub(crate) fn completed_shared_session_count(&self) -> u64 {
        self.completed_shared_sessions
    }
    pub(crate) fn completed_shared_minute_count(&self) -> u64 {
        self.completed_shared_minutes
    }
    pub(crate) fn stocks(&self) -> &BTreeMap<StockId, StockCalendarClock> {
        &self.stocks
    }
    pub(crate) fn stock(
        &self,
        stock: StockId,
    ) -> Result<&StockCalendarClock, CalendarClockModelError> {
        self.stocks
            .get(&stock)
            .ok_or(CalendarClockModelError::UnknownStock { stock })
    }
}

/// 仅按 bindings 实际出现的交易所，对 [start_date, current_date) 逐日计数。
/// 这是独立冷路径 oracle，不能直接放入每 tick 的生产热路径。
pub(crate) fn calendar_day_model(
    calendar: &TradingCalendar,
    start_date: CivilDate,
    current_date: CivilDate,
    bindings: &[(StockId, CalendarExchange)],
) -> Result<CalendarDayModel, CalendarClockModelError> {
    calendar.validate_runtime_start(start_date)?;
    calendar.validate_runtime_start(current_date)?;
    if current_date < start_date {
        return Err(CalendarClockModelError::CurrentBeforeStart {
            start: start_date,
            current: current_date,
        });
    }
    if bindings.is_empty() {
        return Err(CalendarClockModelError::EmptyBindings);
    }
    let mut identities = BTreeSet::new();
    for (stock, _) in bindings {
        if !identities.insert(*stock) {
            return Err(CalendarClockModelError::DuplicateStock { stock: *stock });
        }
    }
    // 先按 exchange 去重；一段日期只扫描一次，不按股票数量重复扫描。
    let mut counts: BTreeMap<_, u64> = bindings
        .iter()
        .map(|(_, exchange)| (*exchange, 0))
        .collect();
    let mut shared_before = 0u64;
    let mut date = start_date;
    while date < current_date {
        let mut any_open = false;
        for (exchange, count) in &mut counts {
            if calendar.is_trading_day(*exchange, date)? {
                *count = count
                    .checked_add(1)
                    .ok_or(CalendarClockModelError::ArithmeticOverflow)?;
                any_open = true;
            }
        }
        if any_open {
            shared_before = shared_before
                .checked_add(1)
                .ok_or(CalendarClockModelError::ArithmeticOverflow)?;
        }
        date = date.next().map_err(CalendarError::from)?;
    }
    let mut stocks = BTreeMap::new();
    let mut shared_open = false;
    for (exchange, before) in counts {
        let status = calendar.day_status(exchange, current_date)?;
        shared_open |= status == DayStatus::Trading;
        for (stock, _) in bindings
            .iter()
            .filter(|(_, binding_exchange)| *binding_exchange == exchange)
        {
            stocks.insert(
                *stock,
                StockCalendarDay {
                    exchange,
                    status: status.clone(),
                    sessions_before_date: before,
                },
            );
        }
    }
    Ok(CalendarDayModel {
        start_date,
        current_date,
        shared_sessions_before_date: shared_before,
        shared_trading_on_date: shared_open,
        stocks,
    })
}

/// progress 同时受 Calendar、tick、completed_shared_sessions 约束；不按 tick 取模猜测阶段。
pub(crate) fn calendar_clock_projection(
    day: &CalendarDayModel,
    layout: SessionTickLayout,
    input: SharedClockInput,
) -> Result<CalendarClockProjection, CalendarClockModelError> {
    let invalid_layout = || CalendarClockModelError::InvalidTickLayout {
        ticks_per_day: layout.ticks_per_day,
        auction_ticks: layout.auction_ticks,
        closing_auction_ticks: layout.closing_auction_ticks,
    };
    let windows = layout
        .auction_ticks
        .checked_add(layout.closing_auction_ticks)
        .filter(|windows| *windows < layout.ticks_per_day)
        .ok_or_else(invalid_layout)?;
    let continuous_ticks = layout
        .ticks_per_day
        .checked_sub(windows)
        .ok_or_else(invalid_layout)?;
    let is_trading_progress = !matches!(input.progress, SharedDayProgress::ClosedDay);
    if is_trading_progress != day.shared_trading_on_date {
        return Err(CalendarClockModelError::ProgressCalendarMismatch {
            progress: input.progress,
            shared_open: day.shared_trading_on_date,
        });
    }
    let before = day.shared_sessions_before_date;
    let (expected_sessions, completed_ticks, date_minutes) = match input.progress {
        SharedDayProgress::ClosedDay => (before, 0, 0),
        SharedDayProgress::Trading { completed_ticks } => {
            if completed_ticks >= layout.ticks_per_day {
                return Err(CalendarClockModelError::TradingTicksOutsideSession {
                    completed_ticks,
                    ticks_per_day: layout.ticks_per_day,
                });
            }
            let continuous_completed = if completed_ticks > layout.auction_ticks {
                (completed_ticks - layout.auction_ticks).min(continuous_ticks)
            } else {
                0
            };
            (
                before,
                completed_ticks,
                completed_market_minute_count(continuous_completed, continuous_ticks)?,
            )
        }
        SharedDayProgress::TradingCompleted => (
            before
                .checked_add(1)
                .ok_or(CalendarClockModelError::ArithmeticOverflow)?,
            0,
            240,
        ),
    };
    let expected_tick = expected_sessions
        .checked_mul(layout.ticks_per_day)
        .and_then(|base| base.checked_add(completed_ticks))
        .ok_or(CalendarClockModelError::ArithmeticOverflow)?;
    if input.tick != expected_tick || input.completed_shared_sessions != expected_sessions {
        return Err(CalendarClockModelError::CountersMismatch {
            expected_tick,
            actual_tick: input.tick,
            expected_shared_sessions: expected_sessions,
            actual_shared_sessions: input.completed_shared_sessions,
        });
    }
    let completed_shared_minutes = minute_count(before, date_minutes)?;
    let mut stocks = BTreeMap::new();
    for (stock, row) in &day.stocks {
        let trading_on_date = row.status == DayStatus::Trading;
        let session_completed =
            trading_on_date && matches!(input.progress, SharedDayProgress::TradingCompleted);
        // getter 的会话加法在构造时先校验；输出字段私有，调用方不能绕过此不变式。
        row.sessions_before_date
            .checked_add(u64::from(session_completed))
            .ok_or(CalendarClockModelError::ArithmeticOverflow)?;
        let current_date_completed_minutes = if trading_on_date { date_minutes } else { 0 };
        stocks.insert(
            *stock,
            StockCalendarClock {
                sessions_before_date: row.sessions_before_date,
                trading_on_date,
                session_completed,
                current_date_completed_minutes,
                completed_minutes: minute_count(
                    row.sessions_before_date,
                    current_date_completed_minutes,
                )?,
            },
        );
    }
    Ok(CalendarClockProjection {
        completed_shared_sessions: expected_sessions,
        completed_shared_minutes,
        stocks,
    })
}

fn minute_count(
    sessions_before_date: u64,
    current_date_minutes: u16,
) -> Result<u64, CalendarClockModelError> {
    sessions_before_date
        .checked_mul(240)
        .and_then(|base| base.checked_add(u64::from(current_date_minutes)))
        .ok_or(CalendarClockModelError::ArithmeticOverflow)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::{
        embedded_lunar_facts, CalendarPolicy, CalendarPolicySpec, ClosedReason, HolidayKind,
        OfficialCoverageEntry, SimulatedFallbackRuleset,
    };
    use CalendarExchange::{Sse, Szse};
    use SharedDayProgress::{ClosedDay, Trading, TradingCompleted};

    const SSE: StockId = StockId(7);
    const SZSE: StockId = StockId(1009);
    const BINDINGS: [(StockId, CalendarExchange); 2] = [(SSE, Sse), (SZSE, Szse)];
    fn date(value: &str) -> CivilDate {
        CivilDate::from_iso(value).unwrap()
    }
    fn calendar(mixed: bool) -> TradingCalendar {
        let official_coverage = if mixed {
            vec![OfficialCoverageEntry::new(
                Sse,
                2030,
                vec![(date("2030-01-01"), date("2030-01-02"))],
                "synthetic-test".into(),
                "synthetic-test".into(),
            )]
        } else {
            vec![]
        };
        TradingCalendar::from_policy(
            CalendarPolicy::from_parts(CalendarPolicySpec {
                algorithm_version: 1,
                default_start: date("2030-01-01"),
                runtime_min_start: date("2000-01-01"),
                runtime_max_end: date("2099-12-31"),
                init_only_min_start: date("1998-01-01"),
                official_coverage,
                simulated_fallback: SimulatedFallbackRuleset::new(
                    1,
                    2026,
                    embedded_lunar_facts().unwrap(),
                )
                .unwrap(),
            })
            .unwrap(),
        )
        .unwrap()
    }
    fn model(current: &str) -> CalendarDayModel {
        calendar_day_model(
            &calendar(true),
            date("2030-01-02"),
            date(current),
            &BINDINGS,
        )
        .unwrap()
    }
    fn layout() -> SessionTickLayout {
        SessionTickLayout {
            ticks_per_day: 2,
            auction_ticks: 0,
            closing_auction_ticks: 0,
        }
    }
    fn input(
        tick: u64,
        completed_shared_sessions: u64,
        progress: SharedDayProgress,
    ) -> SharedClockInput {
        SharedClockInput {
            tick,
            completed_shared_sessions,
            progress,
        }
    }
    fn project(
        current: &str,
        tick: u64,
        sessions: u64,
        progress: SharedDayProgress,
    ) -> CalendarClockProjection {
        calendar_clock_projection(&model(current), layout(), input(tick, sessions, progress))
            .unwrap()
    }
    // 手算值来自合成冻结 Calendar 与 240 分钟约定，未读取任何 live 历史。
    fn assert_clock(
        row: &StockCalendarClock,
        sessions: u64,
        ordinal: Option<u64>,
        minutes: u16,
        total: u64,
        first: Option<u64>,
        last: Option<u64>,
    ) {
        assert_eq!(row.completed_session_count(), sessions);
        assert_eq!(row.date_session_ordinal(), ordinal);
        assert_eq!(row.current_date_completed_minute_count(), minutes);
        assert_eq!(row.completed_minute_count(), total);
        assert_eq!(row.current_date_first_minute_key(), first);
        assert_eq!(row.last_completed_minute_key(), last);
    }

    #[test]
    fn a01_start_is_half_open_and_closed_stock_has_no_minute_key() {
        let day = model("2030-01-02");
        assert_eq!(day.start_date(), date("2030-01-02"));
        assert_eq!(day.current_date(), day.start_date());
        assert_eq!(day.shared_sessions_before_date(), 0);
        assert!(day.shared_trading_on_date());
        let p = project("2030-01-02", 0, 0, Trading { completed_ticks: 0 });
        assert_clock(p.stock(SSE).unwrap(), 0, None, 0, 0, None, None);
        assert_clock(p.stock(SZSE).unwrap(), 0, Some(0), 0, 0, Some(0), None);
    }
    #[test]
    fn a02_only_open_stock_accumulates_minutes() {
        let p = project("2030-01-02", 1, 0, Trading { completed_ticks: 1 });
        assert_eq!(p.completed_shared_minute_count(), 120);
        assert_clock(p.stock(SSE).unwrap(), 0, None, 0, 0, None, None);
        assert_clock(
            p.stock(SZSE).unwrap(),
            0,
            Some(0),
            120,
            120,
            Some(0),
            Some(119),
        );
    }
    #[test]
    fn a03_last_tick_preserves_date_ordinal_without_double_counting() {
        let p = project("2030-01-02", 2, 1, TradingCompleted);
        assert_eq!(p.completed_shared_session_count(), 1);
        assert_eq!(p.completed_shared_minute_count(), 240);
        assert_clock(p.stock(SSE).unwrap(), 0, None, 0, 0, None, None);
        assert_clock(
            p.stock(SZSE).unwrap(),
            1,
            Some(0),
            240,
            240,
            Some(0),
            Some(239),
        );
    }
    #[test]
    fn a04_next_date_start_distinguishes_identical_modulo_tick() {
        let d = model("2030-01-03");
        assert_eq!(d.shared_sessions_before_date(), 1);
        assert_eq!(d.stock(SSE).unwrap().sessions_before_date(), 0);
        assert_eq!(d.stock(SZSE).unwrap().sessions_before_date(), 1);
        let p = project("2030-01-03", 2, 1, Trading { completed_ticks: 0 });
        assert_clock(p.stock(SSE).unwrap(), 0, Some(0), 0, 0, Some(0), None);
        assert_clock(
            p.stock(SZSE).unwrap(),
            1,
            Some(1),
            0,
            240,
            Some(240),
            Some(239),
        );
    }
    #[test]
    fn a05_stock_clocks_have_distinct_absolute_minute_keys() {
        let p = project("2030-01-03", 3, 1, Trading { completed_ticks: 1 });
        assert_eq!(p.completed_shared_minute_count(), 360);
        assert_clock(
            p.stock(SSE).unwrap(),
            0,
            Some(0),
            120,
            120,
            Some(0),
            Some(119),
        );
        assert_clock(
            p.stock(SZSE).unwrap(),
            1,
            Some(1),
            120,
            360,
            Some(240),
            Some(359),
        );
    }
    #[test]
    fn a06_closed_date_keeps_previous_last_key_without_current_key() {
        let d = model("2030-01-05");
        assert_eq!(d.shared_sessions_before_date(), 3);
        assert!(!d.shared_trading_on_date());
        let p = project("2030-01-05", 6, 3, ClosedDay);
        assert_clock(p.stock(SSE).unwrap(), 2, None, 0, 480, None, Some(479));
        assert_clock(p.stock(SZSE).unwrap(), 3, None, 0, 720, None, Some(719));
    }
    #[test]
    fn a07_single_exchange_ignores_unconfigured_open_exchange() {
        let d = calendar_day_model(
            &calendar(true),
            date("2030-01-02"),
            date("2030-01-02"),
            &[(SSE, Sse)],
        )
        .unwrap();
        assert!(!d.shared_trading_on_date());
        let p = calendar_clock_projection(&d, layout(), input(0, 0, ClosedDay)).unwrap();
        assert_eq!(p.completed_shared_minute_count(), 0);
        for progress in [Trading { completed_ticks: 0 }, TradingCompleted] {
            assert!(matches!(
                calendar_clock_projection(&d, layout(), input(0, 0, progress)),
                Err(CalendarClockModelError::ProgressCalendarMismatch {
                    shared_open: false,
                    ..
                })
            ));
        }
    }
    #[test]
    fn a08_shared_union_deduplicates_exchange_and_is_order_independent() {
        let c = calendar(true);
        let bindings = [(SSE, Szse), (SZSE, Szse)];
        let a = calendar_day_model(&c, date("2030-01-02"), date("2030-01-05"), &bindings).unwrap();
        let b = calendar_day_model(
            &c,
            date("2030-01-02"),
            date("2030-01-05"),
            &[bindings[1], bindings[0]],
        )
        .unwrap();
        assert_eq!(a, b);
        assert_eq!(a.shared_sessions_before_date(), 3);
        assert!(a.stocks().values().all(|s| s.sessions_before_date() == 3));
    }
    #[test]
    fn a09_sparse_stock_identity_and_unknown_lookup_are_explicit() {
        let c = calendar(true);
        let d = calendar_day_model(
            &c,
            date("2030-01-02"),
            date("2030-01-02"),
            &[BINDINGS[1], BINDINGS[0]],
        )
        .unwrap();
        assert_eq!(
            d.stocks().keys().copied().collect::<Vec<_>>(),
            vec![SSE, SZSE]
        );
        assert_eq!(d.stock(SSE).unwrap().exchange(), Sse);
        assert_eq!(d.stock(SZSE).unwrap().exchange(), Szse);
        assert!(matches!(
            d.stock(StockId(0)),
            Err(CalendarClockModelError::UnknownStock { stock: StockId(0) })
        ));
        let p =
            calendar_clock_projection(&d, layout(), input(0, 0, Trading { completed_ticks: 0 }))
                .unwrap();
        assert_eq!(p.stocks().len(), 2);
        assert!(matches!(
            p.stock(StockId(0)),
            Err(CalendarClockModelError::UnknownStock { stock: StockId(0) })
        ));
        assert_clock(p.stock(SSE).unwrap(), 0, None, 0, 0, None, None);
    }
    #[test]
    fn a10_closed_reasons_preserve_each_exchange_source() {
        let c = calendar(true);
        let d = calendar_day_model(&c, date("2030-01-01"), date("2030-01-01"), &BINDINGS).unwrap();
        let reversed = calendar_day_model(
            &c,
            date("2030-01-01"),
            date("2030-01-01"),
            &[BINDINGS[1], BINDINGS[0]],
        )
        .unwrap();
        assert_eq!(d, reversed);
        assert!(!d.shared_trading_on_date());
        assert_eq!(
            d.stock(SSE).unwrap().status(),
            &DayStatus::Closed(ClosedReason::OfficialHoliday {
                citation_id: "synthetic-test".into()
            })
        );
        assert_eq!(
            d.stock(SZSE).unwrap().status(),
            &DayStatus::Closed(ClosedReason::SimulatedHoliday(HolidayKind::NewYearDay))
        );
    }
    #[test]
    fn a11_closing_auction_has_240_minutes_before_session_completion() {
        let d = model("2030-01-02");
        let l = SessionTickLayout {
            ticks_per_day: 12,
            auction_ticks: 3,
            closing_auction_ticks: 3,
        };
        for (ticks, minutes) in [(0, 0), (3, 0), (4, 40), (9, 240), (11, 240)] {
            let p = calendar_clock_projection(
                &d,
                l,
                input(
                    ticks,
                    0,
                    Trading {
                        completed_ticks: ticks,
                    },
                ),
            )
            .unwrap();
            assert_eq!(p.completed_shared_session_count(), 0, "ticks={ticks}");
            assert_eq!(
                p.stock(SZSE).unwrap().current_date_completed_minute_count(),
                minutes,
                "ticks={ticks}"
            );
            assert_eq!(p.stock(SZSE).unwrap().completed_session_count(), 0);
            assert_eq!(p.stock(SSE).unwrap().completed_minute_count(), 0);
        }
        let p = calendar_clock_projection(&d, l, input(12, 1, TradingCompleted)).unwrap();
        assert_clock(
            p.stock(SZSE).unwrap(),
            1,
            Some(0),
            240,
            240,
            Some(0),
            Some(239),
        );
    }
    #[test]
    fn a12_empty_and_duplicate_stock_bindings_are_rejected() {
        let c = calendar(true);
        let s = date("2030-01-02");
        assert!(matches!(
            calendar_day_model(&c, s, s, &[]),
            Err(CalendarClockModelError::EmptyBindings)
        ));
        for exchange in [Sse, Szse] {
            assert!(matches!(
                calendar_day_model(&c, s, s, &[(SSE, Sse), (SSE, exchange)]),
                Err(CalendarClockModelError::DuplicateStock { stock: SSE })
            ));
        }
    }
    #[test]
    fn a13_trading_ticks_cannot_silently_become_completed() {
        assert!(matches!(
            calendar_clock_projection(
                &model("2030-01-02"),
                layout(),
                input(2, 1, Trading { completed_ticks: 2 })
            ),
            Err(CalendarClockModelError::TradingTicksOutsideSession {
                completed_ticks: 2,
                ticks_per_day: 2
            })
        ));
    }
    #[test]
    fn a14_progress_must_match_frozen_calendar() {
        assert!(matches!(
            calendar_clock_projection(&model("2030-01-02"), layout(), input(0, 0, ClosedDay)),
            Err(CalendarClockModelError::ProgressCalendarMismatch {
                progress: ClosedDay,
                shared_open: true
            })
        ));
        for progress in [Trading { completed_ticks: 0 }, TradingCompleted] {
            assert!(matches!(
                calendar_clock_projection(&model("2030-01-05"), layout(), input(6, 3, progress)),
                Err(CalendarClockModelError::ProgressCalendarMismatch {
                    shared_open: false,
                    ..
                })
            ));
        }
    }
    #[test]
    fn a15_tick_and_session_counter_errors_keep_expected_actual_context() {
        let d = model("2030-01-02");
        for (tick, sessions) in [(0, 0), (2, 0), (1, 1)] {
            assert!(
                matches!(calendar_clock_projection(&d, layout(), input(tick, sessions, Trading { completed_ticks: 1 })),
                Err(CalendarClockModelError::CountersMismatch { expected_tick: 1, actual_tick, expected_shared_sessions: 0, actual_shared_sessions })
                if actual_tick == tick && actual_shared_sessions == sessions)
            );
        }
        assert!(matches!(
            calendar_clock_projection(&d, layout(), input(2, 0, TradingCompleted)),
            Err(CalendarClockModelError::CountersMismatch {
                expected_tick: 2,
                expected_shared_sessions: 1,
                actual_shared_sessions: 0,
                ..
            })
        ));
        assert!(matches!(
            calendar_clock_projection(&model("2030-01-05"), layout(), input(7, 3, ClosedDay)),
            Err(CalendarClockModelError::CountersMismatch {
                expected_tick: 6,
                ..
            })
        ));
    }
    #[test]
    fn a16_invalid_layout_including_checked_sum_overflow_is_rejected() {
        for (total, auction, closing) in [(0, 0, 0), (2, 1, 1), (2, 2, 1), (u64::MAX, u64::MAX, 1)]
        {
            let l = SessionTickLayout {
                ticks_per_day: total,
                auction_ticks: auction,
                closing_auction_ticks: closing,
            };
            assert!(
                matches!(calendar_clock_projection(&model("2030-01-02"), l, input(0, 0, Trading { completed_ticks: 0 })),
                Err(CalendarClockModelError::InvalidTickLayout { ticks_per_day, auction_ticks, closing_auction_ticks })
                if ticks_per_day == total && auction_ticks == auction && closing_auction_ticks == closing)
            );
        }
    }
    #[test]
    fn a17_tick_arithmetic_overflow_is_explicit() {
        let l = SessionTickLayout {
            ticks_per_day: u64::MAX,
            auction_ticks: 0,
            closing_auction_ticks: 0,
        };
        assert!(matches!(
            calendar_clock_projection(&model("2030-01-03"), l, input(0, 2, TradingCompleted)),
            Err(CalendarClockModelError::ArithmeticOverflow)
        ));
        assert!(matches!(
            calendar_clock_projection(
                &model("2030-01-03"),
                l,
                input(0, 1, Trading { completed_ticks: 1 })
            ),
            Err(CalendarClockModelError::ArithmeticOverflow)
        ));
    }
    #[test]
    fn a18_dates_validate_both_runtime_bounds_and_order() {
        let c = calendar(true);
        assert!(matches!(
            calendar_day_model(&c, date("2030-01-03"), date("2030-01-02"), &BINDINGS),
            Err(CalendarClockModelError::CurrentBeforeStart { .. })
        ));
        for (start, current, bad) in [
            ("1999-01-01", "2030-01-02", "1999-01-01"),
            ("1999-01-01", "1999-01-01", "1999-01-01"),
            ("2030-01-02", "1999-01-01", "1999-01-01"),
            ("2030-01-02", "2100-01-01", "2100-01-01"),
        ] {
            assert!(
                matches!(calendar_day_model(&c, date(start), date(current), &BINDINGS),
                Err(CalendarClockModelError::Calendar(CalendarError::RuntimeStartOutOfRange { date: actual, .. })) if actual == date(bad))
            );
        }
    }
    #[test]
    fn a19_passed_policy_is_the_only_calendar_authority() {
        let s = date("2030-01-02");
        let mixed = calendar_day_model(&calendar(true), s, s, &BINDINGS).unwrap();
        let fallback = calendar_day_model(&calendar(false), s, s, &BINDINGS).unwrap();
        assert!(matches!(
            mixed.stock(SSE).unwrap().status(),
            DayStatus::Closed(_)
        ));
        assert_eq!(fallback.stock(SSE).unwrap().status(), &DayStatus::Trading);
    }
    #[test]
    fn a20_leap_weekend_is_excluded_from_half_open_union() {
        let d = calendar_day_model(
            &calendar(false),
            date("2032-02-27"),
            date("2032-03-02"),
            &BINDINGS,
        )
        .unwrap();
        assert_eq!(d.shared_sessions_before_date(), 2);
        assert!(d.stocks().values().all(|s| s.sessions_before_date() == 2));
        assert!(d.shared_trading_on_date());
    }
}
