use super::b1_continuous_transaction::{
    apply_tick_shadow_b1_continuous_transaction, prepare_b1_continuous_tick,
};
use super::b2_auction_transaction::prepare_b2_auction_tick;
use super::*;
use crate::{
    Account, AccountId, AccountKind, GameSession, Intent, Money, Order, OrderId, Side, StockCode,
    TradingPhase,
};

const PLAYER: AccountId = AccountId(0);
const SELLER: AccountId = AccountId(1);
const RESTING_SELL: OrderId = OrderId(1);
const CROSSING_BUY: OrderId = OrderId(2);

#[test]
fn ordinary_tick_skips_commit_evidence_replay_but_diagnostic_tick_captures_it() {
    let (mut ordinary, _) = continuous_trade_session();
    let (mut diagnostic, _) = continuous_trade_session();
    let before = super::commit_evidence::capture_count_for_test();

    ordinary.step().unwrap();
    assert_eq!(super::commit_evidence::capture_count_for_test(), before);

    let (_, evidence) = diagnostic.step_with_commit_evidence().unwrap();
    assert_eq!(super::commit_evidence::capture_count_for_test(), before + 1);
    assert_eq!(evidence.receipts().len(), 2);
    assert_eq!(
        ordinary.business_state_hash().unwrap(),
        diagnostic.business_state_hash().unwrap()
    );
}

#[test]
fn prepared_b1_commit_preserves_actual_receipt_chains_before_p9_rebase() {
    let (mut authority, code) = continuous_trade_session();
    let prepared = prepare_b1_continuous_tick(&mut authority).unwrap();

    assert!(prepared.evidence().p0_receipts().is_empty());
    assert_eq!(prepared.evidence().receipts().len(), 2);
    assert_eq!(prepared.evidence().envelope_chains().len(), 2);
    assert!(prepared
        .evidence()
        .envelope_chains()
        .iter()
        .all(|chain| chain.is_terminal()));
    assert_eq!(
        prepared
            .evidence()
            .receipts()
            .iter()
            .map(|receipt| (receipt.index, receipt.local_key.clone()))
            .collect::<Vec<_>>(),
        prepared
            .output()
            .receipts
            .iter()
            .map(|receipt| (receipt.index, receipt.local_key.clone()))
            .collect::<Vec<_>>()
    );

    let committed = prepared.commit();

    assert_eq!(authority.envelope_ledger.terminal_count(), 0);
    assert!(authority.envelope_ledger.iter().next().is_none());
    assert_eq!(
        committed.commit.evidence.as_ref().unwrap().receipts().len(),
        2
    );
    assert!(committed
        .commit
        .evidence
        .as_ref()
        .unwrap()
        .envelope_chains()
        .iter()
        .all(|chain| chain.is_terminal() && chain.envelope().live() == ResVec::ZERO));
    assert!(committed
        .commit
        .evidence
        .as_ref()
        .unwrap()
        .envelope_chains()
        .iter()
        .any(|chain| {
            chain.envelope().key().stock == code
                && chain.envelope().key().order == CROSSING_BUY
                && chain.receipts().len() == 1
        }));
}

#[test]
fn prepared_b1_commit_exposes_the_applied_p0_receipt_without_reconstructing_it() {
    let (mut authority, code, account, order_id) = expiring_buy_session();
    let cash_before = authority.accounts[&account].cash;

    let committed = prepare_b1_continuous_tick(&mut authority).unwrap().commit();
    let evidence = committed.commit.evidence.as_ref().unwrap();

    assert!(committed.output.receipts.is_empty());
    assert_eq!(evidence.receipts().len(), 1);
    assert_eq!(evidence.p0_receipts().len(), 1);
    let receipt = &evidence.p0_receipts()[0];
    assert_eq!(receipt.index, 0);
    assert_eq!(receipt.local_key.journal(), JournalRank::PreSeal);
    assert_eq!(receipt.local_key.source(), ReceiptSource::P0Expiry(0));
    assert_eq!(receipt.envelope.account, account);
    assert_eq!(receipt.envelope.stock, code);
    assert_eq!(receipt.envelope.order, order_id);
    assert_eq!(receipt.kind, ReceiptKind::Release);
    assert_eq!(receipt.delta.live_after, ResVec::ZERO);
    let chain = evidence
        .envelope_chains()
        .iter()
        .find(|chain| chain.envelope().key() == &receipt.envelope)
        .unwrap();
    assert!(chain.is_terminal());
    assert_eq!(chain.receipts().len(), 1);
    assert_eq!(chain.receipts()[0].index, receipt.index);
    assert_eq!(committed.output.p6.settlement.applied_receipts, 0);
    assert_eq!(authority.accounts[&account].cash, cash_before);
    assert_eq!(authority.next_receipt_base, 1);
    assert_eq!(authority.envelope_ledger.next_receipt_index(), 1);
    assert_eq!(
        authority.retail_projection_seen.authoritative_identities(),
        vec![(0, receipt.local_key.clone())]
    );
    authority
        .save()
        .expect("a committed P0 receipt must form a saveable v2 seen prefix");
}

#[test]
fn prepared_commit_rejects_missing_receipt_values_without_touching_authority() {
    let (mut authority, _) = continuous_trade_session();
    let mut plan = plan_tick(PhaseInput {
        session: &authority,
    })
    .unwrap();
    apply_tick_shadow_b1_continuous_transaction(&mut plan).unwrap();
    assert!(!plan.applied_receipts.is_empty());
    plan.applied_receipts.clear();
    let business_before = authority.business_state_hash().unwrap();
    let session_before = authority.session_state_hash().unwrap();

    let error = super::p9_candidate_commit::prepare_tick_shadow_plan_commit(&mut authority, plan)
        .err()
        .expect("P9 preparation must reject a receipt-key journal without receipt values");

    assert!(matches!(
        error,
        StepFatal::InvariantViolation { location, description }
            if location == "pipeline::p9_candidate_commit"
                && description.contains("different lengths")
    ));
    assert_eq!(authority.business_state_hash().unwrap(), business_before);
    assert_eq!(authority.session_state_hash().unwrap(), session_before);
}

#[test]
fn ordinary_p9_rejects_receipt_journal_missing_applied_values() {
    let (mut authority, _) = continuous_trade_session();
    let mut plan = plan_tick(PhaseInput {
        session: &authority,
    })
    .unwrap();
    apply_tick_shadow_b1_continuous_transaction(&mut plan).unwrap();
    plan.applied_receipts.clear();
    let before = authority.business_state_hash().unwrap();

    let error = super::p9_candidate_commit::prepare_tick_shadow_plan_commit_with_evidence(
        &mut authority,
        plan,
        false,
    )
    .err()
    .expect("ordinary P9 must reject a journal without applied receipt values");

    assert!(matches!(error, StepFatal::InvariantViolation { .. }));
    assert_eq!(authority.business_state_hash().unwrap(), before);
}

#[test]
fn prepared_commit_rejects_tampered_receipt_values_without_touching_authority() {
    let (mut authority, _) = continuous_trade_session();
    let mut plan = plan_tick(PhaseInput {
        session: &authority,
    })
    .unwrap();
    apply_tick_shadow_b1_continuous_transaction(&mut plan).unwrap();
    let receipt = plan.applied_receipts.first_mut().unwrap();
    receipt.deliver_qty = receipt.deliver_qty.checked_add(1).unwrap();
    let business_before = authority.business_state_hash().unwrap();
    let session_before = authority.session_state_hash().unwrap();

    let error = super::p9_candidate_commit::prepare_tick_shadow_plan_commit(&mut authority, plan)
        .err()
        .expect("P9 preparation must reject receipt values that do not replay to the ledger");

    assert!(matches!(
        error,
        StepFatal::InvariantViolation { location, .. }
            if location == "pipeline::commit_evidence"
    ));
    assert_eq!(authority.business_state_hash().unwrap(), business_before);
    assert_eq!(authority.session_state_hash().unwrap(), session_before);
}

#[test]
fn prepared_b2_commit_reports_per_stock_finalizer_executions_from_the_real_tail() {
    let mut setup = crate::session::npc_working_quote_tests::two_stock_quote_setup();
    setup.npcs.inst_count = 0;
    setup.closing_auction_ticks = 10;
    let mut authority = GameSession::new(setup, 73).unwrap();
    authority.tick = 99;
    authority.pending_npc = Some(crate::session::PendingNpcBatch {
        dependencies: Vec::new(),
        observed_tick: authority.tick,
        observed_accounts: Vec::new(),
        intents: Vec::new(),
    });
    assert_eq!(authority.phase(), TradingPhase::ClosingAuction);

    let prepared = prepare_b2_auction_tick(&mut authority).unwrap();
    let finalizers = prepared.evidence().b2_finalizers().to_vec();

    assert_eq!(finalizers.len(), 2);
    assert_eq!(
        finalizers
            .iter()
            .map(|execution| execution.stock().clone())
            .collect::<Vec<_>>(),
        vec![
            StockCode("600888".to_owned()),
            StockCode("600889".to_owned())
        ]
    );
    assert!(finalizers.iter().all(|execution| {
        execution.auction_tail_passes() == 1
            && execution.auction_completion_passes() == 1
            && execution.day_end_passes() == 1
    }));

    let committed = prepared.commit();
    assert_eq!(
        committed.commit.evidence.as_ref().unwrap().b2_finalizers(),
        finalizers
    );
}

#[test]
fn prepared_b1_commit_keeps_p0_and_sealed_receipts_in_one_global_chain() {
    let mut authority = mixed_p0_and_sealed_receipt_session();

    let committed = prepare_b1_continuous_tick(&mut authority).unwrap().commit();
    let evidence = committed.commit.evidence.as_ref().unwrap();

    assert_eq!(evidence.receipts().len(), 3);
    assert_eq!(evidence.p0_receipts().len(), 1);
    assert_eq!(
        evidence
            .receipts()
            .iter()
            .map(|receipt| (receipt.index, receipt.local_key.journal()))
            .collect::<Vec<_>>(),
        vec![
            (0, JournalRank::PreSeal),
            (1, JournalRank::SealedBatch),
            (2, JournalRank::SealedBatch),
        ]
    );
    assert_eq!(evidence.next_receipt_index(), 3);
    assert_eq!(committed.output.p6.settlement.applied_receipts, 2);
    assert_eq!(authority.next_receipt_base, 3);
    assert_eq!(
        authority
            .retail_projection_seen
            .authoritative_identities()
            .iter()
            .map(|(index, _)| *index)
            .collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    authority
        .save()
        .expect("mixed PreSeal and SealedBatch receipts must be saveable after commit");
}

fn continuous_trade_session() -> (GameSession, StockCode) {
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
    setup.npcs.inst_count = 0;
    let mut session = GameSession::new(setup, 42).unwrap();
    let code = session.markets.keys().next().unwrap().clone();
    let mut seller = Account::new(SELLER, AccountKind::Player, Money::ZERO);
    seller
        .grant_position(code.clone(), 100, Money::from_cents(100_000))
        .unwrap();
    assert!(session.accounts.insert(SELLER, seller).is_none());
    let placed = session
        .markets
        .get_mut(&code)
        .unwrap()
        .place(Order {
            id: RESTING_SELL,
            side: Side::Sell,
            price: Money::from_cents(1_000),
            qty: 100,
            original_qty: 100,
            filled_qty: 0,
            filled_value: Money::ZERO,
            owner: SELLER,
            seq: 0,
        })
        .unwrap();
    assert!(placed.trades.is_empty());
    assert!(placed.resting.is_some());
    session.next_order_id = CROSSING_BUY.0;
    session.hydrate_or_validate_envelope_ledger().unwrap();
    session
        .enqueue_player_intent(
            PLAYER,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(1_000),
                qty: 100,
            },
        )
        .unwrap();
    (session, code)
}

fn expiring_buy_session() -> (GameSession, StockCode, AccountId, OrderId) {
    let code = StockCode("600888".to_owned());
    let account = AccountId(1);
    let mut session =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 51).unwrap();
    session.accounts.get_mut(&account).unwrap().strategy = None;
    let mut events = Vec::new();
    session.seed_order_for_test(
        account,
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: Money::from_cents(990),
            qty: 100,
        },
        &mut events,
    );
    let order_id = events
        .iter()
        .find_map(|event| match event {
            crate::Event::OrderAccepted { id, .. } => Some(*id),
            _ => None,
        })
        .unwrap();
    session.npc_order_lifecycles[0].expires_market_minute = session.current_market_minute();
    (session, code, account, order_id)
}

fn mixed_p0_and_sealed_receipt_session() -> GameSession {
    let (mut session, code, seller, _) = expiring_buy_session();
    let mut strategy_source =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 51).unwrap();
    session.accounts.get_mut(&seller).unwrap().strategy = strategy_source
        .accounts
        .get_mut(&seller)
        .unwrap()
        .strategy
        .take();
    session
        .accounts
        .get_mut(&seller)
        .unwrap()
        .grant_position(code.clone(), 100, Money::from_cents(100_000))
        .unwrap();
    let mut setup_events = Vec::new();
    session.seed_order_for_test(
        seller,
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Sell,
            price: Money::from_cents(1_000),
            qty: 100,
        },
        &mut setup_events,
    );
    assert!(setup_events.iter().any(
        |event| matches!(event, crate::Event::OrderAccepted { account, .. } if *account == seller)
    ));
    session
        .enqueue_player_intent(
            PLAYER,
            Intent::PlaceLimit {
                code,
                side: Side::Buy,
                price: Money::from_cents(1_000),
                qty: 100,
            },
        )
        .unwrap();
    session
}
