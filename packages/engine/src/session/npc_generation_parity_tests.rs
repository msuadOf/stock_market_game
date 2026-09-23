use super::*;
use crate::strategy::{Strategy, StrategyFamily};

struct PinnedNpcStrategy {
    intents: Vec<Intent>,
    reviewed: BTreeSet<StockCode>,
    parent_execution: bool,
}

impl Strategy for PinnedNpcStrategy {
    fn profile(&self) -> StrategyProfile {
        if self.parent_execution {
            StrategyProfile::Institution(crate::strategy::InstitutionStyle::DeepValue)
        } else {
            StrategyProfile::Retail(crate::strategy::RetailStyle::Noise)
        }
    }

    fn strategy_family(&self) -> StrategyFamily {
        if self.parent_execution {
            StrategyFamily::FundamentalValue
        } else {
            StrategyFamily::RetailBehavior
        }
    }

    fn uses_parent_order_execution(&self) -> bool {
        self.parent_execution
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
            intents: self.intents.clone(),
            reviewed_stocks: self.reviewed.clone(),
            position_decision: None,
        }
    }

    fn decide(&mut self, _market: &MarketView, _own: &SelfView, _rng: &mut dyn Rng) -> Vec<Intent> {
        self.intents.clone()
    }
}

fn install_strategy(session: &mut GameSession, account: AccountId, strategy: PinnedNpcStrategy) {
    session.accounts.get_mut(&account).unwrap().strategy = Some(
        crate::account::StoredStrategy::non_authoritative(Box::new(strategy)),
    );
}

fn buy(code: &StockCode, price: i64, qty: u32) -> Intent {
    Intent::PlaceLimit {
        code: code.clone(),
        side: Side::Buy,
        price: Money::from_cents(price),
        qty,
    }
}

#[test]
fn npc_tick_parity_pins_reviewed_retail_cancellation_events_and_state() {
    let code = StockCode("600888".to_owned());
    let retail = AccountId(1);
    let mut session = GameSession::new(npc_working_quote_tests::retail_quote_setup(), 115).unwrap();
    let mut setup_events = Vec::new();
    session.route_intent(retail, buy(&code, 900, 100), &mut setup_events);
    install_strategy(
        &mut session,
        retail,
        PinnedNpcStrategy {
            intents: Vec::new(),
            reviewed: [code.clone()].into(),
            parent_execution: false,
        },
    );
    npc_working_quote_tests::force_attention_candidate(&mut session, retail, 0);

    let events = session.step().expect("healthy step");

    assert!(
        matches!(events.as_slice(), [Event::OrderCanceled { account, code: canceled, remaining_qty: 100, .. }, Event::PriceTick { .. }] if *account == retail && canceled == &code)
    );
    assert!(session.markets[&code].resting_orders_for(retail).is_empty());
    assert!(!session.parent_orders.contains_key(&retail));
    assert!(session.pending_player.is_empty());
    assert!(session.retail_experience[&retail]
        .stocks
        .contains_key(&code));
    assert_eq!(
        session.npc_attention[&retail].next_attention_candidate_tick,
        1
    );
}

#[test]
fn npc_tick_parity_pins_institution_parent_child_and_books() {
    let code = StockCode("600888".to_owned());
    let institution = AccountId(1);
    let mut session = GameSession::new(npc_working_quote_tests::quote_setup(0), 991).unwrap();
    install_strategy(
        &mut session,
        institution,
        PinnedNpcStrategy {
            intents: vec![buy(&code, 1_000, 400)],
            reviewed: BTreeSet::new(),
            parent_execution: true,
        },
    );
    npc_working_quote_tests::force_attention_candidate(&mut session, institution, 0);

    let events = session.step().expect("healthy step");

    assert!(
        matches!(events.as_slice(), [Event::OrderAccepted { account, code: accepted, remaining_qty: 100, .. }, Event::PriceTick { .. }] if *account == institution && accepted == &code)
    );
    let parent = &session.parent_orders[&institution][&code];
    assert_eq!(
        (parent.target_qty, parent.filled_qty, parent.child_qty),
        (400, 0, 100)
    );
    assert_eq!(
        session.markets[&code].resting_orders_for(institution).len(),
        1
    );
    assert!(session.pending_player.is_empty());
}

#[test]
fn npc_tick_parity_pins_multi_account_cap_order_and_player_fifo() {
    let code = StockCode("600888".to_owned());
    let player = AccountId(0);
    let first = AccountId(1);
    let second = AccountId(2);
    let mut setup = npc_working_quote_tests::retail_quote_setup();
    setup.npcs.retail_count = 2;
    let mut session = GameSession::new(setup, 33).unwrap();
    for account in [first, second] {
        session.accounts.get_mut(&account).unwrap().cash = Money::from_cents(1_000_000);
        install_strategy(
            &mut session,
            account,
            PinnedNpcStrategy {
                intents: vec![buy(&code, 1_000, 9_000)],
                reviewed: BTreeSet::new(),
                parent_execution: false,
            },
        );
        npc_working_quote_tests::force_attention_candidate(&mut session, account, 0);
    }
    session
        .enqueue_player_intent(player, buy(&code, 900, 100))
        .unwrap();

    let events = session.step().expect("healthy step");

    let accepted: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            Event::OrderAccepted {
                account,
                remaining_qty,
                ..
            } => Some((*account, *remaining_qty)),
            _ => None,
        })
        .collect();
    assert_eq!(accepted.len(), 3);
    assert_eq!(accepted[0].0, first);
    assert_eq!(accepted[1].0, second);
    assert_eq!(accepted[2], (player, 100));
    assert_eq!(accepted[0].1, 900);
    assert_eq!(accepted[1].1, 900);
    assert!(session.pending_player.is_empty());
    assert_eq!(session.markets[&code].resting_orders_for(first)[0].qty, 900);
    assert_eq!(
        session.markets[&code].resting_orders_for(second)[0].qty,
        900
    );
}
