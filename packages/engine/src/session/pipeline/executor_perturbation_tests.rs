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
    let codes = game.markets.keys().cloned().collect::<Vec<_>>();
    // Closed production StrategyState remains installed. Exogenous test candidates
    // exercise the normal P2/P3 path without replacing the production dispatcher.
    for account in [AccountId(1), AccountId(2)] {
        for code in &codes {
            game.accounts
                .get_mut(&account)
                .unwrap()
                .grant_position(code.clone(), 1_000, Money::from_cents(1_000_000))
                .unwrap();
        }
    }
    for (account, side, qty) in [
        (AccountId(2), Side::Sell, 200),
        (AccountId(0), Side::Buy, 300),
        (AccountId(1), Side::Sell, 100),
    ] {
        for code in codes.iter().rev() {
            game.pending_player.push((
                account,
                Intent::PlaceLimit {
                    code: code.clone(),
                    side,
                    price: Money::from_cents(1_000),
                    qty,
                },
            ));
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

fn run(auction: bool, threads: usize, config: ExecutorPerturbation) -> Vec<ExecutorOrderRecord> {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap()
        .install(|| {
            with_executor_perturbation(config, || {
                let mut game = fixture(auction);
                let codes = game.markets.keys().cloned().collect::<Vec<_>>();
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
                let player = game.accounts.get(&AccountId(0)).unwrap();
                assert!(player.cash < game.setup.config.starting_cash);
                for code in codes {
                    assert_eq!(player_fills.get(&code), Some(&300), "{code:?} player fill");
                    assert_eq!(
                        player.positions.get(&code).map(|position| position.qty),
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

#[test]
fn executor_perturbation_exercises_public_path_across_budgets() {
    for auction in [false, true] {
        let baseline = run(auction, 1, config(ExecutorPermutation::Canonical));
        for threads in [1, 2, 4] {
            for permutation in [
                ExecutorPermutation::Reverse,
                ExecutorPermutation::RotateLeft,
            ] {
                let records = run(auction, threads, config(permutation));
                let stock_boundary = if auction {
                    ExecutorBoundary::P4AuctionStockShards
                } else {
                    ExecutorBoundary::P4ContinuousStockShards
                };
                let completion_boundary = if auction {
                    ExecutorBoundary::P4AuctionWorkerResults
                } else {
                    ExecutorBoundary::P4ContinuousWorkerResults
                };
                for boundary in [
                    ExecutorBoundary::P3AccountShards,
                    stock_boundary,
                    completion_boundary,
                    ExecutorBoundary::P5ReceiptResults,
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
            }
        }
    }
}

#[test]
fn executor_perturbation_allows_stock_output_reordering() {
    for auction in [false, true] {
        let mut game = fixture(auction);
        let perturbation = config(ExecutorPermutation::Reverse);
        for _ in 0..8 {
            let before_tick = game.tick;
            let (result, _) = with_executor_perturbation(perturbation, || game.step()).unwrap();
            result.expect("cross-stock output order must not fail the tick");
            assert_eq!(game.tick, before_tick + 1);
            assert!(game.poison_reason().is_none());
        }
    }
}

#[test]
fn executor_perturbation_dimensions_are_independent_and_scopes_do_not_leak() {
    let canonical = run(false, 1, ExecutorPerturbation::default());
    for stock_shards in [true, false] {
        let mut perturbation = ExecutorPerturbation::default();
        let boundary = if stock_shards {
            perturbation.stock_shards = ExecutorPermutation::Reverse;
            ExecutorBoundary::P4ContinuousStockShards
        } else {
            perturbation.worker_results = ExecutorPermutation::Reverse;
            ExecutorBoundary::P4ContinuousWorkerResults
        };
        let records = run(false, 1, perturbation);
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
}
