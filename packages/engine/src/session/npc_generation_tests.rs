use super::*;
use crate::strategy::{Strategy, StrategyFamily};

struct FixedIntentsStrategy(Vec<Intent>);

struct ReviewedEmptyStrategy(StockCode);

impl Strategy for FixedIntentsStrategy {
    fn profile(&self) -> StrategyProfile {
        StrategyProfile::Retail(crate::strategy::RetailStyle::Noise)
    }

    fn strategy_family(&self) -> StrategyFamily {
        StrategyFamily::RetailBehavior
    }

    fn decide(&mut self, _market: &MarketView, _own: &SelfView, _rng: &mut dyn Rng) -> Vec<Intent> {
        self.0.clone()
    }
}

impl Strategy for ReviewedEmptyStrategy {
    fn profile(&self) -> StrategyProfile {
        StrategyProfile::Retail(crate::strategy::RetailStyle::Noise)
    }

    fn strategy_family(&self) -> StrategyFamily {
        StrategyFamily::RetailBehavior
    }

    fn decide_with_behavior(
        &mut self,
        _market: &MarketView,
        _own: &SelfView,
        _behavior_market: Option<&BehaviorMarketObservation>,
        _account_risk: Option<&AccountRiskObservation>,
        _rng: &mut dyn Rng,
    ) -> crate::strategy::StrategyDecision {
        crate::strategy::StrategyDecision {
            intents: Vec::new(),
            reviewed_stocks: [self.0.clone()].into(),
            position_decision: None,
        }
    }

    fn decide(&mut self, _market: &MarketView, _own: &SelfView, _rng: &mut dyn Rng) -> Vec<Intent> {
        Vec::new()
    }
}

#[test]
fn npc_generator_returns_empty_batch_without_due_npcs_and_preserves_player_queue() {
    let player = AccountId(0);
    let code = StockCode("600888".to_owned());
    let mut session = GameSession::new(npc_working_quote_tests::quote_setup(0), 1).unwrap();
    let queued = Intent::Cancel {
        code: code.clone(),
        id: OrderId(7),
    };
    session.pending_player.push((player, queued.clone()));
    session.attention_queue.clear();
    let attention_before = session.npc_attention.clone();
    let mut events = Vec::new();

    let batch = session.generate_npc_decision_batch(&mut events);

    assert!(batch.accepted_due_npc_ids.is_empty());
    assert!(batch.intents.is_empty());
    assert_eq!(session.npc_attention, attention_before);
    assert_eq!(session.pending_player.len(), 1);
    assert!(matches!(
        &session.pending_player[0],
        (account, Intent::Cancel { code: queued_code, id })
            if *account == player && queued_code == &code && *id == OrderId(7)
    ));
    assert!(events.is_empty());
}

#[test]
fn npc_generator_reschedules_one_due_npc_and_is_deterministic() {
    let account = AccountId(1);
    let mut first = GameSession::new(npc_working_quote_tests::quote_setup(0), 2).unwrap();
    let mut second = GameSession::new(npc_working_quote_tests::quote_setup(0), 2).unwrap();
    npc_working_quote_tests::force_attention_candidate(&mut first, account, 0);
    npc_working_quote_tests::force_attention_candidate(&mut second, account, 0);
    let attention_before = first.npc_attention[&account].clone();
    let mut first_events = Vec::new();
    let mut second_events = Vec::new();

    let first_batch = first.generate_npc_decision_batch(&mut first_events);
    let second_batch = second.generate_npc_decision_batch(&mut second_events);

    assert_eq!(first_batch.accepted_due_npc_ids, vec![account]);
    assert_eq!(first.npc_attention, second.npc_attention);
    assert_ne!(first.npc_attention[&account], attention_before);
    assert_eq!(
        serde_json::to_vec(&first_batch.intents).unwrap(),
        serde_json::to_vec(&second_batch.intents).unwrap()
    );
    assert_eq!(first_events, second_events);
}

#[test]
fn npc_generator_sorts_accounts_and_preserves_local_intent_order() {
    let code = StockCode("600888".to_owned());
    let mut setup = npc_working_quote_tests::retail_quote_setup();
    setup.npcs.retail_count = 2;
    let mut session = GameSession::new(setup, 3).unwrap();
    let first = AccountId(1);
    let second = AccountId(2);
    session.accounts.get_mut(&first).unwrap().strategy = Some(
        crate::account::StoredStrategy::non_authoritative(Box::new(FixedIntentsStrategy(vec![
            Intent::Cancel {
                code: code.clone(),
                id: OrderId(11),
            },
            Intent::Cancel {
                code: code.clone(),
                id: OrderId(12),
            },
        ]))),
    );
    session.accounts.get_mut(&second).unwrap().strategy =
        Some(crate::account::StoredStrategy::non_authoritative(Box::new(
            FixedIntentsStrategy(vec![Intent::Cancel {
                code,
                id: OrderId(13),
            }]),
        )));
    npc_working_quote_tests::force_attention_candidate(&mut session, second, 0);
    npc_working_quote_tests::force_attention_candidate(&mut session, first, 0);
    let mut events = Vec::new();

    let batch = session.generate_npc_decision_batch(&mut events);

    assert_eq!(batch.accepted_due_npc_ids, vec![first, second]);
    assert_eq!(batch.intents.len(), 3);
    assert!(
        matches!(batch.intents[0], (account, Intent::Cancel { id: OrderId(11), .. }) if account == first)
    );
    assert!(
        matches!(batch.intents[1], (account, Intent::Cancel { id: OrderId(12), .. }) if account == first)
    );
    assert!(
        matches!(batch.intents[2], (account, Intent::Cancel { id: OrderId(13), .. }) if account == second)
    );
}

#[test]
fn npc_generator_reconciles_a_reviewed_retail_quote_once() {
    let code = StockCode("600888".to_owned());
    let account = AccountId(1);
    let mut session = GameSession::new(npc_working_quote_tests::retail_quote_setup(), 4).unwrap();
    let mut initial_events = Vec::new();
    session.route_intent(
        account,
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: Money::from_cents(900),
            qty: 100,
        },
        &mut initial_events,
    );
    session.accounts.get_mut(&account).unwrap().strategy =
        Some(crate::account::StoredStrategy::non_authoritative(Box::new(
            ReviewedEmptyStrategy(code.clone()),
        )));
    npc_working_quote_tests::force_attention_candidate(&mut session, account, 0);
    let mut events = Vec::new();

    let batch = session.generate_npc_decision_batch(&mut events);

    assert!(batch.intents.is_empty());
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::OrderCanceled { account: owner, code: canceled, .. } if *owner == account && canceled == &code))
            .count(),
        1
    );
    assert!(session.markets[&code]
        .resting_orders_for(account)
        .is_empty());
}

#[test]
fn npc_generator_leaves_source_authority_unchanged_when_run_on_shadow_clone() {
    let account = AccountId(1);
    let source = GameSession::new(npc_working_quote_tests::quote_setup(0), 5).unwrap();
    let source_hash = source.business_state_hash().unwrap();
    let mut clone = source.clone_for_tick_shadow().unwrap();
    npc_working_quote_tests::force_attention_candidate(&mut clone, account, 0);
    let mut events = Vec::new();

    let batch = clone.generate_npc_decision_batch(&mut events);

    assert_eq!(batch.accepted_due_npc_ids, vec![account]);
    assert_eq!(source.business_state_hash().unwrap(), source_hash);
}
