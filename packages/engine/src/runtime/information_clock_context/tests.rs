//! 真实 Calendar source 的正负门；不把合成 Admin phase 当生产 dispatcher 资格。
use super::*;
use crate::session::exchange_calendar_model::{calendar_clock_projection, SharedDayProgress};

fn context() -> (
    Arc<MarketCalendarSource>,
    InformationClockContext,
    Vec<(StockId, CalendarExchange)>,
) {
    let date = CivilDate::from_iso("2030-01-02").unwrap();
    let bindings = vec![
        (StockId(0), CalendarExchange::Sse),
        (StockId(1), CalendarExchange::Szse),
    ];
    let source = MarketCalendarSource::new(
        crate::TradingCalendar::current_default_calendar().unwrap(),
        date,
        bindings.clone(),
    )
    .unwrap();
    let day = source.day(date).unwrap();
    let layout = SessionTickLayout {
        ticks_per_day: 2,
        auction_ticks: 0,
        closing_auction_ticks: 0,
    };
    let input = SharedClockInput {
        tick: 0,
        completed_shared_sessions: 0,
        progress: SharedDayProgress::Trading { completed_ticks: 0 },
    };
    let projection = calendar_clock_projection(day.day_model(), layout, input).unwrap();
    let clock = InformationClockContext::from_static_projection(
        day,
        layout,
        input,
        projection,
        CivilInstant::from_hms(date, 9, 30, 0).unwrap(),
    );
    (source, clock, bindings)
}

#[test]
fn information_context_rejects_wrong_owner_missing_extra_and_same_position_variant() {
    let (owner, clock, bindings) = context();
    let (_, foreign, _) = context();
    let anchors = clock.anchors().unwrap();
    assert_eq!(
        anchors[&StockId(0)],
        InformationTradingAnchor::TradingDay { ordinal: 0 }
    );
    clock
        .validate(ExpectedInformationClock {
            source: &owner,
            bindings: &bindings,
            date: clock.date(),
            layout: clock.layout(),
            input: clock.input(),
            through: clock.cutoff(),
            anchors: &anchors,
        })
        .unwrap();
    assert!(foreign
        .validate(ExpectedInformationClock {
            source: &owner,
            bindings: &bindings,
            date: clock.date(),
            layout: clock.layout(),
            input: clock.input(),
            through: clock.cutoff(),
            anchors: &anchors,
        })
        .is_err());
    for malformed in [
        {
            let mut map = anchors.clone();
            map.remove(&StockId(1));
            map
        },
        {
            let mut map = anchors.clone();
            map.insert(StockId(2), InformationTradingAnchor::NoPriorSession);
            map
        },
        {
            let mut map = anchors.clone();
            map.insert(
                StockId(0),
                InformationTradingAnchor::DayBefore { ordinal: 0 },
            );
            map
        },
    ] {
        assert!(clock
            .validate(ExpectedInformationClock {
                source: &owner,
                bindings: &bindings,
                date: clock.date(),
                layout: clock.layout(),
                input: clock.input(),
                through: clock.cutoff(),
                anchors: &malformed,
            })
            .is_err());
    }
}

#[test]
fn information_context_rejects_later_through_and_receivers_different_real_progress() {
    let (owner, clock, bindings) = context();
    let anchors = clock.anchors().unwrap();
    let late = CivilInstant::from_hms(clock.date(), 18, 0, 0).unwrap();
    assert!(clock
        .validate(ExpectedInformationClock {
            source: &owner,
            bindings: &bindings,
            date: clock.date(),
            layout: clock.layout(),
            input: clock.input(),
            through: late,
            anchors: &anchors,
        })
        .is_err());
    let next_input = SharedClockInput {
        tick: 1,
        completed_shared_sessions: 0,
        progress: SharedDayProgress::Trading { completed_ticks: 1 },
    };
    assert!(clock
        .validate(ExpectedInformationClock {
            source: &owner,
            bindings: &bindings,
            date: clock.date(),
            layout: clock.layout(),
            input: next_input,
            through: clock.cutoff(),
            anchors: &anchors,
        })
        .is_err());
}

#[test]
fn real_mixed_calendar_projection_keeps_different_stock_ordinals() {
    use crate::calendar::{CalendarPolicy, OfficialCoverageEntry, TradingCalendar};
    let start = CivilDate::from_iso("2030-01-02").unwrap();
    let current = start.next().unwrap();
    let mut saved = CalendarPolicy::current_default_policy().unwrap().spec();
    // 未来双所差异是明确 synthetic-test frozen policy，不宣称官方已公布2030日历。
    saved.official_coverage.push(OfficialCoverageEntry::new(
        CalendarExchange::Sse,
        2030,
        vec![(start, start)],
        "synthetic-test-info-mixed-calendar".into(),
        "synthetic-test-info-mixed-calendar-source".into(),
    ));
    let calendar =
        TradingCalendar::from_policy(CalendarPolicy::from_parts(saved).unwrap()).unwrap();
    let bindings = vec![
        (StockId(0), CalendarExchange::Sse),
        (StockId(1), CalendarExchange::Szse),
    ];
    let source = MarketCalendarSource::new(calendar, start, bindings.clone()).unwrap();
    let day = source.day(current).unwrap();
    let layout = SessionTickLayout {
        ticks_per_day: 2,
        auction_ticks: 0,
        closing_auction_ticks: 0,
    };
    let input = SharedClockInput {
        tick: 2,
        completed_shared_sessions: 1,
        progress: SharedDayProgress::Trading { completed_ticks: 0 },
    };
    let projection = calendar_clock_projection(day.day_model(), layout, input).unwrap();
    let clock = InformationClockContext::from_static_projection(
        day,
        layout,
        input,
        projection,
        CivilInstant::from_hms(current, 9, 30, 0).unwrap(),
    );
    let anchors = clock.anchors().unwrap();
    assert_eq!(
        anchors[&StockId(0)],
        InformationTradingAnchor::TradingDay { ordinal: 0 }
    );
    assert_eq!(
        anchors[&StockId(1)],
        InformationTradingAnchor::TradingDay { ordinal: 1 }
    );
    clock
        .validate(ExpectedInformationClock {
            source: &source,
            bindings: &bindings,
            date: current,
            layout,
            input,
            through: clock.cutoff(),
            anchors: &anchors,
        })
        .unwrap();
}
