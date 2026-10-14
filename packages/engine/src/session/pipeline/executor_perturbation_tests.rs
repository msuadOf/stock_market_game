use super::*;
use crate::{AccountId, GameSession, Intent, Money, Side};
use std::collections::BTreeMap;

fn fixture(auction: bool) -> GameSession {
    let mut setup = crate::session::npc_working_quote_tests::two_stock_quote_setup();
    setup.npcs.inst_count = 0;
    setup.npcs.retail_count = 2;
    setup.auction_ticks = if auction { 3 } else { 0 };
    setup.ticks_per_day = 8;
    setup.closing_auction_ticks = 1;
    let mut game = GameSession::new(setup, 42).unwrap();
    if auction {
        game.fixture_sync_market_modes_for_test();
    }
    let codes = game.state.markets.keys().cloned().collect::<Vec<_>>();
    let moment = crate::experience::ExperienceMoment {
        civil_date: game.civil_date(),
        market_minute: game.current_market_minute(),
        trading_day: u64::from(game.day()),
    };
    // 保持生产 StrategyState 已安装。外部测试候选仍走真实
    // DecisionAndContinuation/AccountValidation 路径，不替换生产 dispatcher。
    for account in [AccountId(1), AccountId(2)] {
        for code in &codes {
            game.fixture_account_mut(&account)
                .unwrap()
                .grant_position(code.clone(), 1_000, Money::from_cents(1_000_000))
                .unwrap();
            game.state
                .retail_experience
                .get_mut(&account)
                .unwrap()
                .initialize_holding_dated(
                    code,
                    Some(Money::from_cents(1_000_000)),
                    Money::from_cents(1_000),
                    moment,
                )
                .unwrap();
        }
    }
    for (account, side, qty) in [
        (AccountId(2), Side::Sell, 200),
        (AccountId(0), Side::Buy, 300),
        (AccountId(1), Side::Sell, 100),
    ] {
        for code in codes.iter().rev() {
            let received = game
                .state
                .ingress_receipt_cursors
                .receive(
                    account,
                    Intent::PlaceLimit {
                        code: code.clone(),
                        side,
                        price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
                        qty,
                    },
                )
                .unwrap();
            game.state.pending_player.push(received);
        }
    }
    game
}

fn config(permutation: ExecutorPermutation) -> ExecutorPerturbation {
    ExecutorPerturbation {
        account_shards: permutation,
        stock_shards: permutation,
        worker_results: permutation,
        disable_merge: None,
    }
}

fn run(
    prototype: &GameSession,
    threads: usize,
    config: ExecutorPerturbation,
) -> Vec<ExecutorOrderRecord> {
    let pool = std::sync::Arc::new(
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap(),
    );
    let compute = crate::compute::ComputeContext::from_pool(pool.clone());
    pool.install(|| {
        with_executor_perturbation(config, || {
            let mut game = prototype.clone_for_tick_shadow().unwrap();
            game.rebind_compute_for_verification(compute.clone());
            let codes = game.state.markets.keys().cloned().collect::<Vec<_>>();
            let mut player_fills = BTreeMap::<_, u64>::new();
            // Includes auction completion, PreOpen, continuous, and day-end finalizers.
            for _ in 0..8 {
                let events = game.step().unwrap();
                for event in events {
                    if let crate::Event::Trade {
                        code,
                        maker,
                        taker,
                        qty,
                        ..
                    } = event
                    {
                        if maker == AccountId(0) || taker == AccountId(0) {
                            *player_fills.entry(code).or_default() += u64::from(qty);
                        }
                    }
                }
                game.save().unwrap();
            }
            let player = game.state.accounts.get(&AccountId(0)).unwrap();
            assert!(player.cash() < game.state.setup.config.starting_cash);
            for code in codes {
                assert_eq!(player_fills.get(&code), Some(&300), "{code:?} player fill");
                assert_eq!(
                    player.positions().get(&code).map(|position| position.qty()),
                    Some(300)
                );
            }
        })
        .unwrap()
        .1
    })
}

fn records_at(
    records: &[ExecutorOrderRecord],
    boundary: ExecutorBoundary,
) -> Vec<&ExecutorOrderRecord> {
    records
        .iter()
        .filter(|record| record.boundary == boundary && record.identities.len() >= 2)
        .collect()
}

fn assert_public_path_budget(auction: bool, threads: usize) {
    let prototype = fixture(auction);
    let prototype_before = prototype.session_state_hash().unwrap();
    let baseline = run(&prototype, threads, config(ExecutorPermutation::Canonical));
    for permutation in [
        ExecutorPermutation::Reverse,
        ExecutorPermutation::RotateLeft,
    ] {
        let records = run(&prototype, threads, config(permutation));
        // 两种交易模式共用实际 stock stream/finish 接缝；扰动不能再观察已删除的独立竞价调度器。
        let stock_boundary = ExecutorBoundary::ContinuousStockShards;
        let completion_boundary = ExecutorBoundary::ContinuousWorkerResults;
        for boundary in [
            ExecutorBoundary::AccountValidationShards,
            stock_boundary,
            completion_boundary,
            ExecutorBoundary::AggregatedReceiptResults,
        ] {
            let before = records_at(&baseline, boundary);
            let after = records_at(&records, boundary);
            assert!(!after.is_empty(), "missing actual nonempty {boundary:?}");
            assert_ne!(
                before[0].identities, after[0].identities,
                "inert {boundary:?}"
            );
            assert!(after[0].item_counts.iter().all(|count| *count > 0));
        }
        assert_eq!(prototype.session_state_hash().unwrap(), prototype_before);
    }
}

#[test]
fn executor_perturbation_exercises_continuous_with_one_thread() {
    assert_public_path_budget(false, 1);
}

#[test]
fn executor_perturbation_exercises_continuous_with_two_threads() {
    assert_public_path_budget(false, 2);
}

#[test]
fn executor_perturbation_exercises_continuous_with_four_threads() {
    assert_public_path_budget(false, 4);
}

#[test]
fn executor_perturbation_exercises_auction_with_one_thread() {
    assert_public_path_budget(true, 1);
}

#[test]
fn executor_perturbation_exercises_auction_with_two_threads() {
    assert_public_path_budget(true, 2);
}

#[test]
fn executor_perturbation_exercises_auction_with_four_threads() {
    assert_public_path_budget(true, 4);
}

#[test]
fn executor_perturbation_allows_stock_output_reordering() {
    for auction in [false, true] {
        let mut game = fixture(auction);
        let perturbation = config(ExecutorPermutation::Reverse);
        for _ in 0..8 {
            let before_tick = game.state.tick;
            let (result, _) = with_executor_perturbation(perturbation, || game.step()).unwrap();
            result.expect("cross-stock output order must not fail the tick");
            assert_eq!(game.state.tick, before_tick + 1);
            assert!(game.poison_reason().is_none());
        }
    }
}

#[test]
fn executor_perturbation_dimensions_are_independent_and_scopes_do_not_leak() {
    let prototype = fixture(false);
    let prototype_before = prototype.session_state_hash().unwrap();
    let canonical = run(&prototype, 1, ExecutorPerturbation::default());
    for stock_shards in [true, false] {
        let mut perturbation = ExecutorPerturbation::default();
        let boundary = if stock_shards {
            perturbation.stock_shards = ExecutorPermutation::Reverse;
            ExecutorBoundary::ContinuousStockShards
        } else {
            perturbation.worker_results = ExecutorPermutation::Reverse;
            ExecutorBoundary::ContinuousWorkerResults
        };
        let records = run(&prototype, 1, perturbation);
        assert_ne!(
            records_at(&records, boundary)[0],
            records_at(&canonical, boundary)[0]
        );
    }
    let (nested, records) = with_executor_perturbation(ExecutorPerturbation::default(), || {
        with_executor_perturbation(ExecutorPerturbation::default(), || unreachable!())
    })
    .unwrap();
    assert!(nested.is_err());
    assert!(records.is_empty());
    assert!(with_executor_perturbation(ExecutorPerturbation::default(), || ()).is_ok());
    assert_eq!(prototype.session_state_hash().unwrap(), prototype_before);
}
#[test]
fn compute_context_executor_binding_restores_after_error_unwind_and_nested_burst() {
    with_executor_perturbation(ExecutorPerturbation::default(), || {
        let original = super::executor_perturbation::current_executor_binding();
        let result: Result<(), ()> = super::executor_perturbation::with_executor_binding(
            super::executor_perturbation::ExecutorBinding::empty(), || {
                assert!(super::executor_perturbation::current_executor_binding().is_empty());
                super::executor_perturbation::with_executor_binding(original.clone(), || {
                    assert!(super::executor_perturbation::current_executor_binding().same_as(&original));
                });
                Err(())
            },
        );
        assert!(result.is_err());
        assert!(super::executor_perturbation::current_executor_binding().same_as(&original));
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            super::executor_perturbation::with_executor_binding(super::executor_perturbation::ExecutorBinding::empty(), || panic!("测试扰动绑定 unwind"));
        }));
        assert!(panic.is_err());
        assert!(super::executor_perturbation::current_executor_binding().same_as(&original));
    }).unwrap();
    assert!(super::executor_perturbation::current_executor_binding().is_empty());
}

#[test]
fn compute_context_shared_host_sessions_keep_executor_controls_isolated() {
    let compute = crate::ComputeContext::with_workers(2).unwrap();
    std::thread::scope(|threads| {
        for stock_shards in [ExecutorPermutation::Canonical, ExecutorPermutation::Reverse] {
            let compute = compute.clone();
            threads.spawn(move || {
                let mut game = fixture(false);
                game.rebind_compute_for_verification(compute);
                let (result, records) = with_executor_perturbation(
                    ExecutorPerturbation { stock_shards, ..Default::default() }, || game.step(),
                ).unwrap();
                result.unwrap();
                assert!(!records_at(&records, ExecutorBoundary::ContinuousStockShards).is_empty());
            });
        }
    });
}

#[test]
fn compute_context_foreign_caller_preserves_real_executor_controls() {
    let mut game = fixture(false);
    game.rebind_compute_for_verification(crate::ComputeContext::with_workers(1).unwrap());
    let foreign = rayon::ThreadPoolBuilder::new().num_threads(3).build().unwrap();
    let (result, records) = foreign.install(|| with_executor_perturbation(
        ExecutorPerturbation { stock_shards: ExecutorPermutation::Reverse, ..Default::default() },
        || game.step(),
    )).unwrap();
    result.unwrap();
    assert!(!records_at(&records, ExecutorBoundary::ContinuousStockShards).is_empty());
    assert!(!records_at(&records, ExecutorBoundary::ContinuousWorkerResults).is_empty());
}

#[test]
fn compute_context_spawned_tasks_inherit_scope_and_sessions_do_not_cross() {
    let left = crate::ComputeContext::with_workers(2).unwrap();
    let right = crate::ComputeContext::with_workers(2).unwrap();
    std::thread::scope(|threads| {
        for (name, compute, boundary, permutation) in [
            ('a', &left, ExecutorBoundary::ContinuousWorkerResults, ExecutorPermutation::Reverse),
            ('b', &right, ExecutorBoundary::AuctionWorkerResults, ExecutorPermutation::RotateLeft),
        ] {
            threads.spawn(move || {
                let (ran_tx, ran_rx) = std::sync::mpsc::channel();
                let signaled = ran_tx.clone();
                let config = ExecutorPerturbation {
                    worker_results: permutation,
                    ..Default::default()
                };
                let ((), records) = with_executor_perturbation(config, || {
                    compute.spawn(move || {
                        let mut values = vec![1_u32, 2, 3];
                        super::executor_perturbation::reorder(boundary, &mut values, |value| {
                            (format!("probe-{value}"), 1)
                        });
                        signaled.send(()).expect("任务完成信号必须可达");
                    });
                    ran_rx
                        .recv_timeout(std::time::Duration::from_secs(2))
                        .expect("派发任务必须在超时前执行")
                })
                .unwrap();
                // 继承：派发任务内的 reorder 必须落入同一捕获
                assert!(!records_at(&records, boundary).is_empty(), "任务未继承扰动捕获 {name}");
                // 隔离：另一会话的边界不得串入本捕获
                let other = if boundary == ExecutorBoundary::ContinuousWorkerResults {
                    ExecutorBoundary::AuctionWorkerResults
                } else {
                    ExecutorBoundary::ContinuousWorkerResults
                };
                assert!(
                    records.iter().all(|record| record.boundary != other),
                    "不同会话的观察串账 {name}"
                );
            });
        }
    });
}

#[test]
fn executor_boundaries_serialize_responsibility_labels() {
    for (boundary, label) in [
        (
            ExecutorBoundary::AccountValidationShards,
            "AccountValidationShards",
        ),
        (
            ExecutorBoundary::AccountValidationWorkerResults,
            "AccountValidationWorkerResults",
        ),
        (
            ExecutorBoundary::ContinuousStockShards,
            "ContinuousStockShards",
        ),
        (
            ExecutorBoundary::ContinuousWorkerResults,
            "ContinuousWorkerResults",
        ),
        (ExecutorBoundary::AuctionStockShards, "AuctionStockShards"),
        (
            ExecutorBoundary::AuctionWorkerResults,
            "AuctionWorkerResults",
        ),
        (
            ExecutorBoundary::AggregatedReceiptResults,
            "AggregatedReceiptResults",
        ),
    ] {
        assert_eq!(
            serde_json::to_value(boundary).unwrap(),
            serde_json::json!(label)
        );
    }
}
