//! P2① 负控（进程隔离版）：producer guard 必须在 `tokio::spawn` 之前登记。
//!
//! 本用例对进程级全局诊断计数做精确 before/+1/after 比较；lib 单元测试
//! （如 auth_tests）会并发创建 actor，正确实现也会在窗口内出现 +2 或破坏
//! 归位断言。集成测试是独立进程，断言强度保持不降且不受干扰。
//!
//! 修复前 guard 在 spawn 的 async body 首次 poll 才 `fetch_add`，
//! `new_session` 返回但 future 未被 poll 时 `actor_producers` 仍为 0，
//! verification drain 可在空账本上"成功"，而 actor 随后才开始发布。
//! current_thread runtime 不经 `block_on` 不会 poll 任务，可确定性构造该窗口。

use engine::session::verification_metrics_snapshot;
use server::SessionManager;

fn minimal_setup() -> engine::SessionSetup {
    use engine::*;
    SessionSetup {
        company_system: None,
        dividend_tax_mode: None,
        flat_withholding_bp: None,
        rights_offering_enabled: false,
        issuer_repurchase_enabled: false,
        par_value_per_share: None,
        auto_corporate_foundation: false,
        report_frequency: None,
        company_operations: None,
        groups: Vec::new(),
        stocks: vec![StockSpec {
            code: StockCode("600101".into()),
            exchange: StockExchange::Shanghai,
            initial_price: Money::from_cents(1000),
            category: SecurityCategory::MainBoard,
            limit_pct: 0.1,
            tick: Money::from_cents(1),
            total_shares: 1_000_000,
            float_shares: 0,
        }],
        npcs: NpcSetup {
            retail_count: 1,
            inst_count: 1,
            hot_count: 0,
            retail_cash_median: Money::ZERO,
        },
        config: GameConfig::proposed_defaults(),
        strategy_params: StrategyParams {
            retail: RetailParams {
                arrival_rate: 0.0,
                order_size_mean: 100,
                chase_prob: 0.0,
            },
            inst: InstParams {
                margin: 0.05,
                order_size: 100,
            },
            hot: HotParams {
                lookback: 3,
                trend_threshold: 0.02,
                order_size: 100,
            },
        },
        ticks_per_day: 8,
        auction_ticks: 0,
        closing_auction_ticks: 0,
        history_len: 5,
        t1_enabled: true,
        float_allocation: FloatAllocation::random(),
        start_date: CivilDate::from_iso("2030-01-02").unwrap(),
        simulation_policy_id: SIMULATION_POLICY_ID.into(),
    }
}

#[test]
fn spawned_but_unpolled_actor_already_counts_as_active_producer() {
    // current_thread runtime：spawn 只入队，未 block_on 前绝不 poll。
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("测试 runtime");
    let before = verification_metrics_snapshot()
        .expect("全局诊断账本必须可读")
        .actor_producers;
    let manager = SessionManager::with_base_ms(1);
    {
        let _enter = runtime.enter();
        let id = manager
            .new_session(minimal_setup(), 42)
            .expect("最小 setup 必须能建会话");
        let during = verification_metrics_snapshot()
            .expect("全局诊断账本必须可读")
            .actor_producers;
        assert_eq!(
            during,
            before + 1,
            "已 spawn 未 poll 的 actor 必须已经登记为 producer；否则 drain 可在空账本上伪成功"
        );
        manager.remove(&id);
    }
    // runtime drop 在本线程丢弃未 poll 任务；guard 随 future 捕获释放。
    drop(runtime);
    let after = verification_metrics_snapshot()
        .expect("全局诊断账本必须可读")
        .actor_producers;
    assert_eq!(
        after, before,
        "未 poll 任务被丢弃后 producer 计数必须归位，不得泄漏"
    );
}
