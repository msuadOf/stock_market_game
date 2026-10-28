//! 使用真实 Calendar Arc 的纯 carrier/consumer fixture；不冒充 Session 翻译发布。

use super::*;
use crate::calendar::{
    embedded_lunar_facts, CalendarExchange, CalendarPolicy, CalendarPolicySpec,
    OfficialCoverageEntry, SimulatedFallbackRuleset, TradingCalendar,
};
use crate::session::exchange_calendar_model::{
    SessionTickLayout, SharedClockInput, SharedDayProgress,
};
use crate::session::market_calendar_context::MarketCalendarSource;

fn date(text: &str) -> crate::CivilDate {
    crate::CivilDate::from_iso(text).unwrap()
}

fn source(start: &str) -> Arc<MarketCalendarSource> {
    // synthetic-test 只验证交易所计数差异，不预测真实 2030 交易所公告。
    let calendar = TradingCalendar::from_policy(
        CalendarPolicy::from_parts(CalendarPolicySpec {
            algorithm_version: 1,
            default_start: date("2030-01-01"),
            runtime_min_start: date("2000-01-01"),
            runtime_max_end: date("2099-12-31"),
            init_only_min_start: date("1998-01-01"),
            official_coverage: vec![OfficialCoverageEntry::new(
                CalendarExchange::Sse,
                2030,
                vec![(date("2030-01-01"), date("2030-01-02"))],
                "synthetic-test".into(),
                "synthetic-test".into(),
            )],
            simulated_fallback: SimulatedFallbackRuleset::new(
                1,
                2026,
                embedded_lunar_facts().unwrap(),
            )
            .unwrap(),
        })
        .unwrap(),
    )
    .unwrap();
    MarketCalendarSource::new(
        calendar,
        date(start),
        vec![
            (StockId(0), CalendarExchange::Sse),
            (StockId(1), CalendarExchange::Szse),
        ],
    )
    .unwrap()
}

fn context(
    source: &Arc<MarketCalendarSource>,
    date_text: &str,
    tick: u64,
    sessions: u64,
    completed_ticks: u64,
) -> Arc<crate::runtime::MarketTickContext> {
    source
        .day(date(date_text))
        .unwrap()
        .tick(
            SessionTickLayout {
                ticks_per_day: 4,
                auction_ticks: 0,
                closing_auction_ticks: 0,
            },
            SharedClockInput {
                tick,
                completed_shared_sessions: sessions,
                progress: SharedDayProgress::Trading { completed_ticks },
            },
        )
        .unwrap()
}

#[test]
fn d3_calendar_shared_root_and_first_reopened_stock_keep_distinct_domains() {
    let source = source("2030-01-02");
    let closed = context(&source, "2030-01-02", 0, 0, 0);
    assert!(causal_tick_time(&closed, Some(StockId(0)), false).is_err());
    let reopened = context(&source, "2030-01-03", 4, 1, 0);
    assert!(Arc::ptr_eq(closed.day().source(), reopened.day().source()));
    let root = causal_tick_time(&reopened, None, false).unwrap();
    let stock = causal_tick_time(&reopened, Some(StockId(0)), false).unwrap();
    assert_eq!((root.market_minute, root.stock_clock), (240, None));
    assert_eq!(stock.market_minute, 240);
    assert_eq!(stock.stock_clock.unwrap().completed_minutes, 0);
    let facts = vec![
        CausalFact {
            sequence: 0,
            time: root,
            kind: CausalFactKind::Decision {
                account: AccountId(0),
            },
        },
        CausalFact {
            sequence: 1,
            time: stock,
            kind: CausalFactKind::Quote(Quote {
                code: StockId(0),
                bid_cents: None,
                ask_cents: None,
                bid_depth: 0,
                ask_depth: 0,
            }),
        },
    ];
    CausalReport::from_facts(7, &facts).unwrap();
}

#[test]
fn d3_calendar_recovery_counts_only_own_minutes_across_other_exchange_open_day() {
    // 元旦两所共同休市；上一真实会话选 2029-12-31。
    let source = source("2029-12-31");
    let before = context(&source, "2029-12-31", 0, 0, 0);
    let loss = context(&source, "2029-12-31", 3, 0, 3);
    let closed = context(&source, "2030-01-02", 4, 1, 0);
    assert!(
        causal_tick_time(&closed, Some(StockId(0)), false).is_err(),
        "停市期间不制造 Quote"
    );
    let recovered = context(&source, "2030-01-03", 8, 2, 0);
    let facts: Vec<_> = [(&before, 100), (&loss, 0), (&recovered, 100)]
        .into_iter()
        .enumerate()
        .map(|(sequence, (context, depth))| CausalFact {
            sequence: sequence as u64,
            time: causal_tick_time(context, Some(StockId(0)), false).unwrap(),
            kind: CausalFactKind::Quote(Quote {
                code: StockId(0),
                bid_cents: Some(990),
                ask_cents: Some(1_010),
                bid_depth: depth,
                ask_depth: depth,
            }),
        })
        .collect();
    assert_eq!(
        facts[2].time.market_minute - facts[1].time.market_minute,
        300
    );
    let report = CausalReport::from_facts(7, &facts).unwrap();
    assert_eq!(report.recoveries.len(), 1);
    let recovery = &report.recoveries[0];
    assert_eq!(
        (
            recovery.loss_sequence,
            recovery.recovered_sequence,
            recovery.market_minutes
        ),
        (1, Some(2), Some(60))
    );
}
