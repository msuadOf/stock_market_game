//! 会话要求 runtime；三档策略由共享 engine 实现。

use super::*;
use engine::RuntimeSyncPolicy;

use crate::session_fixture as fixture;

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

#[test]
fn runtime_create_and_restore_support_all_policies() {
    for policy in [
        RuntimeSyncPolicy::EveryTick,
        RuntimeSyncPolicy::DayEnd,
        RuntimeSyncPolicy::OnDemand,
    ] {
        let mut registry = SessionRegistry::default();
        let mut config = setup();
        config.runtime_sync_policy = policy;
        let handle = registry.create(config, 41).expect("runtime 应支持三档策略");
        assert!(registry.sessions.contains_key(&handle));
        let mut slot = settled_save(42);
        slot.setup.runtime_sync_policy = policy;
        let restored = registry.restore(&slot).expect("runtime 应保留存档策略");
        assert!(registry.sessions.contains_key(&restored));
    }
}

#[test]
fn session_requires_runtime_independently_of_policy() {
    assert!(require_wasm_runtime_enabled(true).is_ok());
    assert!(require_wasm_runtime_enabled(false)
        .unwrap_err()
        .to_string()
        .contains("WASM_RUNTIME_NOT_READY"));
}

#[test]
fn runtime_invalid_setup_and_restore_are_rejected_before_registration() {
    for policy in [RuntimeSyncPolicy::DayEnd, RuntimeSyncPolicy::OnDemand] {
        let mut registry = SessionRegistry::default();
        let mut config = setup();
        config.runtime_sync_policy = policy;
        config.stocks.clear();
        assert!(registry.create(config, 45).is_err());
        assert!(registry.sessions.is_empty());

        let mut slot = settled_save(46);
        slot.setup.runtime_sync_policy = policy;
        slot.setup.ticks_per_day = 0;
        assert!(registry.restore(&slot).is_err());
        assert!(registry.sessions.is_empty());
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
