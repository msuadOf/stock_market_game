//! WASM 仅支持 EveryTick；策略拒绝必须发生在会话及共享 ingress 登记之前。

use super::*;
use engine::RuntimeSyncPolicy;

#[path = "../../../packages/engine/tests/publications/session_fixture.rs"]
mod fixture;

fn setup() -> SessionSetup {
    let mut setup = fixture::civil_setup(engine::CivilDate::from_iso("2030-01-05").unwrap());
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.history_len = 1;
    setup
}

fn settled_save(seed: u64) -> engine::SaveSlot {
    let mut setup = setup();
    setup.runtime_sync_policy = RuntimeSyncPolicy::EveryTick;
    let mut session = ProtocolSession::new(setup, seed).unwrap();
    session.end_civil_day_update().unwrap();
    session.save().unwrap()
}

fn assert_unsupported(error: impl std::fmt::Display) {
    assert!(
        error.to_string().contains("WASM_RUNTIME_SYNC_POLICY_UNSUPPORTED"),
        "WASM 策略拒绝必须给出稳定错误码，实际：{error}"
    );
}

#[test]
fn create_rejects_unsupported_policies_before_registering_a_session() {
    for policy in [RuntimeSyncPolicy::DayEnd, RuntimeSyncPolicy::OnDemand] {
        let mut registry = SessionRegistry::default();
        let mut setup = setup();
        setup.runtime_sync_policy = policy;

        let result = registry.create(setup, 41);
        match result {
            Err(error) => assert_unsupported(error),
            Ok(handle) => panic!("策略 {policy:?} 应在登记前拒绝，错误地登记句柄 {handle}"),
        }
        assert!(registry.sessions.is_empty(), "拒绝后不得登记 handle");
    }
}

#[test]
fn create_rejects_unsupported_policy_before_engine_setup_validation() {
    for policy in [RuntimeSyncPolicy::DayEnd, RuntimeSyncPolicy::OnDemand] {
        let mut setup = setup();
        setup.runtime_sync_policy = policy;
        setup.stocks.clear();
        let mut registry = SessionRegistry::default();

        let result = registry.create(setup, 45);
        match result {
            Err(error) => assert_unsupported(error),
            Ok(handle) => panic!("策略 {policy:?} 应先于 setup 校验拒绝，登记了句柄 {handle}"),
        }
        assert!(registry.sessions.is_empty(), "拒绝后不得登记 handle");
    }
}

#[test]
fn restore_rejects_unsupported_policies_before_registering_a_session() {
    for policy in [RuntimeSyncPolicy::DayEnd, RuntimeSyncPolicy::OnDemand] {
        let mut slot = settled_save(42);
        slot.setup.runtime_sync_policy = policy;
        let mut registry = SessionRegistry::default();

        let result = registry.restore(&slot);
        match result {
            Err(error) => assert_unsupported(error),
            Ok(handle) => panic!("策略 {policy:?} 应在登记前拒绝，错误地登记句柄 {handle}"),
        }
        assert!(registry.sessions.is_empty(), "拒绝后不得登记 handle");
    }
}

#[test]
fn restore_rejects_unsupported_policy_before_engine_setup_validation() {
    for policy in [RuntimeSyncPolicy::DayEnd, RuntimeSyncPolicy::OnDemand] {
        let mut slot = settled_save(46);
        slot.setup.runtime_sync_policy = policy;
        slot.setup.ticks_per_day = 0;
        let mut registry = SessionRegistry::default();

        let result = registry.restore(&slot);
        match result {
            Err(error) => assert_unsupported(error),
            Ok(handle) => panic!("策略 {policy:?} 应先于 restore setup 校验拒绝，登记了句柄 {handle}"),
        }
        assert!(registry.sessions.is_empty(), "拒绝后不得登记 handle");
    }
}

#[test]
fn every_tick_create_and_restore_remain_supported() {
    let mut setup = setup();
    setup.runtime_sync_policy = RuntimeSyncPolicy::EveryTick;
    let mut registry = SessionRegistry::default();
    let created = registry.create(setup.clone(), 43).unwrap();
    assert!(registry.sessions.contains_key(&created));

    let slot = settled_save(44);
    let restored = registry.restore(&slot).unwrap();
    assert!(registry.sessions.contains_key(&restored));
    assert_ne!(created, restored);
}
