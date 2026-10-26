//! C2-C 实际市场行为回归；初始真实 restore 后只观察内存事实，不要求 C4 mixed 恢复。

#[path = "common/mixed_calendar_c2c_fixture.rs"]
mod fixture;

use engine::calendar::{CalendarExchange, CalendarPolicy, TradingCalendar};
use engine::session::{
    CivilPhase, CurrentMinuteHistoryPhase, CurrentMinuteHistoryRequest, MarketHistoryAvailability,
    MarketHistoryRequest,
};
use engine::{Event, StockExchange};
use fixture::{date, restore, PRESET_HISTORY_DAYS, START_DATE, TICKS_PER_DAY};

#[test]
fn initial_mixed_fixture_restores_with_frozen_policy_and_catalog() {
    for closed in [CalendarExchange::Sse, CalendarExchange::Szse] {
        for first in [StockExchange::Shanghai, StockExchange::Shenzhen] {
            let save = fixture::initial_save(closed, first, false);
            let calendar = TradingCalendar::from_policy(
                CalendarPolicy::from_parts(save.civil_clock.policy.clone())
                    .expect("synthetic policy 必须合法"),
            )
            .expect("冻结 Calendar 必须合法");
            assert!(!calendar.is_trading_day(closed, date(START_DATE)).unwrap());
            assert!(!calendar.is_trading_day(closed, date("2030-01-01")).unwrap());
            let session = engine::GameSession::restore(&save)
                .unwrap_or_else(|error| panic!("初始 restore 正控失败：{error:?}"));
            assert_eq!((session.tick(), session.day()), (0, 0));
            assert_eq!(session.civil_date(), date(START_DATE));
            assert_eq!(session.civil_clock().phase(), CivilPhase::IntradayTrading);
            assert_eq!(
                session.save().unwrap().civil_clock.policy,
                save.civil_clock.policy
            );
            assert_eq!(session.stock_id("000101"), Some(engine::StockId(0)));
            assert_eq!(session.stock_id("600101"), Some(engine::StockId(1)));
            for candles in session.snapshot().daily_candles.values() {
                assert_eq!(candles.len(), PRESET_HISTORY_DAYS);
            }
        }
    }
}

#[test]
fn closed_first_tick_emits_no_price_fact_while_open_stock_progresses() {
    let mut fixture = restore(CalendarExchange::Sse, StockExchange::Shanghai, false);
    let events = fixture.session.step().expect("合法 mixed 首拍");
    assert!(
        events.iter().any(|event| matches!(event,
            Event::PriceTick { code, .. } if *code == fixture.open
        )),
        "open 零成交股仍须有真实本拍行情"
    );
    assert!(
        !events.iter().any(|event| matches!(event,
            Event::PriceTick { code, .. } if *code == fixture.closed
        )),
        "closed 股不能产生 PriceTick"
    );
    assert_eq!(fixture.session.tick(), 1, "closed 股仍须参加共享协议拍");
}

#[test]
fn closed_first_tick_preserves_valuation_without_active_daily_candle() {
    let mut fixture = restore(CalendarExchange::Sse, StockExchange::Shenzhen, false);
    let before = fixture.session.snapshot();
    fixture.session.step().expect("合法 mixed 首拍");
    let after = fixture.session.snapshot();
    assert_eq!(
        after.markets[&fixture.closed], before.markets[&fixture.closed],
        "closed 股上一真实估值与盘口保持不变"
    );
    assert!(after.active_daily_candles.contains_key(&fixture.open));
    assert!(
        !after.active_daily_candles.contains_key(&fixture.closed),
        "closed 股不能补零量 active K"
    );
}

#[test]
fn closed_first_tick_adds_neither_price_history_nor_market_minute() {
    let mut fixture = restore(CalendarExchange::Szse, StockExchange::Shenzhen, false);
    let before_prices = fixture.session.runtime_price_history_len(&fixture.closed);
    let before_minutes = fixture
        .session
        .runtime_minute_history(&fixture.closed)
        .to_vec();
    fixture.session.step().expect("合法 mixed 首拍");
    assert!(
        !fixture
            .session
            .runtime_minute_history(&fixture.open)
            .is_empty(),
        "open 股压缩首拍应完成真实连续交易分钟"
    );
    assert_eq!(
        fixture.session.runtime_price_history_len(&fixture.closed),
        before_prices,
        "closed 股不能追加价格样本"
    );
    assert_eq!(
        fixture.session.runtime_minute_history(&fixture.closed),
        before_minutes,
        "closed 股不能追加共享市场分钟"
    );
}

#[test]
fn mixed_day_end_archives_only_open_stock_and_reports_only_its_final_candle() {
    let mut fixture = restore(CalendarExchange::Sse, StockExchange::Shanghai, false);
    let mut events = Vec::new();
    for _ in 0..TICKS_PER_DAY {
        events.extend(fixture.session.step().expect("合法 mixed 交易拍"));
    }
    let snapshot = fixture.session.snapshot();
    assert_eq!(fixture.session.day(), 1, "共享市场日只推进一次");
    assert_eq!(
        snapshot.daily_candles[&fixture.open].len(),
        PRESET_HISTORY_DAYS + 1
    );
    assert_eq!(
        snapshot.daily_candles[&fixture.closed].len(),
        PRESET_HISTORY_DAYS,
        "closed 股不归档虚构日 K"
    );
    let boundary = events
        .iter()
        .find_map(|event| match event {
            Event::DayBoundary {
                closed_daily_candles,
                ..
            } => Some(closed_daily_candles),
            _ => None,
        })
        .expect("共享市场日必须发布 DayBoundary");
    assert!(boundary.contains_key(&fixture.open));
    assert!(
        !boundary.contains_key(&fixture.closed),
        "DayBoundary 只交付真实 open 终 K"
    );
}

#[test]
fn closed_stock_produces_no_auction_facts_in_open_or_closing_window() {
    let mut fixture = restore(CalendarExchange::Sse, StockExchange::Shanghai, true);
    let mut events = Vec::new();
    for _ in 0..12 {
        events.extend(fixture.session.step().expect("合法 mixed 竞价交易拍"));
    }
    let auctions = |stock| {
        events
            .iter()
            .filter(move |event| {
                matches!(event,
                    Event::AuctionCompleted { code, .. } if *code == stock
                )
            })
            .count()
    };
    assert_eq!(auctions(fixture.open), 2, "open 股开盘与收盘清算各一次");
    assert_eq!(auctions(fixture.closed), 0, "closed 股不得执行集合竞价清算");
    assert!(
        !events.iter().any(|event| matches!(event,
            Event::AuctionTick { code, .. } if *code == fixture.closed
        )),
        "closed 股不得生成指示价事实"
    );
}

#[test]
fn closed_current_minute_query_stays_closed_during_shared_market_tick() {
    let mut fixture = restore(CalendarExchange::Sse, StockExchange::Shanghai, false);
    fixture.session.step().expect("合法 mixed 首拍");
    let closed = fixture
        .session
        .current_minute_history(&CurrentMinuteHistoryRequest {
            stock: fixture.closed,
        })
        .expect("健康 Session 的 closed 分钟查询");
    assert_eq!(closed.date, date(START_DATE));
    assert!(!closed.trading);
    assert_eq!(closed.phase, CurrentMinuteHistoryPhase::Closed);
    assert!(closed.bars.is_empty());
    let open = fixture
        .session
        .current_minute_history(&CurrentMinuteHistoryRequest {
            stock: fixture.open,
        })
        .expect("健康 Session 的 open 分钟查询");
    assert!(open.trading);
    assert_eq!(open.phase, CurrentMinuteHistoryPhase::Continuous);
    assert!(open.bars.is_empty(), "open 零成交不补造成交分钟 K");
}

#[test]
fn retained_closed_and_open_no_trades_remain_distinct_after_civil_commit() {
    let mut fixture = restore(CalendarExchange::Sse, StockExchange::Shanghai, false);
    for _ in 0..TICKS_PER_DAY {
        fixture.session.step().expect("合法 mixed 交易拍");
    }
    let report = fixture.session.end_civil_day().expect("真实自然日日结");
    assert_eq!(report.settled_date, date(START_DATE));
    let request = |stock| MarketHistoryRequest {
        stock,
        date_from: date(START_DATE),
        date_to: date(START_DATE),
        after: None,
        page_size: 1,
    };
    let closed = fixture
        .session
        .market_history_page(&request(fixture.closed))
        .expect("closed retained 查询");
    assert_eq!(closed.entries.len(), 1);
    assert_eq!(
        closed.entries[0].availability,
        MarketHistoryAvailability::Closed
    );
    assert!(closed.entries[0].bars.is_empty());
    assert!(closed.entries[0].daily_candle.is_none());
    let open = fixture
        .session
        .market_history_page(&request(fixture.open))
        .expect("open retained 查询");
    assert_eq!(open.entries.len(), 1);
    assert_eq!(
        open.entries[0].availability,
        MarketHistoryAvailability::NoTrades
    );
    assert!(open.entries[0].bars.is_empty());
    assert!(
        open.entries[0].daily_candle.is_some(),
        "open 零成交仍有真实终 K"
    );
}

#[test]
fn first_reopen_uses_stock_minute_zero_while_other_exchange_starts_day_one() {
    let mut fixture = restore(CalendarExchange::Sse, StockExchange::Shanghai, false);
    for _ in 0..TICKS_PER_DAY {
        fixture.session.step().expect("首日真实交易拍");
    }
    fixture.session.end_civil_day().expect("首日真实自然日日结");
    assert_eq!(fixture.session.civil_date(), date("2030-01-03"));
    assert_eq!((fixture.session.tick(), fixture.session.day()), (2, 1));
    fixture.session.step().expect("原 closed 股首次复市");
    let reopened = fixture.session.runtime_minute_history(&fixture.closed);
    assert!(!reopened.is_empty(), "首次复市首拍应完成本股分钟");
    assert_eq!(
        reopened[0].absolute_trading_minute, 0,
        "本股尚无已完成开市日，分钟首键从 0 开始"
    );
    let continued = fixture.session.runtime_minute_history(&fixture.open);
    assert!(!continued.is_empty());
    assert_eq!(
        continued[0].absolute_trading_minute, 240,
        "另一所已经完成一会话，其本股分钟首键为 240"
    );
    for _ in 1..TICKS_PER_DAY {
        fixture.session.step().expect("复市日剩余真实交易拍");
    }
    let snapshot = fixture.session.snapshot();
    assert_eq!(
        snapshot.daily_candles[&fixture.closed].len(),
        PRESET_HISTORY_DAYS + 1
    );
    assert_eq!(
        snapshot.daily_candles[&fixture.open].len(),
        PRESET_HISTORY_DAYS + 2
    );
}
