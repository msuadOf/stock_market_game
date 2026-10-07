use super::*;
use crate::{AccountId, Intent, LimitPrice, Money, Order, OrderId, Side, StockCode};

const PLAYER: AccountId = AccountId(0);
const SELLER: AccountId = AccountId(1);

fn install_quote(game: &mut GameSession, code: &StockCode, side: Side, price: i64, quantity: u32) {
    let id = game.state.next_order_id;
    let result = game.state.markets.get_mut(code).unwrap().place(Order {
        id: OrderId(id), side, price: Money::from_cents(price),
        qty: quantity, original_qty: quantity, filled_qty: 0, filled_value: Money::ZERO,
        owner: SELLER, seq: id,
    }).unwrap();
    assert!(result.trades.is_empty());
    game.state.next_order_id += 1;
}

fn trading_session() -> (ProtocolSession, StockCode) {
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
    setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
    setup.ticks_per_day = 12;
    setup.stocks[0].float_shares = 600;
    let mut game = GameSession::new(setup, 81).unwrap();
    let code = game.state.setup.stocks[0].code.clone();
    let cash_before = game.state.accounts[&SELLER].cash();
    assert_eq!(game.state.accounts[&SELLER].positions()[&code].qty(), 600);
    game.state.accounts.get_mut(&SELLER).unwrap().grant_position(code.clone(), 400, Money::from_cents(1000)).unwrap();
    game.state.accounts.get_mut(&PLAYER).unwrap().grant_position(code.clone(), 200, Money::from_cents(1000)).unwrap();
    assert_eq!(game.state.accounts[&SELLER].cash(), cash_before);
    assert_eq!(game.state.accounts.values().map(|account| account.sellable_qty(&code)).sum::<u32>(), 600);
    game.state.npc_attention.get_mut(&SELLER).unwrap().next_attention_candidate_tick = 1000;
    game.state.attention_scheduler = [(1000, SELLER)].into_iter().collect();
    install_quote(&mut game, &code, Side::Sell, 1000, 300);
    install_quote(&mut game, &code, Side::Buy, 999, 100);
    install_quote(&mut game, &code, Side::Buy, 999, 100);
    game.hydrate_or_validate_envelope_ledger().unwrap();
    (ProtocolSession::from_game(game, None), code)
}

fn buy(session: &mut ProtocolSession, code: &StockCode) {
    session.enqueue_player_intent(PLAYER, Intent::PlaceLimit {
        code: code.clone(), side: Side::Buy, price: LimitPrice::Fixed(Money::from_cents(1000)), qty: 100,
    }).unwrap();
}

#[test]
fn personal_fills_survive_real_protocol_day_end_and_continue_receipt_cursor() {
    let (mut session, code) = trading_session();
    assert!(session.save().is_err());
    let mut seller_fills = Vec::new();
    for _ in 0..2 {
        buy(&mut session, &code);
        let (_, evidence) = session.step_frame_with_commit_evidence().unwrap();
        seller_fills.extend(evidence.receipts().iter().filter(|receipt| receipt.envelope.account == SELLER && receipt.kind == crate::session::pipeline::ReceiptKind::Fill).cloned());
    }
    assert_eq!(seller_fills.len(), 2);
    assert_eq!(seller_fills[0].charged.commission, Money::from_cents(500));
    assert_eq!(seller_fills[1].charged.commission, Money::ZERO);
    session.enqueue_player_intent(PLAYER, Intent::PlaceLimit {
        code: code.clone(), side: Side::Sell, price: LimitPrice::Fixed(Money::from_cents(999)), qty: 200,
    }).unwrap();
    session.step_frame().unwrap();
    let complete = session.personal_trade_confirmations(PLAYER);
    let buyer = complete.iter().filter(|confirmation| confirmation.side == Side::Buy).collect::<Vec<_>>();
    let seller = complete.iter().filter(|confirmation| confirmation.side == Side::Sell).collect::<Vec<_>>();
    assert_eq!(buyer.len(), 2);
    assert_eq!(seller.len(), 2);
    assert_eq!(seller[0].actual_fees.commission, Money::from_cents(500));
    assert_eq!(seller[1].actual_fees.commission, Money::ZERO);
    assert_eq!(seller[0].actual_fees.stamp_tax, Money::from_cents(50));
    assert_eq!(seller[1].actual_fees.stamp_tax, Money::from_cents(50));
    let counterparty = session.personal_trade_confirmations(SELLER);
    assert_eq!(counterparty.len(), 4);
    assert_eq!(counterparty[0].actual_fees, seller_fills[0].charged);
    assert_eq!(counterparty[1].actual_fees, seller_fills[1].charged);
    assert_eq!(counterparty.iter().map(|confirmation| confirmation.side).collect::<Vec<_>>(), [Side::Sell, Side::Sell, Side::Buy, Side::Buy]);
    assert!(session.save().is_err());
    while !session.civil_day_ready().unwrap() { session.step_frame().unwrap(); }
    session.end_civil_day_update().unwrap();
    let saved = session.save().unwrap();
    assert_eq!(saved.runtime_state.personal_trade_confirmations[&PLAYER], complete);
    assert_eq!(saved.runtime_state.personal_trade_confirmations[&SELLER], counterparty);
    let mut restored = ProtocolSession::restore(&saved).unwrap();
    assert_eq!(restored.personal_trade_confirmations(PLAYER), complete);
    assert_eq!(restored.personal_trade_confirmations(SELLER), counterparty);
    install_quote(&mut restored.state.game, &code, Side::Sell, 1000, 100);
    restored.state.game.hydrate_or_validate_envelope_ledger().unwrap();
    buy(&mut restored, &code);
    restored.step_frame().unwrap();
    install_quote(&mut restored.state.game, &code, Side::Buy, 999, 100);
    restored.state.game.hydrate_or_validate_envelope_ledger().unwrap();
    restored.enqueue_player_intent(PLAYER, Intent::PlaceLimit {
        code: code.clone(), side: Side::Sell, price: LimitPrice::Fixed(Money::from_cents(999)), qty: 100,
    }).unwrap();
    restored.step_frame().unwrap();
    let continued = restored.personal_trade_confirmations(PLAYER);
    assert_eq!(&continued[..4], complete.as_slice());
    assert_eq!(continued.len(), 6);
    let counterparty_continued = restored.personal_trade_confirmations(SELLER);
    assert_eq!(&counterparty_continued[..4], counterparty.as_slice());
    assert_eq!(counterparty_continued.len(), 6);
    assert!(counterparty_continued[4..].iter().all(|confirmation| confirmation.receipt_id >= saved.runtime_state.next_receipt_base));
    for confirmation in &continued[4..] {
        assert!(confirmation.receipt_id >= saved.runtime_state.next_receipt_base);
        assert_eq!(confirmation.civil_date, restored.civil_date());
        assert_eq!(confirmation.actual_fees.commission, Money::from_cents(500));
    }
    assert_eq!(serde_json::to_value(restored.save().unwrap()).unwrap(), serde_json::to_value(saved).unwrap(), "日内公开save只交付此前已完成的日终候选");
}

#[test]
fn rejected_publication_and_failed_close_do_not_leak_personal_fills() {
    let (mut session, code) = trading_session();
    buy(&mut session, &code);
    session.malformed_frame = true;
    assert!(session.step_frame().is_err());
    assert!(session.personal_trade_confirmations(PLAYER).is_empty());
    assert!(session.personal_trade_confirmations(SELLER).is_empty());
    session.step_frame().unwrap();
    let committed = session.personal_trade_confirmations(PLAYER);
    assert_eq!(committed.len(), 1);
    while !session.civil_day_ready().unwrap() { session.step_frame().unwrap(); }
    session.malformed_civil = true;
    assert!(session.end_civil_day_update().is_err());
    assert_eq!(session.personal_trade_confirmations(PLAYER), committed);
    assert!(session.save().is_err());
    session.end_civil_day_update().unwrap();
    assert_eq!(session.save().unwrap().runtime_state.personal_trade_confirmations[&PLAYER], committed);
}

#[test]
fn personal_history_pages_have_stable_receipt_range_and_shared_checkpoints() {
    let (mut session, code) = trading_session();
    buy(&mut session, &code);
    session.step_frame().unwrap();
    let original = session.personal_trade_confirmations(PLAYER)[0].clone();
    for index in 1..130 {
        let mut confirmation = original.clone();
        confirmation.receipt_id = index;
        session.state.game.state.personal_trade_confirmations.get_mut(&PLAYER).unwrap().push(confirmation);
    }
    let checkpoint = session.checkpoint().unwrap();
    let history = &session.state.game.state.personal_trade_confirmations[&PLAYER];
    let shared = &checkpoint.state.game.state.personal_trade_confirmations[&PLAYER];
    assert!(std::ptr::eq(&history[0], &shared[0]));
    let page = session.personal_trade_confirmations_page(PLAYER, None);
    assert_eq!(page.len(), 100);
    assert_eq!((page[0].receipt_id, page[99].receipt_id), (129, 30));
    let older = session.personal_trade_confirmations_page(PLAYER, Some(30));
    assert_eq!(older.len(), 30);
    assert_eq!((older[0].receipt_id, older[29].receipt_id), (29, 0));
    assert!(session.personal_trade_confirmations_page(PLAYER, Some(0)).is_empty());
    assert_eq!(session.personal_trade_confirmations_page(SELLER, None).len(), 1);
    assert!(session.personal_trade_confirmations_page(AccountId(2), None).is_empty());
}
