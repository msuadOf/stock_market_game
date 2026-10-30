//! V12：真实 mixed 日结后严格 SaveSlot restore 与下一复市日续行。
//! direct-engine GameSession 公共API；合成休市不代表官方未来安排。

use crate::runtime::calendar_public_restore_fixture as fixture;

use crate::calendar::CalendarExchange;
use crate::session::{MarketHistoryAvailability, MarketHistoryRequest};
use crate::{
    GameSession, RuntimeMirrorDayEndStage, RuntimeMirrorOrigin, Snapshot, StockExchange, StockId,
    TradingPhase,
};
use fixture::{date, restore, START_DATE, TICKS_PER_DAY};

fn assert_snapshot_with_expected_origins(
    original: Snapshot,
    restored: Snapshot,
    original_origin: RuntimeMirrorOrigin,
    restored_origin: RuntimeMirrorOrigin,
) {
    assert_eq!(
        original.mirror_origin, original_origin,
        "原会话来源必须精确匹配"
    );
    assert_eq!(
        restored.mirror_origin, restored_origin,
        "恢复会话来源必须精确匹配"
    );
    // origin 属于各自会话；保留完整 Snapshot，未来新增业务字段仍须逐字段相等。
    let mut expected = original.clone();
    expected.mirror_origin = restored_origin;
    assert_eq!(
        serde_json::to_value(restored).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
}

fn retained(session: &GameSession, stock: StockId, day: &str) -> crate::session::MarketHistoryPage {
    session
        .market_history_page(&MarketHistoryRequest {
            stock,
            date_from: date(day),
            date_to: date(day),
            after: None,
            page_size: 1,
        })
        .expect("严格恢复后的 retained 查询必须成功")
}

fn strict_restore_and_continue(closed_exchange: CalendarExchange, first: StockExchange) {
    let mut fixture = restore(closed_exchange, first, false);
    for _ in 0..TICKS_PER_DAY {
        fixture.session.step().expect("mixed 首日真实市场拍");
    }
    let boundary = fixture
        .session
        .end_civil_day()
        .expect("mixed 首日真实自然日日结");
    assert_eq!(boundary.settled_date, date(START_DATE));
    assert_eq!(fixture.session.civil_date(), date("2030-01-03"));
    assert_eq!((fixture.session.tick(), fixture.session.day()), (2, 1));
    let save = fixture.session.save().expect("真正日终产生完整 SaveSlot");
    // 先经真实 JSON 往返，再走严格公共 restore；不修改 Calendar、日龄或任何派生字段。
    let wire = serde_json::to_string(&save).unwrap();
    let transported: crate::SaveSlot = serde_json::from_str(&wire).unwrap();
    let mut restored =
        GameSession::restore(&transported).expect("合法 mixed 日终 SaveSlot 必须可严格恢复");
    assert_eq!(
        serde_json::to_value(restored.save().unwrap()).unwrap(),
        serde_json::to_value(&save).unwrap(),
        "完整事实往返保持，不剔除不匹配字段"
    );
    assert_snapshot_with_expected_origins(
        fixture.session.snapshot(),
        restored.snapshot(),
        RuntimeMirrorOrigin {
            authority_epoch: 1,
            install_sequence: TICKS_PER_DAY + 1,
            source_tick: TICKS_PER_DAY,
            source_generation: TICKS_PER_DAY + 1,
            day_end_stage: RuntimeMirrorDayEndStage::RetainedPublished,
            non_tick_revision: 0,
            mirror_phase: TradingPhase::Continuous,
        },
        RuntimeMirrorOrigin {
            authority_epoch: 0,
            install_sequence: 0,
            source_tick: TICKS_PER_DAY,
            source_generation: 0,
            day_end_stage: RuntimeMirrorDayEndStage::Tick,
            non_tick_revision: 0,
            mirror_phase: TradingPhase::Continuous,
        },
    );
    let closed = retained(&restored, fixture.closed, START_DATE);
    assert_eq!(closed.entries.len(), 1);
    assert_eq!(
        closed.entries[0].availability,
        MarketHistoryAvailability::Closed
    );
    assert!(closed.entries[0].bars.is_empty());
    assert!(closed.entries[0].daily_candle.is_none());
    let open = retained(&restored, fixture.open, START_DATE);
    assert_eq!(open.entries.len(), 1);
    assert_eq!(
        open.entries[0].availability,
        MarketHistoryAvailability::NoTrades
    );
    assert!(open.entries[0].bars.is_empty());
    assert!(open.entries[0].daily_candle.is_some());

    for tick in 3..=4 {
        let expected = fixture.session.step().expect("原会话复市日真实续行");
        let actual = restored.step().expect("严格恢复后复市日真实续行");
        assert_eq!(restored.tick(), tick);
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(expected).unwrap(),
            "零 NPC 固定输入的公共事实逐字段相同"
        );
        let continued_ticks = tick - TICKS_PER_DAY;
        let day_end_stage = if continued_ticks == TICKS_PER_DAY {
            RuntimeMirrorDayEndStage::RetainedPublished
        } else {
            RuntimeMirrorDayEndStage::Tick
        };
        assert_snapshot_with_expected_origins(
            fixture.session.snapshot(),
            restored.snapshot(),
            RuntimeMirrorOrigin {
                authority_epoch: 1,
                // 首次静点接管与首日 civil INFO 各占一次安装，市场拍各占一次。
                install_sequence: tick + 2,
                source_tick: tick,
                source_generation: tick + tick / TICKS_PER_DAY,
                day_end_stage,
                non_tick_revision: 1,
                mirror_phase: TradingPhase::Continuous,
            },
            RuntimeMirrorOrigin {
                authority_epoch: 1,
                install_sequence: continued_ticks + 1,
                source_tick: tick,
                source_generation: continued_ticks + continued_ticks / TICKS_PER_DAY,
                day_end_stage,
                non_tick_revision: 0,
                mirror_phase: TradingPhase::Continuous,
            },
        );
        if tick == 3 {
            let reopened = restored.runtime_minute_history(&fixture.closed);
            let continuing = restored.runtime_minute_history(&fixture.open);
            assert_eq!(
                reopened.first().unwrap().absolute_trading_minute,
                0,
                "严格恢复后首次复市仍从本股会话零起步"
            );
            assert_eq!(
                continuing.first().unwrap().absolute_trading_minute,
                240,
                "另一股不能被首次复市股的时钟倒放"
            );
        }
    }
    fixture
        .session
        .end_civil_day()
        .expect("原会话复市日自然日日结");
    restored.end_civil_day().expect("恢复会话复市日自然日日结");
    assert_eq!(
        serde_json::to_value(restored.save().unwrap()).unwrap(),
        serde_json::to_value(fixture.session.save().unwrap()).unwrap()
    );
    for stock in [fixture.closed, fixture.open] {
        let history = retained(&restored, stock, "2030-01-03");
        assert_eq!(history.entries.len(), 1);
        assert_eq!(
            history.entries[0].availability,
            MarketHistoryAvailability::NoTrades
        );
        assert!(history.entries[0].bars.is_empty());
        assert!(history.entries[0].daily_candle.is_some());
    }
}

#[test]
fn sse_mixed_day_end_strict_save_restore_continues_first_reopen() {
    strict_restore_and_continue(CalendarExchange::Sse, StockExchange::Shanghai);
}

#[test]
fn szse_mixed_day_end_strict_save_restore_continues_first_reopen() {
    strict_restore_and_continue(CalendarExchange::Szse, StockExchange::Shenzhen);
}

fn rejects_edited_mixed_history(closed_exchange: CalendarExchange, first: StockExchange) {
    let mut fixture = restore(closed_exchange, first, false);
    for _ in 0..TICKS_PER_DAY {
        fixture.session.step().unwrap();
    }
    fixture.session.end_civil_day().unwrap();
    let save = fixture.session.save().unwrap();
    GameSession::restore(&save).expect("负例前的真实 mixed 日终存档必须合法");
    let closed = fixture.closed;
    let open = fixture.open;
    assert_eq!(save.price_history[&closed].len(), 0);
    assert_eq!(save.price_history[&open].len(), 2);
    for (stock, add, expected) in [(closed, true, 0), (open, false, 2)] {
        let mut edited = save.clone();
        let prices = edited.price_history.get_mut(&stock).unwrap();
        if add {
            prices.push(crate::Money::from_cents(1_000));
        } else {
            prices.pop().unwrap();
        }
        let actual = prices.len();
        assert!(
            matches!(GameSession::restore(&edited),
            Err(crate::SessionError::InvalidSave(fault)) if fault.code == 2010
                && fault.context == vec![u64::from(stock.0), actual as u64, expected]),
            "闭市股多造 history 与开市股缺 history 均须精确拒绝"
        );
    }
    for stock in [closed, open] {
        assert!(save.market_minute_closes[&stock].is_empty());
        let mut edited = save.clone();
        edited.market_minute_closes.get_mut(&stock).unwrap().push(
            crate::observation::MarketMinuteClose {
                absolute_trading_minute: 0,
                close: crate::Money::from_cents(1_000),
            },
        );
        assert!(
            matches!(GameSession::restore(&edited),
            Err(crate::SessionError::InvalidSave(fault)) if fault.code == 2017
                && fault.context == vec![u64::from(stock.0)]),
            "日终分钟桶已清，任何股均不能恢复额外当前日分钟"
        );
    }
    let mut extra_candle = save.clone();
    let real_open_candle = save.snapshot.daily_candles[&open].last().unwrap().clone();
    extra_candle
        .snapshot
        .daily_candles
        .get_mut(&closed)
        .unwrap()
        .push(real_open_candle);
    assert!(
        matches!(GameSession::restore(&extra_candle),
        Err(crate::SessionError::InvalidSave(fault)) if fault.code == 2064
            && fault.context == vec![u64::from(closed.0), 361, 360]),
        "闭市股不得补一根零成交日 K 冒充真实会话"
    );
    let mut wrong_date = save.clone();
    let candle = wrong_date
        .snapshot
        .daily_candles
        .get_mut(&open)
        .unwrap()
        .last_mut()
        .unwrap();
    let correct = candle.time;
    candle.time += 86_400;
    assert!(
        matches!(GameSession::restore(&wrong_date),
        Err(crate::SessionError::InvalidSave(fault)) if fault.code == 2066
            && fault.context == vec![360, u64::from(open.0), (correct + 86_400).unsigned_abs(), correct.unsigned_abs()]),
        "真实开市 K 的错误日期必须在长度相同的条件下被拒绝"
    );
}

#[test]
fn sse_mixed_restore_rejects_edited_stock_history_minute_and_candle() {
    rejects_edited_mixed_history(CalendarExchange::Sse, StockExchange::Shanghai);
}

#[test]
fn szse_mixed_restore_rejects_edited_stock_history_minute_and_candle() {
    rejects_edited_mixed_history(CalendarExchange::Szse, StockExchange::Shenzhen);
}

#[test]
fn mixed_restore_across_weekend_keeps_stock_history_cap_and_real_candle_dates() {
    let mut fixture = restore(CalendarExchange::Sse, StockExchange::Shanghai, false);
    for day in ["2030-01-02", "2030-01-03", "2030-01-04"] {
        assert_eq!(fixture.session.civil_date(), date(day));
        for _ in 0..TICKS_PER_DAY {
            fixture.session.step().unwrap();
        }
        fixture.session.end_civil_day().unwrap();
    }
    assert_eq!(fixture.session.civil_date(), date("2030-01-05"));
    let friday_save = fixture.session.save().unwrap();
    assert_eq!(friday_save.price_history[&fixture.closed].len(), 4);
    assert_eq!(
        friday_save.price_history[&fixture.open].len(),
        5,
        "本股 history_len 截断保持"
    );
    let mut restored = GameSession::restore(&friday_save).expect("周末起点严格恢复");
    for day in ["2030-01-05", "2030-01-06"] {
        assert_eq!(restored.civil_date(), date(day));
        let before = restored.save().unwrap();
        restored.end_civil_day().unwrap();
        fixture.session.end_civil_day().unwrap();
        let after = restored.save().unwrap();
        assert_eq!(after.snapshot.tick, before.snapshot.tick);
        assert_eq!(after.price_history, before.price_history);
        assert_eq!(
            after.snapshot.daily_candles, before.snapshot.daily_candles,
            "全闭日不新增本股日 K 或价格样本"
        );
        assert!(after.market_minute_closes.values().all(Vec::is_empty));
        assert_eq!(
            serde_json::to_value(&after).unwrap(),
            serde_json::to_value(fixture.session.save().unwrap()).unwrap()
        );
        restored = GameSession::restore(&after).expect("每个全闭日日结后继续严格恢复");
    }
    assert_eq!(restored.civil_date(), date("2030-01-07"));
    for _ in 0..TICKS_PER_DAY {
        let expected = fixture.session.step().unwrap();
        let actual = restored.step().unwrap();
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
    }
    fixture.session.end_civil_day().unwrap();
    restored.end_civil_day().unwrap();
    let save = restored.save().unwrap();
    assert_eq!(save.snapshot.daily_candles[&fixture.closed].len(), 363);
    assert_eq!(save.snapshot.daily_candles[&fixture.open].len(), 364);
    for stock in [fixture.closed, fixture.open] {
        assert_eq!(save.price_history[&stock].len(), 5);
        let page = retained(&restored, stock, "2030-01-07");
        assert_eq!(page.entries.len(), 1);
        assert_eq!(
            page.entries[0].availability,
            MarketHistoryAvailability::NoTrades
        );
        assert!(page.entries[0].daily_candle.is_some());
    }
    assert_eq!(
        serde_json::to_value(&save).unwrap(),
        serde_json::to_value(fixture.session.save().unwrap()).unwrap()
    );
    GameSession::restore(&save).expect("周末后真实复市日终仍可严格恢复");
}

#[test]
fn mixed_restore_completed_day_and_advanced_day_keep_empty_minutes_and_reject_clock_drift() {
    let mut fixture = restore(CalendarExchange::Sse, StockExchange::Shanghai, false);
    for _ in 0..TICKS_PER_DAY {
        fixture.session.step().unwrap();
    }
    let completed = fixture.session.save().unwrap();
    assert_eq!(completed.civil_clock.current_date, date(START_DATE));
    assert!(completed.market_minute_closes.values().all(Vec::is_empty));
    let completed_restore =
        GameSession::restore(&completed).expect("同 civil date 已完成交易日的真实存档必须合法");
    assert_eq!(
        serde_json::to_value(completed_restore.save().unwrap()).unwrap(),
        serde_json::to_value(&completed).unwrap()
    );
    fixture.session.end_civil_day().unwrap();
    let advanced = fixture.session.save().unwrap();
    assert_eq!(advanced.snapshot.tick, completed.snapshot.tick);
    assert_eq!(advanced.civil_clock.current_date, date("2030-01-03"));
    assert!(advanced.market_minute_closes.values().all(Vec::is_empty));
    GameSession::restore(&advanced).expect("相同 tick 的次日零进度存档必须合法");
    let mut clock_drift = advanced.clone();
    clock_drift.snapshot.tick += 2 * TICKS_PER_DAY;
    let clock_result = GameSession::restore(&clock_drift).map(|_| ());
    assert!(matches!(&clock_result,
        Err(crate::SessionError::CivilClock(crate::session::CivilClockError::SaveInconsistent { .. }))),
        "tick 与冻结 Calendar 的已完成共享会话不符必须拒绝，不能伪装成 history 长度错误：{clock_result:?}");
    let mut policy_drift = advanced;
    policy_drift.civil_clock.policy.simulated_fallback.digest = "invalid-c2c-digest".into();
    assert!(
        matches!(
            GameSession::restore(&policy_drift),
            Err(crate::SessionError::CivilClock(
                crate::session::CivilClockError::Calendar(
                    crate::calendar::CalendarError::DigestMismatch {
                        field: "simulated_fallback",
                        ..
                    }
                )
            ))
        ),
        "坏 policy digest 必须保留 CivilClock/Calendar 的精确错误域"
    );
}

#[test]
fn mixed_restore_rejects_missing_and_wrong_stock_minute_after_real_reopen_tick() {
    let mut fixture = restore(CalendarExchange::Sse, StockExchange::Shanghai, false);
    for _ in 0..TICKS_PER_DAY {
        fixture.session.step().unwrap();
    }
    fixture.session.end_civil_day().unwrap();
    fixture.session.step().unwrap();
    let save = fixture.session.save().unwrap();
    GameSession::restore(&save).expect("两股均开市的真实非零分钟基线必须合法");
    assert_eq!(save.market_minute_closes[&fixture.closed].len(), 120);
    assert_eq!(save.market_minute_closes[&fixture.open].len(), 120);
    assert_eq!(
        save.market_minute_closes[&fixture.closed][0].absolute_trading_minute,
        0
    );
    assert_eq!(
        save.market_minute_closes[&fixture.open][0].absolute_trading_minute,
        240
    );
    let mut missing = save.clone();
    missing
        .market_minute_closes
        .get_mut(&fixture.closed)
        .unwrap()
        .pop()
        .unwrap();
    assert!(
        matches!(GameSession::restore(&missing),
        Err(crate::SessionError::InvalidSave(fault)) if fault.code == 2018
            && fault.context == vec![u64::from(fixture.closed.0)]),
        "本股真实已完成的分钟缺键必须拒绝"
    );
    let mut wrong = save;
    wrong.market_minute_closes.get_mut(&fixture.open).unwrap()[0].absolute_trading_minute = 0;
    assert!(
        matches!(GameSession::restore(&wrong),
        Err(crate::SessionError::InvalidSave(fault)) if fault.code == 2017
            && fault.context == vec![u64::from(fixture.open.0)]),
        "其它股的合法绝对分钟也不能替换本股分钟键"
    );
}

#[test]
fn mixed_initial_restore_at_closed_runtime_ceiling_does_not_query_a_future_session() {
    // 仅初始存档冻结 synthetic ceiling；Jan1 休市覆盖保留，不改前史事实。
    let mut save = fixture::initial_save(CalendarExchange::Sse, StockExchange::Shanghai, false);
    save.civil_clock.policy.runtime_max_end = date(START_DATE);
    assert_eq!(save.snapshot.tick, 0);
    assert!(save.price_history.values().all(Vec::is_empty));
    assert!(save
        .snapshot
        .daily_candles
        .values()
        .all(|candles| candles.len() == 360));
    let transported: crate::SaveSlot =
        serde_json::from_str(&serde_json::to_string(&save).unwrap()).unwrap();
    let restored = GameSession::restore(&transported)
        .expect("闭市股零已完成会话的合法初始档不能被未来日期查询拒绝");
    assert_eq!(
        serde_json::to_value(restored.save().unwrap()).unwrap(),
        serde_json::to_value(&save).unwrap()
    );
    let day = restored.prepare_market_calendar_day().unwrap();
    assert!(matches!(
        day.day_model().stock(StockId(1)).unwrap().status(),
        crate::calendar::DayStatus::Closed(_)
    ));
    assert_eq!(
        day.day_model()
            .stock(StockId(1))
            .unwrap()
            .sessions_before_date(),
        0
    );
}
