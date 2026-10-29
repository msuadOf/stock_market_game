//! 运行上界是只读前置拒绝；真实市场收盘不应使该拒绝终结 Session。

use super::*;
use crate::runtime::drive::RuntimeDriver;
use std::sync::Arc;

const TICKS_PER_DAY: u64 = 4;
const FINAL_DATE: &str = "2099-12-31";

fn date(iso: &str) -> crate::CivilDate {
    crate::CivilDate::from_iso(iso).unwrap()
}

fn boundary_game(start: &str) -> GameSession {
    let mut setup = super::npc_working_quote_tests::quote_setup(0);
    setup.start_date = date(start);
    setup.ticks_per_day = TICKS_PER_DAY;
    GameSession::new(setup, 42).unwrap()
}

fn unexpected_disclosure(_: crate::calendar::CivilInstant) {
    panic!("被拒绝的运行上界推进不得调用 disclosure observer");
}

fn assert_ceiling_rejection_preserves_state(game: &mut GameSession) {
    game.civil_clock_mut()
        .add_disclosure_observer(unexpected_disclosure);
    let before = game.save().expect("拒绝前应可读取完整状态投影");
    assert_eq!(
        before.company_state.legacy_for_test().0.next_due_date(),
        Some(date(FINAL_DATE)),
        "必须存在末日尚未派发的公司 due"
    );

    let error = game.end_civil_day().unwrap_err();
    assert!(
        matches!(&error, SessionError::CivilClock(CivilClockError::BeyondRuntimeCeiling {
            next,
            ceiling,
        }) if *next == date("2100-01-01") && *ceiling == date(FINAL_DATE)),
        "必须保留精确的运行上界错误：{error:?}"
    );
    assert!(game.poison_reason().is_none());
    game.require_healthy().unwrap();
    let after = game.save().expect("上界拒绝后仍应可读取状态投影");
    assert_eq!(
        serde_json::to_vec(&before).unwrap(),
        serde_json::to_vec(&after).unwrap(),
        "完整 SaveSlot（账户、RNG、公司 due 等）必须逐字节保持"
    );
    assert_eq!(game.civil_date(), date(FINAL_DATE));
    assert_eq!(game.civil_clock().settled_through(), None);
    assert_eq!(
        after.company_state.legacy_for_test().0.next_due_date(),
        Some(date(FINAL_DATE))
    );
}

#[test]
fn internal_full_close_ceiling_rejection_preserves_state_and_owner() {
    let mut game = boundary_game(FINAL_DATE);
    for _ in 0..TICKS_PER_DAY {
        game.step_via_runtime().unwrap();
    }
    assert_eq!((game.tick(), game.day()), (TICKS_PER_DAY, 1));
    let epoch = game.runtime_authority_epoch;
    let publication_slot = {
        let driver = game.runtime_driver.as_ref().unwrap().lock().unwrap();
        assert!(driver
            .has_committed_market_close_for_civil(game.tick(), game.civil_date())
            .unwrap());
        driver.publication()
    };
    let publication = publication_slot.load();

    assert_ceiling_rejection_preserves_state(&mut game);

    assert_eq!(game.runtime_authority_epoch, epoch);
    let driver = game.runtime_driver.as_ref().unwrap().lock().unwrap();
    assert_eq!(driver.world_committed_tick(), TICKS_PER_DAY);
    assert!(Arc::ptr_eq(&publication_slot, &driver.publication()));
    assert!(Arc::ptr_eq(&publication, &driver.publication().load()));
}

#[test]
fn external_full_close_ceiling_rejection_preserves_state_and_owner() {
    let mut game = boundary_game(FINAL_DATE);
    let mut driver = RuntimeDriver::from_session(&mut game).unwrap();
    for _ in 0..TICKS_PER_DAY {
        game.runtime_step_with_install(&mut driver).unwrap();
    }
    assert_eq!((game.tick(), game.day()), (TICKS_PER_DAY, 1));
    assert!(driver
        .has_committed_market_close_for_civil(game.tick(), game.civil_date())
        .unwrap());
    assert!(game.has_current_external_runtime_authority());
    let epoch = game.runtime_authority_epoch;
    let publication = driver.publication().load();

    assert_ceiling_rejection_preserves_state(&mut game);

    assert!(game.runtime_driver.is_none());
    assert!(game.has_current_external_runtime_authority());
    assert_eq!(game.runtime_authority_epoch, epoch);
    assert_eq!(driver.world_committed_tick(), TICKS_PER_DAY);
    assert!(Arc::ptr_eq(&publication, &driver.publication().load()));
}

#[test]
fn penultimate_trading_day_can_settle_into_runtime_ceiling() {
    let mut game = boundary_game("2099-12-30");
    for _ in 0..TICKS_PER_DAY {
        game.step_via_runtime().unwrap();
    }
    let epoch = game.runtime_authority_epoch;

    let report = game.end_civil_day().unwrap();

    assert_eq!(report.settled_date, date("2099-12-30"));
    assert_eq!(report.next_date, date(FINAL_DATE));
    assert_eq!(game.civil_date(), date(FINAL_DATE));
    assert_eq!(
        game.civil_clock().settled_through(),
        Some(date("2099-12-30"))
    );
    assert_eq!(game.runtime_authority_epoch, epoch);
    game.require_healthy().unwrap();
    let save = game.save().unwrap();
    assert_eq!(
        save.company_state.legacy_for_test().0.next_due_date(),
        Some(date(FINAL_DATE))
    );
}

#[test]
fn incomplete_final_market_session_rejects_before_runtime_ceiling() {
    let mut game = boundary_game(FINAL_DATE);
    for _ in 0..TICKS_PER_DAY - 1 {
        game.step_via_runtime().unwrap();
    }
    let before = serde_json::to_vec(&game.save().unwrap()).unwrap();
    let epoch = game.runtime_authority_epoch;
    let publication = game
        .runtime_driver
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .publication()
        .load();

    let error = game.end_civil_day().unwrap_err();

    assert!(
        matches!(&error, SessionError::CivilClock(CivilClockError::MarketSessionOutOfSync {
            date: rejected,
            completed_sessions: 0,
            expected_sessions: 1,
        }) if *rejected == date(FINAL_DATE)),
        "末日未收盘须保留原市场会话错误优先级：{error:?}"
    );
    assert!(game.poison_reason().is_none());
    assert_eq!(serde_json::to_vec(&game.save().unwrap()).unwrap(), before);
    assert_eq!(game.runtime_authority_epoch, epoch);
    let driver = game.runtime_driver.as_ref().unwrap().lock().unwrap();
    assert_eq!(driver.world_committed_tick(), TICKS_PER_DAY - 1);
    assert!(Arc::ptr_eq(&publication, &driver.publication().load()));
}
