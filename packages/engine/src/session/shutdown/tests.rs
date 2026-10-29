//! 不经过 tick 长跑的真实 session/driver owner 回收候选。
use super::*;
use crate::{AccountId, Intent, OrderId, StockId};

fn game() -> GameSession {
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
    setup.npcs.inst_count = 0;
    setup.history_len = 1;
    GameSession::new(setup, 71).expect("最短真实 session 合法")
}

fn cancel() -> Intent {
    Intent::Cancel { code: StockId(0), id: OrderId(1) }
}

#[test]
fn shutdown_without_driver_closes_existing_ingress_without_creating_workers() {
    let mut game = game();
    let ingress = game.shared_ingress();
    assert!(game.runtime_driver.is_none());
    let report = game.shutdown().expect("无 driver session 仍须显式关闭 ingress");
    assert!(report.runtime.is_none());
    assert!(ingress.enqueue_player_intent(AccountId(0), cancel()).is_err());
}

#[test]
fn shutdown_consumes_existing_real_driver_and_returns_its_joins_and_finals() {
    let mut game = game();
    game.construct_runtime_driver_lazy_for_test().expect("真实既有 driver 构造成功");
    let ingress = game.shared_ingress();
    let report = game.shutdown().expect("必须消费原 driver，不能只 Drop 或另造一个");
    let completion = report.runtime.expect("已存在 driver 必须返回其真实收尾事实");
    assert_eq!(completion.owned_workers, 2);
    assert_eq!(completion.joined_workers, 2);
    assert_eq!(completion.worker_results.len(), 2);
    assert_eq!((completion.group_finals, completion.desk_finals), (1, 1));
    assert!(completion.worker_results.iter().all(|worker| worker.result.is_ok()));
    assert!(ingress.enqueue_player_intent(AccountId(0), cancel()).is_err());
}

#[test]
fn shutdown_poisoned_driver_only_recovers_owner_for_failure_cleanup() {
    let mut game = game();
    game.construct_runtime_driver_lazy_for_test().expect("真实既有 driver 构造成功");
    let ingress = game.shared_ingress();
    let cell = game.runtime_driver.as_ref().expect("driver 在位");
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = cell.lock().expect("首次加锁正常");
        panic!("shutdown-poison-owner-fixture");
    })).is_err());
    assert!(cell.is_poisoned());
    let failure = game.shutdown().expect_err("poison owner 不能恢复健康 Drain 或回成功 ack");
    assert_eq!(failure.primary.stage, "runtime_driver_mutex");
    assert!(failure.primary.detail.contains("poison"));
    let completion = failure.completion.expect("poison 失败仍须取得 owner 并真实销毁");
    assert_eq!(completion.owned_workers, 2);
    assert_eq!(completion.joined_workers, 2);
    assert_eq!(completion.worker_results.len(), 2);
    assert_eq!((completion.group_finals, completion.desk_finals), (0, 0), "失败销毁不能冒正常 Final");
    assert!(ingress.enqueue_player_intent(AccountId(0), cancel()).is_err());
}

#[test]
fn shutdown_keeps_normal_protocol_primary_after_sever_and_still_joins_every_owner() {
    let mut game = game();
    game.construct_runtime_driver_lazy_for_test().expect("真实既有 driver 构造成功");
    game.runtime_driver.as_ref().unwrap().lock().unwrap().sever_world_for_test();
    let failure = game.shutdown().expect_err("已 sever 的活动 World 不得返回成功");
    assert!(!failure.primary.detail.is_empty());
    let completion = failure.completion.expect("停止主流程失败不能省略真实 join 事实");
    assert_eq!(completion.owned_workers, 2);
    assert_eq!(completion.joined_workers, 2);
    assert_eq!(completion.worker_results.len(), 2);
}

#[test]
fn shutdown_terminal_session_keeps_original_fatal_and_still_consumes_existing_driver() {
    let mut game = game();
    game.construct_runtime_driver_lazy_for_test().expect("真实既有 driver 构造成功");
    game.poison = Some(crate::session::StepFatal::InvariantViolation {
        location: "shutdown-terminal-fixture".to_owned(),
        description: "首错不能被 require_healthy 早退覆盖".to_owned(),
    });
    let failure = game.shutdown().expect_err("terminal session 不能冒健康关闭成功");
    assert_eq!(failure.primary.stage, "session_poison");
    assert!(failure.primary.detail.contains("shutdown-terminal-fixture"));
    assert!(failure.primary.detail.contains("首错不能被 require_healthy 早退覆盖"));
    let completion = failure.completion.expect("terminal session 仍须回收原 driver");
    assert_eq!(completion.owned_workers, 2);
    assert_eq!(completion.joined_workers, 2);
    assert_eq!(completion.worker_results.len(), 2);
    assert_eq!((completion.group_finals, completion.desk_finals), (0, 0));
}
