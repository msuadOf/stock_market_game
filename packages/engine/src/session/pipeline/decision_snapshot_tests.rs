use super::*;
use crate::behavior::BehaviorMarketObservation;
use crate::observation::{AccountRiskObservation, EqualWeightMarketObservation};
use crate::strategy::{MarketView, MomentumStrategy, SelfView, StrategyState};
use crate::{AccountId, AccountKind};
use std::collections::BTreeMap;

fn account_input() -> DecisionAccountInput {
    DecisionAccountInput::new(
        AccountKind::Inst,
        SelfView {
            cash: crate::Money::from_cents(10_000),
            positions: BTreeMap::new(),
        },
        StrategyState::Momentum(MomentumStrategy::new(5, 0.02, 100).unwrap()),
        None,
        None,
    )
}

fn retail_account_input() -> DecisionAccountInput {
    retail_account_input_with(None, None)
}

fn retail_account_input_with(
    risk: Option<AccountRiskObservation>,
    experience: Option<crate::RetailExperienceState>,
) -> DecisionAccountInput {
    DecisionAccountInput::new(
        AccountKind::Retail,
        SelfView {
            cash: crate::Money::from_cents(10_000),
            positions: BTreeMap::new(),
        },
        StrategyState::Momentum(MomentumStrategy::new(5, 0.02, 100).unwrap()),
        risk,
        experience,
    )
}

fn behavior_market() -> BehaviorMarketObservation {
    BehaviorMarketObservation {
        price_paths: BTreeMap::new(),
        thirty_minute_market: EqualWeightMarketObservation {
            total_stock_count: 0,
            observed_stock_count: 0,
            equal_weight_return: None,
            advance_fraction: None,
            decline_fraction: None,
            unchanged_fraction: None,
        },
    }
}

fn account_risk() -> AccountRiskObservation {
    AccountRiskObservation {
        equity: crate::Money::from_cents(10_000),
        return_from_reference: None,
        drawdown_from_peak: None,
        positions: BTreeMap::new(),
    }
}

fn market(tick: u64, minute: u64) -> MarketView {
    MarketView {
        stocks: BTreeMap::new(),
        tick,
        market_minute: minute,
    }
}

#[test]
fn decision_snapshot_owns_immutable_inputs_in_canonical_account_order() {
    let first = AccountId(1);
    let second = AccountId(2);
    let mut accounts = BTreeMap::new();
    accounts.insert(second, account_input());
    accounts.insert(first, account_input());

    let snapshot = DecisionSnapshot::new(
        7,
        42,
        crate::TradingPhase::Continuous,
        11,
        market(7, 11),
        None,
        vec![first, second],
        accounts,
    )
    .unwrap();

    assert_eq!(snapshot.tick(), 7);
    assert_eq!(snapshot.npc_seed_base(), 42);
    assert_eq!(snapshot.market_minute(), 11);
    assert_eq!(snapshot.due_npc_ids(), &[first, second]);
    assert_eq!(snapshot.account(first).unwrap().kind(), AccountKind::Inst);
    assert_eq!(snapshot.market().tick, 7);
}

#[test]
fn decision_snapshot_rejects_noncanonical_or_missing_due_accounts() {
    let first = AccountId(1);
    let second = AccountId(2);
    let mut accounts = BTreeMap::new();
    accounts.insert(first, account_input());

    assert!(matches!(
        DecisionSnapshot::new(
            7,
            42,
            crate::TradingPhase::Continuous,
            11,
            market(7, 11),
            None,
            vec![second, first],
            accounts.clone(),
        ),
        Err(DecisionSnapshotError::NonCanonicalDueAccounts)
    ));
    assert!(matches!(
        DecisionSnapshot::new(
            7,
            42,
            crate::TradingPhase::Continuous,
            11,
            market(7, 11),
            None,
            vec![second],
            accounts,
        ),
        Err(DecisionSnapshotError::MissingAccount(id)) if id == second
    ));
}

#[test]
fn decision_snapshot_rejects_clock_drift_between_header_and_market_view() {
    assert!(matches!(
        DecisionSnapshot::new(
            7,
            42,
            crate::TradingPhase::Continuous,
            11,
            market(8, 11),
            None,
            Vec::new(),
            BTreeMap::new(),
        ),
        Err(DecisionSnapshotError::MarketClockMismatch { .. })
    ));
}

#[test]
fn decision_snapshot_rejects_incomplete_retail_observation_bundle() {
    let retail = AccountId(1);
    let mut accounts = BTreeMap::new();
    accounts.insert(retail, retail_account_input());

    assert!(matches!(
        DecisionSnapshot::new(
            7,
            42,
            crate::TradingPhase::Continuous,
            11,
            market(7, 11),
            None,
            vec![retail],
            accounts,
        ),
        Err(DecisionSnapshotError::MissingRetailBehaviorMarket(id)) if id == retail
    ));

    let mut missing_risk = BTreeMap::new();
    missing_risk.insert(
        retail,
        retail_account_input_with(
            None,
            Some(crate::RetailExperienceState::without_equity_reference()),
        ),
    );
    assert!(matches!(
        DecisionSnapshot::new(
            7,
            42,
            crate::TradingPhase::Continuous,
            11,
            market(7, 11),
            Some(behavior_market()),
            vec![retail],
            missing_risk,
        ),
        Err(DecisionSnapshotError::MissingRetailAccountRisk(id)) if id == retail
    ));

    let mut missing_experience = BTreeMap::new();
    missing_experience.insert(
        retail,
        retail_account_input_with(Some(account_risk()), None),
    );
    assert!(matches!(
        DecisionSnapshot::new(
            7,
            42,
            crate::TradingPhase::Continuous,
            11,
            market(7, 11),
            Some(behavior_market()),
            vec![retail],
            missing_experience,
        ),
        Err(DecisionSnapshotError::MissingRetailExperience(id)) if id == retail
    ));

    let mut complete = BTreeMap::new();
    complete.insert(
        retail,
        retail_account_input_with(
            Some(account_risk()),
            Some(crate::RetailExperienceState::without_equity_reference()),
        ),
    );
    assert!(DecisionSnapshot::new(
        7,
        42,
        crate::TradingPhase::Continuous,
        11,
        market(7, 11),
        Some(behavior_market()),
        vec![retail],
        complete,
    )
    .is_ok());
}

#[test]
fn decision_snapshot_rejects_player_duplicate_and_market_minute_drift() {
    let player = AccountId(1);
    let mut player_accounts = BTreeMap::new();
    player_accounts.insert(
        player,
        DecisionAccountInput::new(
            AccountKind::Player,
            SelfView {
                cash: crate::Money::from_cents(10_000),
                positions: BTreeMap::new(),
            },
            StrategyState::Momentum(MomentumStrategy::new(5, 0.02, 100).unwrap()),
            None,
            None,
        ),
    );
    assert!(matches!(
        DecisionSnapshot::new(
            7,
            42,
            crate::TradingPhase::Continuous,
            11,
            market(7, 11),
            None,
            vec![player],
            player_accounts,
        ),
        Err(DecisionSnapshotError::PlayerInNpcBatch(id)) if id == player
    ));

    let npc = AccountId(2);
    let mut npc_accounts = BTreeMap::new();
    npc_accounts.insert(npc, account_input());
    assert!(matches!(
        DecisionSnapshot::new(
            7,
            42,
            crate::TradingPhase::Continuous,
            11,
            market(7, 11),
            None,
            vec![npc, npc],
            npc_accounts,
        ),
        Err(DecisionSnapshotError::NonCanonicalDueAccounts)
    ));
    assert!(matches!(
        DecisionSnapshot::new(
            7,
            42,
            crate::TradingPhase::Continuous,
            11,
            market(7, 12),
            None,
            Vec::new(),
            BTreeMap::new(),
        ),
        Err(DecisionSnapshotError::MarketClockMismatch { .. })
    ));
}
