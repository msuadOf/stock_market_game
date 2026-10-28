//! 外持 driver 的活跃、退役与 Session 身份边界；不以历史 epoch 非零冒充活 owner。
use super::*;
use crate::runtime::drive::RuntimeDriver;

fn external_game() -> GameSession {
    let mut setup = super::super::npc_working_quote_tests::quote_setup(0);
    setup.start_date = crate::CivilDate::from_iso("2030-01-05").unwrap();
    setup.ticks_per_day = 2;
    GameSession::new(setup, 42).unwrap()
}

fn open_external_game() -> GameSession {
    let mut setup = super::super::npc_working_quote_tests::quote_setup(0);
    setup.start_date = crate::CivilDate::from_iso("2030-01-04").unwrap();
    setup.ticks_per_day = 4;
    setup.auction_ticks = 0;
    setup.closing_auction_ticks = 0;
    GameSession::new(setup, 42).unwrap()
}

fn assert_owner_error<T>(result: Result<T, String>, marker: &str) {
    let error = match result {
        Ok(_) => panic!("合法业务输入必须在 owner 身份守卫处拒绝"),
        Err(error) => error,
    };
    assert!(
        error.contains(marker),
        "必须保留准确 owner 拒绝来源：{error}"
    );
}

fn assert_projection_writes_reject(
    driver: &mut RuntimeDriver,
    game: &mut GameSession,
    marker: &str,
) {
    assert_owner_error(driver.sync_seq_into_session(game), marker);
    game.require_healthy().unwrap();
    assert_owner_error(driver.install_market_views(game), marker);
    game.require_healthy().unwrap();
    assert_owner_error(driver.install_candles_with_mode(game, false), marker);
    game.require_healthy().unwrap();
    let mut events = Vec::new();
    assert_owner_error(driver.emit_price_tick_events(game, &mut events), marker);
    assert!(events.is_empty());
    game.require_healthy().unwrap();
    let error = driver.step_frame_equivalent(game).unwrap_err();
    assert!(error.to_string().contains(marker));
    game.require_healthy().unwrap();
    let error = game.runtime_step_with_install(driver).unwrap_err();
    assert!(error.to_string().contains(marker));
    game.require_healthy().unwrap();
}

#[test]
fn external_driver_drop_allows_next_civil_to_adopt_internal_authority() {
    let mut game = external_game();
    let driver = RuntimeDriver::from_session(&mut game).unwrap();
    let epoch = game.runtime_authority_epoch;
    assert!(game.runtime_driver.is_none());
    drop(driver);
    game.end_civil_day().unwrap();
    assert!(
        game.runtime_driver.is_some(),
        "当前外 owner 退役后必须可正常惰性接管"
    );
    assert_eq!(game.runtime_authority_epoch, epoch + 1);
}

#[test]
fn replaced_old_driver_cannot_write_or_keep_a_retired_current_owner_alive() {
    let mut game = open_external_game();
    let mut old = RuntimeDriver::from_session(&mut game).unwrap();
    game.runtime_step_with_install(&mut old)
        .expect("先提交真实合法 Open tick，不能靠 Closed/缺 tick 遮挡所有权错误");
    assert_eq!((game.tick(), old.world_committed_tick()), (1, 1));
    let mut current = RuntimeDriver::from_session(&mut game).unwrap();
    let epoch = game.runtime_authority_epoch;
    let before = game.business_state_hash().unwrap();
    let old_tick = old.world_committed_tick();
    let seq = game.seq();
    assert_owner_error(old.step(&mut game), "authority_epoch");
    game.require_healthy().unwrap();
    assert_owner_error(old.day_end(&mut game), "authority_epoch");
    game.require_healthy().unwrap();
    assert_owner_error(old.deliver_information(&mut game), "authority_epoch");
    game.require_healthy().unwrap();
    assert_projection_writes_reject(&mut old, &mut game, "authority_epoch");
    assert_eq!(old.world_committed_tick(), old_tick);
    assert_eq!(game.business_state_hash().unwrap(), before);
    assert_eq!(game.seq(), seq);
    for _ in 0..3 {
        game.runtime_step_with_install(&mut current).unwrap();
    }
    assert_eq!((game.tick(), game.day()), (4, 1));
    drop(current);
    game.end_civil_day().unwrap();
    assert!(
        game.runtime_driver.is_some(),
        "旧 A 活着不得延长已退役当前 B 的 lease"
    );
    assert_eq!(game.runtime_authority_epoch, epoch + 1);
    assert_owner_error(old.prepare_driver_step(&game), "authority_epoch");
    game.require_healthy().unwrap();
}

#[test]
fn factory_epoch_failure_preserves_the_existing_external_owner() {
    let mut game = external_game();
    let driver = RuntimeDriver::from_session(&mut game).unwrap();
    let epoch = game.runtime_authority_epoch;
    let before = game.business_state_hash().unwrap();
    game.runtime_authority_epoch = u64::MAX;
    assert!(RuntimeDriver::from_session(&mut game).is_err());
    assert_eq!(game.runtime_authority_epoch, u64::MAX);
    assert_eq!(game.business_state_hash().unwrap(), before);
    // 恢复仅用于独立验证失败构造没有换掉原 owner 的 lease；不复用耗尽身份继续业务。
    game.runtime_authority_epoch = epoch;
    game.end_civil_day().unwrap();
    assert!(
        game.runtime_driver.is_none(),
        "失败 factory 不得撤销原外 owner"
    );
    assert_eq!(game.runtime_authority_epoch, epoch);
    drop(driver);
}

#[test]
fn active_internal_driver_rejects_external_factory_before_any_mutation() {
    let mut game = closed_runtime_information_session();
    let before = game.business_state_hash().unwrap();
    let revision = game.external_revision();
    let epoch = game.runtime_authority_epoch;
    let publication = game
        .runtime_driver
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .publication()
        .load();
    assert!(
        RuntimeDriver::from_session(&mut game).is_err(),
        "活跃内部 owner 不得被公开 factory 抢权"
    );
    assert_eq!(game.business_state_hash().unwrap(), before);
    assert_eq!(game.external_revision(), revision);
    assert_eq!(game.runtime_authority_epoch, epoch);
    let after = game
        .runtime_driver
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .publication()
        .load();
    assert!(std::sync::Arc::ptr_eq(&publication, &after));
}

#[test]
fn external_owner_survives_callback_abort_and_shadow_does_not_keep_it_alive() {
    let mut game = external_game();
    let driver = RuntimeDriver::from_session(&mut game).unwrap();
    let epoch = game.runtime_authority_epoch;
    let before = game.business_state_hash().unwrap();
    let shadow = game.clone_for_tick_shadow().unwrap();
    let fault = ValidationFault::new(1034, vec![74, 12]);
    let mut reached = false;
    let result: Result<(), SessionError> = game.with_prepared_civil_day(|_, _| {
        reached = true;
        Err(SessionError::InvalidSave(fault.clone()))
    });
    assert!(reached);
    assert!(matches!(result, Err(SessionError::InvalidSave(actual)) if actual == fault));
    assert_eq!(game.business_state_hash().unwrap(), before);
    assert_eq!(game.runtime_authority_epoch, epoch);
    assert!(game.runtime_driver.is_none());
    drop(driver);
    game.end_civil_day().unwrap();
    assert!(
        game.runtime_driver.is_some(),
        "shadow 只可持 Weak，不得延长旧 World 寿命"
    );
    assert_eq!(game.runtime_authority_epoch, epoch + 1);
    drop(shadow);
}

#[test]
fn restore_has_new_session_identity_and_same_epoch_driver_cannot_write_it() {
    let mut game = open_external_game();
    let mut driver = RuntimeDriver::from_session(&mut game).unwrap();
    game.runtime_step_with_install(&mut driver)
        .expect("先提交真实 Open tick，Drain 必须具有实际 Calendar 来源");
    assert_eq!((game.tick(), driver.world_committed_tick()), (1, 1));
    let mut restored = GameSession::restore(&game.save().unwrap()).unwrap();
    assert!(
        !restored.has_current_external_runtime_authority(),
        "restore 不继承内存 owner"
    );
    let mut restored_driver = RuntimeDriver::from_session(&mut restored).unwrap();
    assert_eq!(
        game.runtime_authority_epoch,
        restored.runtime_authority_epoch
    );
    let before = game.business_state_hash().unwrap();
    let restored_before = restored.business_state_hash().unwrap();
    let seq = (game.seq(), restored.seq());
    let ticks = (
        driver.world_committed_tick(),
        restored_driver.world_committed_tick(),
    );
    assert_owner_error(driver.step(&mut restored), "Session 身份");
    restored.require_healthy().unwrap();
    assert_owner_error(driver.prepare_driver_step(&restored), "Session 身份");
    restored.require_healthy().unwrap();
    assert_owner_error(driver.day_end(&mut restored), "Session 身份");
    restored.require_healthy().unwrap();
    assert_owner_error(driver.deliver_information(&mut restored), "Session 身份");
    restored.require_healthy().unwrap();
    assert_projection_writes_reject(&mut driver, &mut restored, "Session 身份");
    game.require_healthy().unwrap();
    assert_eq!(game.business_state_hash().unwrap(), before);
    assert_eq!(restored.business_state_hash().unwrap(), restored_before);
    assert_eq!((game.seq(), restored.seq()), seq);
    assert_eq!(
        (
            driver.world_committed_tick(),
            restored_driver.world_committed_tick()
        ),
        ticks
    );
    for _ in 0..3 {
        restored
            .runtime_step_with_install(&mut restored_driver)
            .unwrap();
    }
    assert_eq!((restored.tick(), restored.day()), (4, 1));
    drop(restored_driver);
    restored.end_civil_day().unwrap();
    assert!(
        restored.runtime_driver.is_some(),
        "restore 不得继承原 Session 的外 lease"
    );
}
