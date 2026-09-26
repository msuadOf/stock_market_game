use super::continuous_cancellation::ContinuousCancellationCause;
use super::*;

fn session_with_resting_npc_order() -> (GameSession, StockCode, AccountId, OrderId) {
    let code = StockCode("600888".to_string());
    let account = AccountId(1);
    let mut session = GameSession::new(npc_working_quote_tests::quote_setup(0), 7).unwrap();
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
            Event::OrderAccepted { id, .. } => Some(*id),
            _ => None,
        })
        .expect("fixture must create a resting order");
    (session, code, account, order_id)
}

#[test]
fn state_only_continuous_cancellation_returns_fact_without_consuming_seq() {
    let (mut session, code, account, order_id) = session_with_resting_npc_order();
    let seq_before = session.seq();

    let fact = session
        .cancel_continuous_order_state_only(
            account,
            code.clone(),
            order_id,
            ContinuousCancellationCause::Voluntary,
        )
        .expect("owned resting order cancels");

    assert_eq!(fact.account, account);
    assert_eq!(fact.code, code);
    assert_eq!(fact.order_id, order_id);
    assert_eq!(fact.side, Side::Buy);
    assert_eq!(fact.remaining_qty, 100);
    assert_eq!(session.seq(), seq_before);
    assert!(session.markets[&code]
        .resting_orders_for(account)
        .is_empty());
    assert!(session.npc_order_lifecycles.is_empty());
}

#[test]
fn state_only_continuous_cancellation_rejects_wrong_owner_without_mutation() {
    let (mut session, code, account, order_id) = session_with_resting_npc_order();
    let before_business = session.business_state_hash().unwrap();
    let before_session = session.session_state_hash().unwrap();
    let before_seq = session.seq();

    let error = session
        .cancel_continuous_order_state_only(
            AccountId(account.0 + 1),
            code.clone(),
            order_id,
            ContinuousCancellationCause::Voluntary,
        )
        .expect_err("another account cannot cancel the order");

    assert!(matches!(
        error,
        super::continuous_cancellation::ContinuousCancellationError::NotOrderOwner
    ));
    assert_eq!(session.business_state_hash().unwrap(), before_business);
    assert_eq!(session.session_state_hash().unwrap(), before_session);
    assert_eq!(session.seq(), before_seq);
    assert_eq!(session.markets[&code].resting_orders_for(account).len(), 1);

    let fact = session
        .cancel_continuous_order_state_only(
            account,
            code,
            order_id,
            ContinuousCancellationCause::Voluntary,
        )
        .expect("the owner can cancel after a rejected forged attempt");
    assert_eq!(fact.order_id, order_id);
}

#[test]
fn state_only_continuous_cancellation_rejects_missing_order_without_mutation() {
    let (mut session, code, account, order_id) = session_with_resting_npc_order();
    let before_business = session.business_state_hash().unwrap();
    let before_session = session.session_state_hash().unwrap();
    let before_seq = session.seq();

    let error = session
        .cancel_continuous_order_state_only(
            account,
            code,
            OrderId(order_id.0 + 1),
            ContinuousCancellationCause::Voluntary,
        )
        .expect_err("unknown order cannot cancel");

    assert!(matches!(
        error,
        super::continuous_cancellation::ContinuousCancellationError::OrderNotFound
    ));
    assert_eq!(session.business_state_hash().unwrap(), before_business);
    assert_eq!(session.session_state_hash().unwrap(), before_session);
    assert_eq!(session.seq(), before_seq);
}

#[test]
fn state_only_continuous_cancellation_rejects_unknown_stock_without_mutation() {
    let (mut session, _code, account, order_id) = session_with_resting_npc_order();
    let before_business = session.business_state_hash().unwrap();
    let before_session = session.session_state_hash().unwrap();
    let before_seq = session.seq();

    let error = session
        .cancel_continuous_order_state_only(
            account,
            StockCode("600999".to_string()),
            order_id,
            ContinuousCancellationCause::Voluntary,
        )
        .expect_err("unknown stock cannot cancel");

    assert!(matches!(
        error,
        super::continuous_cancellation::ContinuousCancellationError::UnknownStock
    ));
    assert_eq!(session.business_state_hash().unwrap(), before_business);
    assert_eq!(session.session_state_hash().unwrap(), before_session);
    assert_eq!(session.seq(), before_seq);
}
