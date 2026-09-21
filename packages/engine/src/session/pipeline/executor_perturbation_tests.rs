use super::*;
use crate::{AccountId, GameSession, Intent, Money, Side, TradingPhase};

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

fn run(
    auction: bool,
    threads: usize,
    config: ExecutorPerturbation,
) -> (Vec<Vec<u8>>, Vec<ExecutorOrderRecord>) {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap()
        .install(|| {
            with_executor_perturbation(config, || {
                let mut game = fixture(auction);
                let mut observations = Vec::new();
                // Includes auction completion, PreOpen, continuous, and day-end finalizers.
                for _ in 0..8 {
                    let events = game.step().unwrap();
                    observations.push(serde_json::to_vec(&events).unwrap());
                    observations.push(serde_json::to_vec(&game.save().unwrap()).unwrap());
                }
                observations
            })
            .unwrap()
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
fn executor_perturbation_public_path_preserves_authority_events_and_save_across_budgets() {
    for auction in [false, true] {
        let (canonical, baseline) = run(auction, 1, config(ExecutorPermutation::Canonical));
        for threads in [1, 2, 4] {
            for permutation in [
                ExecutorPermutation::Reverse,
                ExecutorPermutation::RotateLeft,
            ] {
                let (actual, records) = run(auction, threads, config(permutation));
                assert_eq!(
                    actual, canonical,
                    "auction={auction}, threads={threads}, {permutation:?}"
                );
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
                    assert_eq!(before.len(), after.len());
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
fn executor_perturbation_actual_multi_stock_multi_leg_receipt_indices_are_stable() {
    let execute = |permutation| {
        with_executor_perturbation(config(permutation), || {
            let mut game = fixture(false);
            assert_eq!(game.phase(), TradingPhase::Continuous);
            game.hydrate_or_validate_envelope_ledger().unwrap();
            let committed = super::b1_continuous_transaction::prepare_b1_continuous_tick(&mut game)
                .unwrap()
                .commit();
            let receipts = committed.commit.evidence.receipts();
            assert!(
                receipts.len() >= 8,
                "two stocks each have multiple fill legs"
            );
            receipts
                .iter()
                .map(|receipt| {
                    (
                        receipt.index,
                        receipt.local_key.clone(),
                        format!("{receipt:?}"),
                    )
                })
                .collect::<Vec<_>>()
        })
        .unwrap()
        .0
    };
    let canonical = execute(ExecutorPermutation::Canonical);
    for permutation in [
        ExecutorPermutation::Reverse,
        ExecutorPermutation::RotateLeft,
    ] {
        assert_eq!(execute(permutation), canonical);
    }
}

#[test]
fn executor_perturbation_gate_each_disabled_merge_fails_without_partial_commit() {
    for auction in [false, true] {
        for disabled in [
            CanonicalMerge::Account,
            CanonicalMerge::Stock,
            CanonicalMerge::Completion,
        ] {
            let mut game = fixture(auction);
            let mut perturbation = config(ExecutorPermutation::Reverse);
            perturbation.disable_merge = Some(disabled);
            let mut failed = false;
            for _ in 0..8 {
                let before = game.business_state_hash().unwrap();
                let (result, records) =
                    with_executor_perturbation(perturbation, || game.step()).unwrap();
                if result.is_err() {
                    assert!(!records.is_empty());
                    assert_eq!(game.business_state_hash().unwrap(), before);
                    assert!(game.poison_reason().is_some());
                    failed = true;
                    break;
                }
            }
            assert!(
                failed,
                "disabled {disabled:?} unexpectedly passed; auction={auction}"
            );
        }
    }
}

#[test]
fn executor_perturbation_dimensions_are_independent_and_scopes_do_not_leak() {
    let (baseline, canonical) = run(false, 1, ExecutorPerturbation::default());
    for dimension in [
        CanonicalMerge::Account,
        CanonicalMerge::Stock,
        CanonicalMerge::Completion,
    ] {
        let mut perturbation = ExecutorPerturbation::default();
        let boundary = match dimension {
            CanonicalMerge::Account => {
                perturbation.account_shards = ExecutorPermutation::Reverse;
                ExecutorBoundary::P3AccountShards
            }
            CanonicalMerge::Stock => {
                perturbation.stock_shards = ExecutorPermutation::Reverse;
                ExecutorBoundary::P4ContinuousStockShards
            }
            CanonicalMerge::Completion => {
                perturbation.worker_results = ExecutorPermutation::Reverse;
                ExecutorBoundary::P4ContinuousWorkerResults
            }
        };
        let (actual, records) = run(false, 1, perturbation);
        assert_eq!(actual, baseline);
        assert_ne!(
            records_at(&records, boundary)[0],
            records_at(&canonical, boundary)[0]
        );
    }
    let mut game = fixture(false);
    let first = game.step().unwrap();
    assert_eq!(serde_json::to_vec(&first).unwrap(), baseline[0]);
    let (nested, records) = with_executor_perturbation(ExecutorPerturbation::default(), || {
        with_executor_perturbation(ExecutorPerturbation::default(), || unreachable!())
    })
    .unwrap();
    assert!(nested.is_err());
    assert!(records.is_empty());
    assert!(with_executor_perturbation(ExecutorPerturbation::default(), || ()).is_ok());
}
