//! 共同基线三种镜像输入的行为边界；不把静点或手动日终伪造成新市场拍。

use super::drive::{PreparedDriverCalendar, RuntimeDriver};
use crate::calendar::{CalendarExchange, CivilDate, OfficialCoverageEntry};
use crate::{GameSession, StockCode, StockExchange, StockId};
use std::sync::Arc;

fn mixed_game(all_closed: bool) -> GameSession {
    let mut setup = super::bridge_tests::setup_fixture();
    setup.start_date = CivilDate::from_iso("2030-01-02").unwrap();
    setup.ticks_per_day = 2;
    setup.auction_ticks = 0;
    setup.closing_auction_ticks = 0;
    setup.npcs.retail_count = 0;
    setup.stocks[0].float_shares = 0;
    setup.stocks[1] = setup.stocks[0].clone();
    setup.stocks[1].code = StockCode("600101".into());
    setup.stocks[1].exchange = StockExchange::Shanghai;
    let initial = GameSession::new(setup, 73).unwrap();
    let mut save = initial.save().unwrap();
    let exchanges = if all_closed {
        vec![CalendarExchange::Sse, CalendarExchange::Szse]
    } else {
        vec![CalendarExchange::Sse]
    };
    // 2030 双所差异仅为 synthetic-test；不改变已有初始化前史或预测官方安排。
    for exchange in exchanges {
        save.civil_clock
            .policy
            .official_coverage
            .push(OfficialCoverageEntry::new(
                exchange,
                2030,
                vec![(
                    CivilDate::from_iso("2030-01-01").unwrap(),
                    initial.civil_date(),
                )],
                "synthetic-test-c2c-calendar-projection".into(),
                "synthetic-test-c2c-calendar-projection-source".into(),
            ));
    }
    GameSession::restore(&save).expect("合法首次冻结 synthetic Calendar")
}

#[test]
fn calendar_projection_rejects_mode_owner_and_tick_before_mirror_side_effects() {
    for wrong_day in [false, true] {
        let mut invalid = mixed_game(true);
        if wrong_day {
            invalid.advance_runtime_market_day();
        } else {
            invalid.set_runtime_tick(1);
        }
        let before = serde_json::to_value(invalid.snapshot()).unwrap();
        let error = RuntimeDriver::from_session(&mut invalid)
            .err()
            .expect("closed 静点错 tick/day 必须在构造前拒绝");
        assert!(
            error.contains("tick") || error.contains("共享"),
            "须报告 Calendar 计数错配：{error}"
        );
        assert_eq!(serde_json::to_value(invalid.snapshot()).unwrap(), before);
    }
    let mut game = mixed_game(false);
    let foreign = mixed_game(false);
    let mut driver = RuntimeDriver::from_session(&mut game).unwrap();
    let input = driver.prepare_static_driver_projection(&game).unwrap();
    let day = Arc::clone(input.calendar.day());
    let before = serde_json::to_value(game.snapshot()).unwrap();
    let source = driver.mirror_source();
    let published = driver.publication().load();
    let seq = driver.next_seq();
    let pending = driver.pending_mirror_domains();
    let mut candidates = Vec::new();
    let mut malformed = input.clone();
    malformed.emit_price_tick = true;
    candidates.push((malformed, 0, "Calendar 模式"));
    let mut malformed = input.clone();
    malformed.calendar =
        PreparedDriverCalendar::MarketTick(game.prepare_market_calendar_tick().unwrap().1);
    candidates.push((malformed, 0, "Calendar 模式"));
    let mut malformed = input.clone();
    malformed.calendar = PreparedDriverCalendar::ManualDayEnd(Arc::clone(&day));
    malformed.static_read = None;
    candidates.push((malformed, 0, "Calendar 模式"));
    let mut malformed = input.clone();
    malformed.calendar =
        PreparedDriverCalendar::Static(foreign.prepare_market_calendar_day().unwrap());
    candidates.push((malformed, 0, "source owner"));
    candidates.push((input, 1, "非市场 Calendar 不得推进镜像 tick"));
    for (candidate, tick, expected) in candidates {
        let error = game
            .prepare_runtime_mirror(&mut driver, candidate, tick)
            .err()
            .expect("非法模式或时间来源须显式拒绝");
        assert!(
            error.contains(expected),
            "应拒绝目标原因：{expected}，实际：{error}"
        );
        assert_eq!(serde_json::to_value(game.snapshot()).unwrap(), before);
        assert_eq!(driver.mirror_source(), source);
        assert_eq!(driver.pending_mirror_domains(), pending);
        assert_eq!(driver.next_seq(), seq);
        assert!(Arc::ptr_eq(&published, &driver.publication().load()));
    }
}

#[test]
fn all_closed_static_full_keeps_day_context_without_constructing_market_tick() {
    let mut game = mixed_game(true);
    assert!(
        game.prepare_market_calendar_tick().is_err(),
        "全休市根本不能构造市场 Tick"
    );
    let mut driver = RuntimeDriver::from_session(&mut game).unwrap();
    let mut before = serde_json::to_value(game.snapshot()).unwrap();
    let prior_origin = game.runtime_mirror_origin();
    assert_eq!(prior_origin.install_sequence, 0);
    let prior_source = driver.mirror_source();
    let prior_seq = driver.next_seq();
    let published = driver.publication().load();
    let input = driver.prepare_static_driver_projection(&game).unwrap();
    assert!(matches!(&input.calendar, PreparedDriverCalendar::Static(_)));
    assert!(!input.calendar.stock_is_open(StockId(0)).unwrap());
    assert!(!input.calendar.stock_is_open(StockId(1)).unwrap());
    assert_eq!(input.calendar.market_phase_before(), None);
    let prepared = game.prepare_runtime_mirror(&mut driver, input, 0).unwrap();
    let ack = game
        .validate_runtime_mirror_source(&driver, &prepared)
        .unwrap();
    assert!(game
        .install_runtime_mirror(&mut driver, prepared)
        .is_empty());
    driver.ack_prepared_mirror(ack);
    assert_eq!((game.tick(), game.day()), (0, 0));
    let installed_origin = game.runtime_mirror_origin();
    assert_eq!(installed_origin.install_sequence, 1, "成功 Full 恰好安装一次");
    let mut expected_origin = prior_origin;
    expected_origin.install_sequence = 1;
    assert_eq!(installed_origin, expected_origin, "静点 Full 不改变其它 source 身份或进度");
    before["mirror_origin"] = serde_json::to_value(expected_origin).unwrap();
    assert_eq!(serde_json::to_value(game.snapshot()).unwrap(), before);
    assert_eq!(driver.mirror_source(), prior_source);
    assert_eq!(driver.next_seq(), prior_seq, "静点安装不额外生成市场事件或执行 flush");
    assert!(driver.world_report_log().is_empty());
    assert!(Arc::ptr_eq(&published, &driver.publication().load()));
}

#[test]
fn manual_day_end_uses_ended_day_mask_without_preparing_another_tick() {
    let mut game = mixed_game(false);
    let mut driver = RuntimeDriver::from_session(&mut game).unwrap();
    let before = game.snapshot();
    // direct driver 提交真实 progress；市场日结仍由下方显式入口执行。
    for tick in 1..=2 {
        driver.step(&mut game).expect("真实 mixed 市场拍");
        assert_eq!(game.tick(), tick);
    }
    assert!(
        game.prepare_market_calendar_tick().is_err(),
        "本日已无可新造的市场拍"
    );
    let day = game.prepare_market_calendar_day().unwrap();
    let manual = PreparedDriverCalendar::ManualDayEnd(day);
    assert!(manual.stock_is_open(StockId(0)).unwrap());
    assert!(!manual.stock_is_open(StockId(1)).unwrap());
    assert_eq!(manual.market_phase_before(), None);
    let starts = driver.world_report_log().len();
    driver
        .day_end_with_runtime_market_day(&mut game)
        .expect("真实末拍之后手动日终");
    let after = game.snapshot();
    assert_eq!((game.tick(), game.day()), (2, 1));
    assert_eq!(
        driver.world_report_log().len(),
        starts,
        "手动日终不能再跑 Decision 或 Start"
    );
    assert_eq!(
        after.daily_candles[&StockId(0)].len(),
        before.daily_candles[&StockId(0)].len() + 1
    );
    assert_eq!(
        after.daily_candles[&StockId(1)],
        before.daily_candles[&StockId(1)]
    );
    assert!(!after.active_daily_candles.contains_key(&StockId(1)));
}
