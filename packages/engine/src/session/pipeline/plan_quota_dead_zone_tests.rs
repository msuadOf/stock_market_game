//! 死区（候选全被 AccountValidation 拒绝）推进见证。
//!
//! Sol 终审·冗余收口（执行期 root-wait 穿透删除）：原 e2_integration/e1_integration
//! 两用例以 root gate 闩住在飞 root、经 `next_ready_batch_typed`（执行期阻塞拉取）
//! 构造「operations 空+roots 未排空」死区——该形态依赖已删除的执行期 root 计算
//! （生产 root 全部在 capture 期 `drain_ready_accounts_at_capture` 闭环，执行期
//! 不存在未排空 roots，死区形状生产不可构造）。两用例随之退役；等价覆盖=
//! capture 闭环边界见证（ready_ingress_tests::capture_boundary_asserts_drain_and_
//! personal_merge_at_return 空/非空 root×单/多 worker）+ 本文件存留的
//! e1_full_transaction（真实完整事务全拒形态整体完成）。注入（cfg(test) 全局
//! 静态 ⇒ 本文件用例一律 #[ignore] 串行，结束清空）。

use super::account_validation::set_validation_reject_accounts_for_test;
use super::account_validation_context::build_account_validation_context;
use super::ready_ingress::validate_available_ready;
use super::ready_stock_stream::{FeedOutcome, ReadyStockStream};
use super::stock_stream::StockStreamProgress;
use super::{AccountValidatorDriver, DecisionResourceSnapshot, EnvelopeLedger};
use crate::session::plan_chain_candidates_tests::execution_fixture_with_compute;
use crate::{AccountId, Event};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// 普通测试 10s 硬上限（项目纪律）。
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(10)
}

fn dedicated_compute() -> Arc<crate::compute::ComputeContext> {
    crate::compute::ComputeContext::with_workers(3).unwrap()
}

/// 观察者闩（本文件专用形态）：进入 bound 的 root 全部阻塞在观察点直至
/// 放行（per-ComputeContext，专用池不串扰）。
type RootGate = Arc<(Mutex<u8>, Condvar)>;

fn assert_guards_returned(compute: &Arc<crate::compute::ComputeContext>) {
    let wait = deadline();
    while compute.admission_snapshot() != (0, 0) {
        assert!(
            Instant::now() < wait,
            "guard 必须最终归还（admission={:?}）",
            compute.admission_snapshot()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
#[ignore = "口径=串行独立运行：同 e2_integration（全局注入）；node scripts/run-engine-lib-tests.mjs normal --ignored --bin <bin> plan_quota_dead_zone_tests"]
fn e1_full_transaction_all_rejected_completes() {
    set_validation_reject_accounts_for_test(&[AccountId(1)]);
    let compute = dedicated_compute();
    let (mut session, request) = execution_fixture_with_compute(compute.clone());
    let market = session.build_market_view();
    let mut roots = crate::session::plan_chain_candidates::PlanChainOperationBatch::accounts(
        vec![AccountId(2)],
        market,
        Default::default(),
        Default::default(),
        crate::calendar::CivilInstant::new(
            crate::calendar::CivilDate::from_iso("2030-01-02").unwrap(),
            0,
        )
        .unwrap(),
        Default::default(),
    );
    roots.push_execution(request);
    let events = super::run_injected_roots_market_tick_for_test(&mut session, roots)
        .expect("全拒形态的完整事务必须整体完成（不得楔死）");
    set_validation_reject_accounts_for_test(&[]);
    let rejections = events
        .iter()
        .filter(|event| {
            matches!(event, Event::IntentRejected { account, .. } if *account == AccountId(1))
        })
        .count();
    assert!(
        rejections >= 1,
        "F 全拒事实必须以 IntentRejected 事件入流（实际事件 {} 条）",
        events.len()
    );
    assert_guards_returned(&compute);
}
