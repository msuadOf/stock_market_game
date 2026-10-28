//! 稀疏版本跳过只免观察，不免真实来源校验。
use super::*;

#[test]
fn same_library_version_validates_calendar_before_returning_without_observation() {
    let mut setup = crate::runtime::bridge_tests::setup_fixture();
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 1;
    setup.strategy_params.hot.order_size = setup.config.lot_size;
    let mut session = GameSession::new(setup, 42).unwrap();
    let mut driver = RuntimeDriver::from_session_grouped(&mut session, 2).unwrap();
    assert_eq!(
        driver.deliver_information_for_test(&mut session).unwrap(),
        0
    );
    let session_before = serde_json::to_vec(&session.save().unwrap()).unwrap();
    let source = driver.mirror_source();
    assert_eq!(driver.deliver_information(&mut session).unwrap(), 0);
    assert_eq!(driver.mirror_source(), source);
    assert_eq!(
        serde_json::to_vec(&session.save().unwrap()).unwrap(),
        session_before
    );
    driver
        .world
        .set_civil_date(session.civil_date().next().unwrap());
    let bad_source = driver.mirror_source();
    let error = driver
        .deliver_information(&mut session)
        .expect_err("相同库版本不能吞掉错误日期来源");
    assert!(error.contains("civil date"), "{error}");
    assert_eq!(driver.mirror_source(), bad_source);
    assert_eq!(
        serde_json::to_vec(&session.save().unwrap()).unwrap(),
        session_before
    );
}

#[test]
fn delivery_rejects_world_behind_session_without_observing_or_changing_source() {
    let mut setup = crate::runtime::bridge_tests::setup_fixture();
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 1;
    setup.strategy_params.hot.order_size = setup.config.lot_size;
    let mut session = GameSession::new(setup, 42).unwrap();
    let mut driver = RuntimeDriver::from_session_grouped(&mut session, 2).unwrap();
    session.set_runtime_tick(1);
    let before = serde_json::to_vec(&session.snapshot()).unwrap();
    let source = driver.mirror_source();
    let error = driver
        .deliver_information_for_test(&mut session)
        .expect_err("World 落后必须显式拒绝");
    assert!(error.contains("落后 Session"), "{error}");
    assert_eq!(serde_json::to_vec(&session.snapshot()).unwrap(), before);
    assert_eq!(driver.mirror_source(), source);
}
