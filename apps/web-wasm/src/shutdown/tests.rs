//! 真实 Registry 双 session 范围与 ingress 撤销；worker 回收由 engine 短 fixture 验证。
#[path = "../../../../packages/engine/tests/publications/session_fixture.rs"]
mod fixture;

use super::*;
use crate::{ingress_operation, AccountId, Intent};

fn registry() -> (SessionRegistry, u32, u32) {
    let mut registry = SessionRegistry::default();
    let mut setup = fixture::civil_setup(engine::CivilDate::from_iso("2030-01-02").unwrap());
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.history_len = 1;
    let first = registry.create(setup.clone(), 71).unwrap();
    let second = registry.create(setup, 72).unwrap();
    (registry, first, second)
}

fn cancel() -> Intent {
    Intent::Cancel {
        code: engine::StockId(0),
        id: engine::OrderId(1),
    }
}

#[test]
fn shutdown_removes_only_selected_owner_and_revokes_its_existing_ingress() {
    let (mut registry, first, second) = registry();
    let first_ingress = registry
        .with_session(first, |session| session.shared_ingress())
        .unwrap();
    let second_ingress = registry
        .with_session(second, |session| session.shared_ingress())
        .unwrap();
    let report = registry.shutdown(first).expect("指定 owner 应显式停止");
    assert!(report.runtime.is_none(), "关闭不能暗中构造不存在的 driver");
    assert_eq!(registry.sessions.len(), 1);
    assert!(registry.with_session(first, |_| ()).is_err());
    assert!(registry.step_update(first).is_err());
    assert!(first_ingress
        .enqueue_player_intent(AccountId(0), cancel())
        .is_err());
    assert!(!ingress_operation(|services| Ok(services.contains_key(&first))).unwrap());
    second_ingress
        .enqueue_player_intent(AccountId(0), cancel())
        .expect("其他 session 不能被一起关闭");
    assert_eq!(
        registry
            .with_session(second, |session| session.tick())
            .unwrap(),
        0
    );
    registry.shutdown(second).expect("清理另一个真实 owner");
}

#[test]
fn shutdown_unknown_or_consumed_handle_is_explicit_failure_without_second_join() {
    let (mut registry, first, second) = registry();
    let unknown = registry
        .shutdown(u32::MAX)
        .expect_err("未知 handle 不得返回成功 ack");
    assert_eq!(unknown.primary.stage, "registry_handle");
    assert!(unknown.primary.detail.contains(&u32::MAX.to_string()));
    assert!(unknown.completion.is_none());
    assert_eq!(registry.sessions.len(), 2);
    registry.shutdown(first).expect("首次关闭成功");
    let repeated = registry
        .shutdown(first)
        .expect_err("已消费 owner 不能再次 join 或伪造成功");
    assert_eq!(repeated.primary.stage, "registry_handle");
    assert!(repeated.completion.is_none());
    assert_eq!(registry.sessions.len(), 1);
    registry.shutdown(second).expect("清理剩余 owner");
}

#[test]
fn shutdown_ingress_revoke_failure_still_removes_owner_and_closes_existing_capability() {
    let (mut registry, first, second) = registry();
    let capability = registry
        .with_session(first, |session| session.shared_ingress())
        .unwrap();
    // 只 poison 本 case 私有锁；这是撤销 Err 负例，不冒充全局 INGRESS 锁故障实证。
    let local = std::sync::Mutex::new(());
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = local.lock().expect("局部锁首次加锁正常");
        panic!("shutdown-local-revoke-poison-fixture");
    }))
    .is_err());
    let mut invoked = false;
    let failure = registry
        .shutdown_with_ingress_revoke(first, |handle| {
            assert_eq!(handle, first);
            invoked = true;
            let _guard = local
                .lock()
                .map_err(|_| SessionError::ResourceLimit(engine::ValidationFault::bare(1300)))?;
            Ok(())
        })
        .expect_err("撤销失败不能给成功 ack 或提前放弃 owner 清理");
    assert!(invoked, "必须实际尝试本 handle 的撤销操作");
    assert_eq!(failure.primary.stage, "registry_ingress_revoke");
    assert!(failure.primary.detail.contains("1300"));
    assert!(
        failure.completion.is_none(),
        "无 World driver 不得制造 join 证明"
    );
    assert!(
        registry.with_session(first, |_| ()).is_err(),
        "撤销 Err 后也要摘除 owner"
    );
    assert!(
        capability
            .enqueue_player_intent(AccountId(0), cancel())
            .is_err(),
        "仍需关闭 owner 已持 capability"
    );
    assert_eq!(registry.sessions.len(), 1);
    assert!(registry.with_session(second, |_| ()).is_ok());
    // 负例刻意没有触碰实际 INGRESS map；清理它遗留的 token，避免污染并发套件。
    ingress_operation(|services| {
        services.remove(&first);
        Ok(())
    })
    .unwrap();
    registry.shutdown(second).expect("清理剩余真实 owner");
}
