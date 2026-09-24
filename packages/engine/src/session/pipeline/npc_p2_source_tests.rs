use super::npc_p2_source::{npc_rng_seed, run_npc_p2_source, NpcP2SourceError};
use super::{DecisionAccountInput, DecisionSnapshot, P2CandidateKey};
use crate::behavior::BehaviorMarketObservation;
use crate::observation::{
    AccountRiskObservation, EqualWeightMarketObservation, HorizonReturn, PricePathObservation,
};
use crate::strategy::{
    MarketView, MomentumStrategy, SelfView, StockView, StrategyState, ZiNoiseStrategy,
};
use crate::{AccountId, AccountKind, Money, SplitMix64, StockCode, TradingPhase};
use std::collections::BTreeMap;
use std::sync::Arc;

fn account_input() -> DecisionAccountInput {
    DecisionAccountInput::new(
        AccountKind::Inst,
        SelfView {
            cash: Money::from_cents(10_000),
            positions: BTreeMap::new(),
        },
        StrategyState::Momentum(MomentumStrategy::new(5, 0.02, 100).unwrap()),
        None,
        None,
    )
}

fn snapshot(accounts: Vec<AccountId>) -> Arc<DecisionSnapshot> {
    let inputs: BTreeMap<_, _> = accounts
        .iter()
        .copied()
        .map(|account| (account, account_input()))
        .collect();
    Arc::new(
        DecisionSnapshot::new(
            7,
            42,
            TradingPhase::Continuous,
            11,
            MarketView {
                stocks: BTreeMap::new(),
                tick: 7,
                market_minute: 11,
            },
            None,
            accounts,
            inputs,
        )
        .unwrap(),
    )
}

fn unavailable_horizon() -> HorizonReturn {
    HorizonReturn {
        requested_span: 30,
        available_span: 0,
        return_ratio: None,
    }
}

fn retail_snapshot() -> Arc<DecisionSnapshot> {
    let code = StockCode("600888".to_owned());
    let market = MarketView {
        stocks: [(
            code.clone(),
            StockView {
                best_bid: Some(Money::from_cents(999)),
                best_ask: Some(Money::from_cents(1_001)),
                last_price: Money::from_cents(1_000),
                recent_prices: vec![Money::from_cents(1_000)],
                recent_market_minute_prices: Vec::new(),
                relative_volume: 1.0,
                order_book_imbalance: 0.0,
            },
        )]
        .into(),
        tick: 7,
        market_minute: 11,
    };
    let path = PricePathObservation {
        one_minute: unavailable_horizon(),
        thirty_minute: unavailable_horizon(),
        intraday: unavailable_horizon(),
        five_day: unavailable_horizon(),
        twenty_day: unavailable_horizon(),
        one_hundred_twenty_day: unavailable_horizon(),
        two_hundred_fifty_day: unavailable_horizon(),
        prior_thirty_minute_range: None,
    };
    let behavior_market = BehaviorMarketObservation {
        price_paths: [(code, path)].into(),
        thirty_minute_market: EqualWeightMarketObservation {
            total_stock_count: 1,
            observed_stock_count: 0,
            equal_weight_return: None,
            advance_fraction: None,
            decline_fraction: None,
            unchanged_fraction: None,
        },
    };
    let risk = AccountRiskObservation {
        equity: Money::from_cents(10_000_000),
        return_from_reference: None,
        drawdown_from_peak: None,
        positions: BTreeMap::new(),
    };
    let accounts = [AccountId(1), AccountId(2)]
        .into_iter()
        .map(|account| {
            (
                account,
                DecisionAccountInput::new(
                    AccountKind::Retail,
                    SelfView {
                        cash: Money::from_cents(10_000_000),
                        positions: BTreeMap::new(),
                    },
                    StrategyState::ZiNoise(ZiNoiseStrategy::new(1.0, 100, 0.5, 1).unwrap()),
                    Some(risk.clone()),
                    Some(crate::RetailExperienceState::without_equity_reference()),
                ),
            )
        })
        .collect();
    Arc::new(
        DecisionSnapshot::new(
            7,
            8,
            TradingPhase::Continuous,
            11,
            market,
            Some(behavior_market),
            vec![AccountId(1), AccountId(2)],
            accounts,
        )
        .unwrap(),
    )
}

fn multi_intent_snapshot() -> Arc<DecisionSnapshot> {
    let market = MarketView {
        stocks: ["600001", "600002"]
            .into_iter()
            .map(|code| {
                (
                    StockCode(code.to_owned()),
                    StockView {
                        best_bid: Some(Money::from_cents(1_049)),
                        best_ask: Some(Money::from_cents(1_051)),
                        last_price: Money::from_cents(1_050),
                        recent_prices: vec![
                            Money::from_cents(1_000),
                            Money::from_cents(1_020),
                            Money::from_cents(1_050),
                        ],
                        recent_market_minute_prices: vec![
                            Money::from_cents(1_000),
                            Money::from_cents(1_020),
                            Money::from_cents(1_050),
                        ],
                        relative_volume: 1.0,
                        order_book_imbalance: 0.0,
                    },
                )
            })
            .collect(),
        tick: 7,
        market_minute: 11,
    };
    let account = AccountId(9);
    let accounts = [(
        account,
        DecisionAccountInput::new(
            AccountKind::Hot,
            SelfView {
                cash: Money::from_cents(10_000_000),
                positions: BTreeMap::new(),
            },
            StrategyState::Momentum(MomentumStrategy::new(3, 0.02, 100).unwrap()),
            None,
            None,
        ),
    )]
    .into();
    Arc::new(
        DecisionSnapshot::new(
            7,
            8,
            TradingPhase::Continuous,
            11,
            market,
            None,
            vec![account],
            accounts,
        )
        .unwrap(),
    )
}

#[test]
fn npc_p2_source_hydrates_in_stable_account_order_and_returns_state_without_mutating_snapshot() {
    let first = AccountId(1);
    let second = AccountId(2);
    let snapshot = snapshot(vec![first, second]);
    let snapshot_before = snapshot.clone();

    let output = run_npc_p2_source(snapshot.clone()).unwrap();

    assert_eq!(output.accounts(), &[first, second]);
    assert!(output.intents().is_empty());
    assert_eq!(
        output.strategy_state(first).unwrap(),
        snapshot.account(first).unwrap().strategy_state()
    );
    assert_eq!(
        output.strategy_state(second).unwrap(),
        snapshot.account(second).unwrap().strategy_state()
    );
    assert!(Arc::ptr_eq(&snapshot, &snapshot_before));
    assert_eq!(snapshot.due_npc_ids(), &[first, second]);
}

#[test]
fn npc_p2_source_is_repeatable_for_the_same_sealed_snapshot() {
    let snapshot = snapshot(vec![AccountId(1)]);

    let first = run_npc_p2_source(snapshot.clone()).unwrap();
    let second = run_npc_p2_source(snapshot).unwrap();

    assert_eq!(first.accounts(), second.accounts());
    assert_eq!(
        serde_json::to_vec(first.intents()).unwrap(),
        serde_json::to_vec(second.intents()).unwrap()
    );
    for account in first.accounts() {
        assert_eq!(
            first.strategy_state(*account).unwrap(),
            second.strategy_state(*account).unwrap()
        );
    }
}

#[test]
fn npc_p2_source_keeps_retail_decisions_and_candidate_keys_across_thread_pools() {
    let snapshot = retail_snapshot();
    let input_before: Vec<_> = snapshot
        .due_npc_ids()
        .iter()
        .map(|account| {
            let input = snapshot.account(*account).unwrap();
            (
                *account,
                serde_json::to_vec(input.self_view()).unwrap(),
                serde_json::to_vec(input.strategy_state()).unwrap(),
            )
        })
        .collect();
    let one_thread = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap()
        .install(|| run_npc_p2_source(snapshot.clone()).unwrap());
    let two_threads = rayon::ThreadPoolBuilder::new()
        .num_threads(2)
        .build()
        .unwrap()
        .install(|| run_npc_p2_source(snapshot.clone()).unwrap());

    assert_eq!(one_thread.accounts(), &[AccountId(1), AccountId(2)]);
    assert_eq!(
        serde_json::to_vec(one_thread.intents()).unwrap(),
        serde_json::to_vec(two_threads.intents()).unwrap()
    );
    assert_eq!(one_thread.intents().len(), 2);
    for (index, intent) in one_thread.intents().iter().enumerate() {
        assert_eq!(
            intent.key(),
            &P2CandidateKey::npc(AccountId(u64::try_from(index + 1).unwrap()), 0)
        );
        assert!(matches!(intent.intent(), crate::Intent::PlaceLimit { .. }));
    }

    for account in snapshot.due_npc_ids() {
        let input = snapshot.account(*account).unwrap();
        let mut direct_strategy = input.strategy_state().clone().into_strategy().unwrap();
        let account_seed = snapshot.npc_seed_base()
            ^ snapshot.tick().wrapping_mul(0x9E3779B97F4A7C15)
            ^ account.0.wrapping_mul(0x6A09E667F3BCC908);
        let mut rng = SplitMix64::new(account_seed);
        let expected = direct_strategy.decide_with_experience(
            snapshot.market(),
            input.self_view(),
            snapshot.behavior_market(),
            input.account_risk(),
            input.retail_experience(),
            snapshot.market_minute(),
            &mut rng,
        );
        let actual = one_thread
            .account_outputs()
            .iter()
            .find(|output| output.account() == *account)
            .unwrap();
        assert!(actual.position_decision().is_some());
        assert!(!actual.reviewed_stocks().is_empty());
        assert!(actual.updates_working_quotes());
        assert!(!actual.uses_parent_order_execution());
        assert_eq!(
            actual.position_decision(),
            expected.position_decision.as_ref()
        );
        let actual_intents: Vec<_> = one_thread
            .intents()
            .iter()
            .filter(|intent| matches!(intent.key(), P2CandidateKey::Npc { account: owner, .. } if owner == account))
            .map(|intent| serde_json::to_vec(intent.intent()).unwrap())
            .collect();
        let expected_intents: Vec<_> = expected
            .intents
            .iter()
            .map(|intent| serde_json::to_vec(intent).unwrap())
            .collect();
        assert_eq!(actual_intents, expected_intents);
    }
    let input_after: Vec<_> = snapshot
        .due_npc_ids()
        .iter()
        .map(|account| {
            let input = snapshot.account(*account).unwrap();
            (
                *account,
                serde_json::to_vec(input.self_view()).unwrap(),
                serde_json::to_vec(input.strategy_state()).unwrap(),
            )
        })
        .collect();
    assert_eq!(input_after, input_before);
}

#[test]
fn npc_p2_source_assigns_incrementing_local_indexes_to_one_accounts_multiple_intents() {
    let output = run_npc_p2_source(multi_intent_snapshot()).unwrap();

    assert_eq!(output.intents().len(), 2);
    assert_eq!(
        output.intents()[0].key(),
        &P2CandidateKey::npc(AccountId(9), 0)
    );
    assert_eq!(
        output.intents()[1].key(),
        &P2CandidateKey::npc(AccountId(9), 1)
    );
    assert!(output.intents().iter().all(|intent| matches!(
        intent.intent(),
        crate::Intent::PlaceLimit {
            side: crate::Side::Buy,
            ..
        }
    )));
}

#[test]
fn npc_p2_source_derives_rng_from_tick_and_account() {
    let base = 0x1234_5678_9abc_def0;
    let tick = 97_u64;
    let account = AccountId(41);

    assert_eq!(
        npc_rng_seed(base, tick, account),
        base ^ tick.wrapping_mul(0x9E3779B97F4A7C15) ^ account.0.wrapping_mul(0x6A09E667F3BCC908)
    );
}

#[test]
fn npc_p2_source_reports_first_invalid_account_independent_of_worker_count() {
    let first = AccountId(1);
    let second = AccountId(2);
    let third = AccountId(3);
    let mut invalid_state = serde_json::to_value(StrategyState::Momentum(
        MomentumStrategy::new(5, 0.02, 100).unwrap(),
    ))
    .unwrap();
    invalid_state["Momentum"]["order_size"] = serde_json::json!(0);
    let invalid_state: StrategyState = serde_json::from_value(invalid_state).unwrap();
    let mut accounts = BTreeMap::new();
    accounts.insert(first, account_input());
    accounts.insert(
        second,
        DecisionAccountInput::new(
            AccountKind::Inst,
            SelfView {
                cash: Money::from_cents(10_000),
                positions: BTreeMap::new(),
            },
            invalid_state.clone(),
            None,
            None,
        ),
    );
    accounts.insert(
        third,
        DecisionAccountInput::new(
            AccountKind::Inst,
            SelfView {
                cash: Money::from_cents(10_000),
                positions: BTreeMap::new(),
            },
            invalid_state,
            None,
            None,
        ),
    );
    let snapshot = Arc::new(
        DecisionSnapshot::new(
            7,
            42,
            TradingPhase::Continuous,
            11,
            MarketView {
                stocks: BTreeMap::new(),
                tick: 7,
                market_minute: 11,
            },
            None,
            vec![first, second, third],
            accounts,
        )
        .unwrap(),
    );
    let encoded_before =
        serde_json::to_vec(snapshot.account(second).unwrap().strategy_state()).unwrap();

    for workers in [1, 4] {
        let error = rayon::ThreadPoolBuilder::new()
            .num_threads(workers)
            .build()
            .unwrap()
            .install(|| run_npc_p2_source(snapshot.clone()))
            .unwrap_err();
        assert!(matches!(
            error,
            NpcP2SourceError::StrategyHydration { account, .. } if account == second
        ));
    }
    assert_eq!(
        serde_json::to_vec(snapshot.account(second).unwrap().strategy_state()).unwrap(),
        encoded_before
    );
}
