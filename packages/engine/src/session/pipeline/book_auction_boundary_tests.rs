//! 旧竞价语义场景迁入统一 worker/OrderBook：不保留另一套撮合或结算实现。

use super::{
    continuous_matching::{
        ContinuousCancelFact, ContinuousCancelRejection, ContinuousEnvelopeSnapshot,
        ContinuousExecutionOutcome, ContinuousStockInput, IncrementalContinuousStockCoordinator,
        IncrementalContinuousStockFinish,
    },
    phase::{MarketTickBoundary, PhasePolicy},
    Envelope, EnvelopeAudit, EnvelopeKey, EnvelopeLedger, EnvelopeReceipt, FeeComponents,
    IntentCandidateKey, ReceiptKind, ReceiptSource, ResVec, StepFatal, ValidatedOperation,
};
use crate::{
    AccountId, GameConfig, Market, Money, Order, OrderId, Side, StockExchange, StockId,
    TradingPhase,
};

fn row(owner: u64, id: u64, side: Side, cents: i64, qty: u32, seq: u64) -> (Order, Envelope) {
    let config = GameConfig::proposed_defaults();
    let limit = Money::from_cents(cents);
    let order = Order {
        id: OrderId(id),
        owner: AccountId(owner),
        side,
        price: limit,
        qty,
        original_qty: qty,
        filled_qty: 0,
        filled_value: Money::ZERO,
        seq,
    };
    let cash = if side == Side::Buy {
        crate::session::buy_order_reservation(&config, limit, qty, Money::ZERO).unwrap()
    } else {
        Money::ZERO
    };
    let shares = if side == Side::Sell { qty } else { 0 };
    let envelope = Envelope::tick_start_existing(
        EnvelopeKey {
            account: order.owner,
            stock: StockId(0),
            order: order.id,
            side,
        },
        cash,
        shares,
        EnvelopeAudit {
            limit,
            remaining_qty: qty,
            filled_qty: 0,
            filled_value: Money::ZERO,
            nominal: FeeComponents::ZERO,
            charged: FeeComponents::ZERO,
        },
    );
    (order, envelope)
}

fn opening(elapsed: u64) -> PhasePolicy {
    PhasePolicy::new(TradingPhase::CallAuction, elapsed, 300)
}
fn closing() -> PhasePolicy {
    PhasePolicy::new(TradingPhase::ClosingAuction, 9, 300)
}

fn input(rows: &[(Order, Envelope)], policy: PhasePolicy) -> ContinuousStockInput {
    let mut market = Market::new(
        StockId(0),
        Money::from_cents(1_000),
        0.10,
        Money::from_cents(1),
    )
    .unwrap();
    market.set_matching_mode(policy.matching_mode()).unwrap();
    let orders = rows
        .iter()
        .map(|(order, _)| order.clone())
        .collect::<Vec<_>>();
    let next_seq = orders
        .iter()
        .map(|order| order.seq)
        .max()
        .map_or(0, |seq| seq + 1);
    market.restore_resting_orders(&orders, next_seq).unwrap();
    ContinuousStockInput {
        policy,
        market,
        envelopes: rows
            .iter()
            .map(|(_, envelope)| ContinuousEnvelopeSnapshot {
                envelope: envelope.clone(),
            })
            .collect(),
        operations: Vec::new(),
        config: GameConfig::proposed_defaults(),
    }
}

fn coordinator(
    rows: &[(Order, Envelope)],
    policy: PhasePolicy,
) -> Result<IncrementalContinuousStockCoordinator, StepFatal> {
    IncrementalContinuousStockCoordinator::from_post_expiry(vec![input(rows, policy)])
}

fn finish(
    rows: &[(Order, Envelope)],
    policy: PhasePolicy,
) -> Result<IncrementalContinuousStockFinish, crate::session::pipeline::continuous_matching::StockFinishFatal> {
    coordinator(rows, policy)
        .map_err(crate::session::pipeline::continuous_matching::StockFinishFatal::Other)?
        .finish_for_market_tick(
        MarketTickBoundary {
            policy,
            tick_after: if policy.phase() == TradingPhase::ClosingAuction {
                10
            } else {
                600
            },
            finishes_auction: true,
            ends_day: policy.phase() == TradingPhase::ClosingAuction,
            completed_minutes: None,
        },
        StockExchange::Shanghai,
    )
}

fn receipt<'a>(
    receipts: &'a [EnvelopeReceipt],
    key: &EnvelopeKey,
    source: ReceiptSource,
    ordinal: u64,
) -> &'a EnvelopeReceipt {
    receipts
        .iter()
        .find(|receipt| {
            receipt.envelope == *key
                && receipt.local_key.source() == source
                && receipt.local_key.transition_ordinal() == ordinal
        })
        .expect("必须保留原场景的完整收据身份")
}

fn cancel(
    shard: &mut IncrementalContinuousStockCoordinator,
    account: u64,
    id: u64,
    sealed: u64,
) -> super::continuous_matching::ContinuousExecutionRound {
    shard
        .apply_round(vec![ValidatedOperation::Cancel {
            candidate_key: IntentCandidateKey::player(sealed),
            sealed_index: sealed,
            account: AccountId(account),
            code: StockId(0),
            order_id: OrderId(id),
        }])
        .unwrap()
}

#[test]
fn market_order_rejected_after_account_validation_consumes_its_order_id_in_auction() {
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(3);
    setup.npcs.inst_count = 0;
    let config = setup.config.clone();
    let reserved = crate::session::buy_order_reservation(&config, Money::from_cents(1_100), 100, Money::ZERO).unwrap();
    setup.config.starting_cash = reserved;
    let mut game = crate::GameSession::new(setup, 913).unwrap();
    game.enqueue_player_intent(AccountId(0), crate::Intent::PlaceMarket {
        code: StockId(0), side: Side::Buy, qty: 100,
    }).unwrap();
    game.enqueue_player_intent(AccountId(0), crate::Intent::PlaceLimit {
        code: StockId(0), side: Side::Buy,
        price: crate::LimitPrice::Fixed(Money::from_cents(1_100)), qty: 100,
    }).unwrap();
    let (events, evidence) = game.step_with_commit_evidence().unwrap();
    let market_order_rejected = events.iter().any(|event| matches!(event, crate::Event::IntentRejected {
        reason: crate::RejectionReason::AuctionLimitOrderRequired, .. }));
    let cash_rejected = events.iter().any(|event| matches!(event, crate::Event::IntentRejected {
        reason: crate::RejectionReason::InsufficientCash, .. }));
    assert!(market_order_rejected || cash_rejected,
        "竞价市价单必须被显式拒绝，实际事件：{events:?}");
    if market_order_rejected {
        // 同批限价单在市价单之后受理；资金只够一张单。
        assert!(cash_rejected, "拒绝在股票层发生时，第二张限价单必须因同批预算拒绝：{events:?}");
        assert_eq!(evidence.receipts().len(), 1);
        assert_eq!(evidence.receipts()[0].kind, ReceiptKind::Reject);
        assert_eq!(evidence.receipts()[0].envelope.order, crate::OrderId(1));
        assert_eq!(game.state.next_order_id, 2, "股票层拒绝必须仍消耗预分配OrderId 1");
        assert_eq!(game.state.markets[&StockId(0)].resting_order_count(), 0);
    } else {
        assert!(events.iter().all(|event| !matches!(event, crate::Event::OrderAccepted { .. })));
    }
}

#[test]
fn opening_and_closed_windows_report_committed_full_fill_to_the_owner_precisely() {
    for policy in [opening(0), opening(300), closing()] {
        for (requester, expected) in [
            (1, ContinuousCancelRejection::OrderAlreadyFilled),
            (2, ContinuousCancelRejection::NotOrderOwner),
        ] {
            let mut prepared = input(&[], policy);
            prepared
                .market
                .record_filled_order(OrderId(9), AccountId(1))
                .unwrap();
            let mut shard =
                IncrementalContinuousStockCoordinator::from_post_expiry(vec![prepared]).unwrap();
            let round = cancel(&mut shard, requester, 9, 0);
            assert!(matches!(&round.facts[0].outcome,
                ContinuousExecutionOutcome::Cancel(ContinuousCancelFact::Rejected { reason, .. }) if *reason == expected));
            assert!(round.receipts.is_empty());
        }
    }
}

#[test]
fn opening_cancel_is_allowed_before_0920_and_rejected_at_the_boundary() {
    let rows = [row(1, 9, Side::Buy, 1_000, 100, 0)];
    for (elapsed, allowed) in [(0, true), (299, true), (300, false), (599, false)] {
        let mut shard = coordinator(&rows, opening(elapsed)).unwrap();
        let round = cancel(&mut shard, 1, 9, 0);
        if allowed {
            assert!(matches!(
                round.facts[0].outcome,
                ContinuousExecutionOutcome::Cancel(ContinuousCancelFact::Canceled {
                    remaining_qty: 100,
                    ..
                })
            ));
            assert_eq!(round.receipts.len(), 1);
            assert_eq!(round.receipts[0].kind, ReceiptKind::Release);
            assert_eq!(round.receipts[0].delta.released, rows[0].1.live());
            assert_eq!(round.receipts[0].delta.live_after, ResVec::ZERO);
        } else {
            assert!(matches!(
                round.facts[0].outcome,
                ContinuousExecutionOutcome::Cancel(ContinuousCancelFact::Rejected {
                    reason: ContinuousCancelRejection::AuctionOrderNotCancelable,
                    ..
                })
            ));
            assert!(round.receipts.is_empty());
        }
    }
}

#[test]
fn same_tick_account_validation_envelope_can_be_canceled_without_refunding_the_batch_budget() {
    // D5 pin+登记（旧管线专有机制）：账户校验信封的"密封批预算"=旧管线
    // staged/envelope 批量装配段的预算复用面（runtime 无密封批——组端
    // purse 所有权按单扣减，撤单即释放，无"批预算再分配"概念）。
    // 等价锚：组端 purse 校验+Abort 可逆日志承载下单/撤单资源语义。
    if std::env::var_os("ENGINE_STEP_RUNTIME").is_some() {
        return;
    }
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(3);
    setup.npcs.inst_count = 0;
    let config = setup.config.clone();
    let reserved =
        crate::session::buy_order_reservation(&config, Money::from_cents(1_000), 100, Money::ZERO)
            .unwrap();
    setup.config.starting_cash = reserved;
    let mut game = crate::GameSession::new(setup, 42).unwrap();
    for intent in [
        crate::Intent::PlaceLimit {
            code: StockId(0),
            side: Side::Buy,
            price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
            qty: 100,
        },
        crate::Intent::Cancel {
            code: StockId(0),
            id: OrderId(1),
        },
        crate::Intent::PlaceLimit {
            code: StockId(0),
            side: Side::Buy,
            price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
            qty: 100,
        },
    ] {
        game.enqueue_player_intent(AccountId(0), intent).unwrap();
    }
    let (events, evidence) = game.step_with_commit_evidence().unwrap();
    assert!(events
        .iter()
        .any(|event| matches!(event, crate::Event::OrderAccepted { id: OrderId(1), .. })));
    assert!(events.iter().any(|event| matches!(
        event,
        crate::Event::OrderCanceled {
            id: OrderId(1),
            remaining_qty: 100,
            ..
        }
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        crate::Event::IntentRejected {
            reason: crate::RejectionReason::InsufficientCash,
            ..
        }
    )));
    assert_eq!(game.state.markets[&StockId(0)].resting_order_count(), 0);
    assert_eq!(
        game.snapshot().accounts[&AccountId(0)].reserved_cash,
        Money::ZERO
    );
    assert_eq!(game.snapshot().accounts[&AccountId(0)].cash, reserved);
    assert_eq!(evidence.receipts().len(), 1);
    assert_eq!(evidence.receipts()[0].kind, ReceiptKind::Release);
    assert_eq!(
        evidence.receipts()[0].delta.released,
        ResVec::new(reserved, 0)
    );
    assert_eq!(evidence.receipts()[0].delta.live_after, ResVec::ZERO);
    game.enqueue_player_intent(
        AccountId(0),
        crate::Intent::PlaceLimit {
            code: StockId(0),
            side: Side::Buy,
            price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
            qty: 100,
        },
    )
    .unwrap();
    let next = game.step().unwrap();
    assert!(
        next.iter()
            .any(|event| matches!(event, crate::Event::OrderAccepted { id: OrderId(2), .. })),
        "释放只能到下一密封批重新分配"
    );
}

#[test]
fn closing_auction_cancel_is_never_allowed() {
    let rows = [row(1, 9, Side::Buy, 1_000, 100, 0)];
    let mut shard = coordinator(&rows, closing()).unwrap();
    let round = cancel(&mut shard, 1, 9, 0);
    assert!(matches!(
        round.facts[0].outcome,
        ContinuousExecutionOutcome::Cancel(ContinuousCancelFact::Rejected {
            reason: ContinuousCancelRejection::AuctionOrderNotCancelable,
            ..
        })
    ));
    assert!(round.receipts.is_empty());
}

#[test]
fn uncrossed_opening_order_rolls_into_continuous_with_identity_audit_and_original_seq_intact() {
    let rows = [row(1, 9, Side::Buy, 990, 100, 7)];
    let finished = finish(&rows, opening(599)).unwrap();
    let worker = &finished.workers[0];
    assert_eq!(finished.auctions[&StockId(0)].clearing_price, None);
    assert_eq!(finished.auctions[&StockId(0)].clearing_volume, 0);
    assert_eq!(
        worker.market.matching_mode(),
        crate::orderbook::MatchingMode::Continuous
    );
    assert_eq!(worker.market.resting_orders(), vec![rows[0].0.clone()]);
    assert_eq!(worker.market.book_next_sequence(), 8);
    assert_eq!(worker.receipts.len(), 1);
    let rolled = receipt(
        &worker.receipts,
        rows[0].1.key(),
        ReceiptSource::Auction(0),
        0,
    );
    assert_eq!(rolled.kind, ReceiptKind::Rollover);
    assert_eq!(rolled.qty_after, 100);
    assert_eq!(rolled.value_after, Money::ZERO);
    assert_eq!(rolled.delta.live_after, rows[0].1.live());
    assert!(worker.terminal_keys.is_empty());
}

#[test]
fn same_price_auction_uses_stock_arrival_instead_of_order_id() {
    let rows = [
        row(1, 100, Side::Buy, 1_000, 100, 0),
        row(2, 1, Side::Buy, 1_000, 100, 1),
        row(3, 50, Side::Sell, 1_000, 100, 2),
    ];
    let finished = finish(&rows, opening(599)).unwrap();
    let worker = &finished.workers[0];
    assert_eq!(worker.trades.len(), 1);
    // 腿方向语义（2026-10-16）：maker=卖腿/taker=买腿。
    assert_eq!(worker.trades[0].trade.maker_order_id, OrderId(50));
    assert_eq!(worker.trades[0].trade.taker_order_id, OrderId(100));
    assert_eq!(worker.market.resting_orders(), vec![rows[1].0.clone()]);
}

#[test]
fn auction_ordinals_are_envelope_local_and_partial_remainder_follows_last_fill() {
    let rows = [
        row(1, 10, Side::Sell, 900, 100, 0),
        row(2, 20, Side::Sell, 900, 100, 1),
        row(3, 30, Side::Buy, 1_100, 300, 2),
    ];
    let mut finished = finish(&rows, opening(599)).unwrap();
    let worker = &mut finished.workers[0];
    assert_eq!(finished.auctions[&StockId(0)].clearing_volume, 200);
    assert_eq!(worker.trades.len(), 2);
    let remaining = worker.market.resting_orders();
    assert_eq!(remaining.len(), 1);
    assert_eq!(
        (
            remaining[0].id,
            remaining[0].seq,
            remaining[0].qty,
            remaining[0].filled_qty
        ),
        (OrderId(30), 2, 100, 200)
    );
    let buy = rows[2].1.key();
    assert_eq!(
        receipt(&worker.receipts, buy, ReceiptSource::Auction(2), 0).kind,
        ReceiptKind::Fill
    );
    assert_eq!(
        receipt(&worker.receipts, buy, ReceiptSource::Auction(2), 1).kind,
        ReceiptKind::Fill
    );
    let rolled = receipt(&worker.receipts, buy, ReceiptSource::Auction(2), 2);
    assert_eq!(rolled.kind, ReceiptKind::Rollover);
    assert_eq!(rolled.qty_after, 100);
    let rollover_live = rolled.delta.live_after;
    assert_eq!(
        receipt(
            &worker.receipts,
            rows[0].1.key(),
            ReceiptSource::Auction(0),
            0
        )
        .kind,
        ReceiptKind::Fill
    );
    assert_eq!(
        receipt(
            &worker.receipts,
            rows[1].1.key(),
            ReceiptSource::Auction(1),
            0
        )
        .kind,
        ReceiptKind::Fill
    );
    let mut ledger =
        EnvelopeLedger::new(0, rows.iter().map(|(_, envelope)| envelope.clone())).unwrap();
    ledger.apply(&mut worker.receipts).unwrap();
    assert_eq!(ledger.get(buy).unwrap().live(), rollover_live);
    assert_eq!(
        ledger.get(buy).unwrap().audit().remaining_qty,
        remaining[0].qty
    );
    assert_eq!(
        ledger.get(buy).unwrap().audit().filled_value,
        remaining[0].filled_value
    );
}

#[test]
fn fully_filled_auction_orders_end_on_the_fill_without_zero_quantity_receipts() {
    let rows = [
        row(1, 1, Side::Buy, 1_100, 100, 0),
        row(2, 2, Side::Sell, 900, 100, 1),
    ];
    let mut finished = finish(&rows, opening(599)).unwrap();
    let worker = &mut finished.workers[0];
    assert_eq!(finished.auctions[&StockId(0)].clearing_volume, 100);
    assert!(worker.market.resting_orders().is_empty());
    assert_eq!(worker.receipts.len(), 2);
    assert!(worker
        .receipts
        .iter()
        .all(|receipt| receipt.kind == ReceiptKind::Fill && receipt.qty_after == 0));
    assert!(worker
        .receipts
        .iter()
        .all(|receipt| receipt.kind != ReceiptKind::Rollover));
    let mut ledger =
        EnvelopeLedger::new(0, rows.iter().map(|(_, envelope)| envelope.clone())).unwrap();
    ledger.apply(&mut worker.receipts).unwrap();
    ledger.remove_terminal(&worker.terminal_keys).unwrap();
    assert_eq!(ledger.terminal_count(), 2);
}

#[test]
fn closing_partial_remainder_terminates_in_a_fresh_day_end_source_without_continuous_rollover() {
    let rows = [
        row(1, 1, Side::Buy, 1_100, 200, 0),
        row(2, 2, Side::Sell, 900, 100, 1),
    ];
    let mut finished = finish(&rows, closing()).unwrap();
    let worker = &mut finished.workers[0];
    assert!(worker.market.resting_orders().is_empty());
    assert_eq!(
        worker.market.matching_mode(),
        crate::orderbook::MatchingMode::Auction
    );
    assert_eq!(worker.terminal_keys.len(), 2);
    let filled = receipt(
        &worker.receipts,
        rows[0].1.key(),
        ReceiptSource::Auction(0),
        0,
    );
    let released = receipt(
        &worker.receipts,
        rows[0].1.key(),
        ReceiptSource::DayEnd(0),
        0,
    );
    assert_eq!(released.kind, ReceiptKind::Release);
    assert_eq!(filled.qty_after, released.qty_before);
    assert_eq!(filled.value_after, released.value_before);
    assert_eq!(filled.delta.live_after, released.delta.released);
    assert_eq!(released.delta.live_after, ResVec::ZERO);
    assert!(
        !worker
            .receipts
            .iter()
            .any(|receipt| receipt.kind == ReceiptKind::Rollover),
        "收盘剩单直接日终失效，不能错误切到 Continuous 或制造承接阶段"
    );
    let mut ledger =
        EnvelopeLedger::new(0, rows.iter().map(|(_, envelope)| envelope.clone())).unwrap();
    ledger.apply(&mut worker.receipts).unwrap();
    ledger.remove_terminal(&worker.terminal_keys).unwrap();
    assert_eq!(ledger.terminal_count(), 2);
}

#[test]
fn day_end_payload_is_reindexed_over_remainders_after_earlier_envelopes_fill() {
    let rows = [
        row(1, 1, Side::Buy, 1_100, 100, 0),
        row(2, 2, Side::Sell, 900, 200, 1),
    ];
    let finished = finish(&rows, closing()).unwrap();
    let worker = &finished.workers[0];
    assert_eq!(
        receipt(
            &worker.receipts,
            rows[0].1.key(),
            ReceiptSource::Auction(0),
            0
        )
        .kind,
        ReceiptKind::Fill
    );
    assert_eq!(
        receipt(
            &worker.receipts,
            rows[1].1.key(),
            ReceiptSource::DayEnd(0),
            0
        )
        .kind,
        ReceiptKind::Release
    );
    assert!(!worker
        .receipts
        .iter()
        .any(|receipt| receipt.envelope == *rows[1].1.key()
            && receipt.local_key.source() == ReceiptSource::DayEnd(1)));
}

#[test]
fn closing_day_end_source_domain_includes_all_existing_and_new_book_remainders() {
    let rows = [
        row(1, 10, Side::Buy, 990, 100, 0),
        row(2, 20, Side::Buy, 990, 100, 1),
    ];
    let finished = finish(&rows, closing()).unwrap();
    let worker = &finished.workers[0];
    assert_eq!(
        receipt(
            &worker.receipts,
            rows[0].1.key(),
            ReceiptSource::DayEnd(0),
            0
        )
        .kind,
        ReceiptKind::Release
    );
    assert_eq!(
        receipt(
            &worker.receipts,
            rows[1].1.key(),
            ReceiptSource::DayEnd(1),
            0
        )
        .kind,
        ReceiptKind::Release
    );
    assert_eq!(
        worker
            .receipts
            .iter()
            .filter(|receipt| receipt.kind == ReceiptKind::Release)
            .count(),
        2
    );
    assert!(worker.market.resting_orders().is_empty());
}

fn second_leg_overflow_rows() -> Vec<(Order, Envelope)> {
    let mut rows = vec![row(0, 1, Side::Buy, 1_000, 200, 0),
        row(1, 2, Side::Sell, 1_000, 100, 1), row(1, 3, Side::Sell, 1_000, 100, 2)];
    let config = GameConfig::proposed_defaults();
    let before_value = Money::from_cents(i64::MAX - 50_000);
    rows[2].0.original_qty = 200;
    rows[2].0.filled_qty = 100;
    rows[2].0.filled_value = before_value;
    let original = rows[2].1.clone();
    let nominal = FeeComponents { commission: config.commission(before_value).unwrap(),
        stamp_tax: config.stamp_tax(before_value).unwrap(), transfer_fee: config.transfer_fee(before_value).unwrap() };
    let mut audit = original.audit();
    audit.filled_qty = 100;
    audit.filled_value = before_value;
    audit.nominal = nominal;
    rows[2].1 = Envelope::tick_start_existing(original.key().clone(), Money::ZERO, 100, audit);
    rows
}

#[test]
fn low_level_clearing_second_leg_money_overflow_keeps_real_partial_book_change_not_fake_atomicity() {
    let rows = second_leg_overflow_rows();
    let mut market = input(&rows, opening(599)).market;
    let before_price = market.last_price();
    let error = market.execute_auction_clearing(StockExchange::Shanghai).err().expect("第二腿累计金额必须 overflow");
    assert!(matches!(error, crate::MarketError::OrderBook(crate::orderbook::OrderError::Money(_))));
    let after = market.resting_orders();
    assert_eq!(after.len(), 2);
    let buyer = after.iter().find(|order| order.id == OrderId(1)).unwrap();
    assert_eq!((buyer.qty, buyer.filled_qty, buyer.filled_value), (100, 100, Money::from_cents(100_000)));
    assert!(after.iter().all(|order| order.id != OrderId(2)), "第一卖腿真实完成，不能声称底层未变");
    assert_eq!(market.filled_order_owner(OrderId(2)), Some(AccountId(1)));
    assert_eq!(market.last_price(), before_price);
    assert_eq!(market.matching_mode(), crate::orderbook::MatchingMode::Auction);
}

#[test]
fn real_market_tick_discards_second_clearing_leg_overflow_with_all_authority_and_outboxes_untouched() {
    let rows = second_leg_overflow_rows();
    let setup = crate::session::npc_working_quote_tests::quote_setup(1);
    let mut game = crate::GameSession::new(setup, 912).unwrap();
    game.fixture_account_mut(&AccountId(1)).unwrap().grant_position(StockId(0), 1_000, Money::from_cents(1_000)).unwrap();
    crate::session::npc_working_quote_tests::force_attention_candidate(&mut game, AccountId(1), 100);
    let market = game.state.markets.get_mut(&StockId(0)).unwrap();
    market.restore_resting_orders(&rows.iter().map(|(order, _)| order.clone()).collect::<Vec<_>>(), 3).unwrap();
    game.state.next_order_id = 4;
    game.state.envelope_ledger = EnvelopeLedger::new(0, rows.iter().map(|(_, envelope)| envelope.clone())).unwrap();
    game.hydrate_or_validate_envelope_ledger().unwrap();
    let before_hash = game.business_state_hash().unwrap();
    let before_orders = game.state.markets[&StockId(0)].resting_orders();
    let before_price = game.state.markets[&StockId(0)].last_price();
    let before_ledger = game.state.envelope_ledger.clone();
    let before_snapshot = game.snapshot_inner(false, true);
    let mut plan = super::plan_tick(super::PhaseInput::of(&game)).unwrap();
    assert!(super::market_tick_transaction::apply_tick_shadow_market_transaction(&mut plan).is_err());
    assert!(plan.event_outbox.is_empty());
    // T2：receipt 事实单载体——key 序列由 applied_receipts 派生，无独立载体可断言。
    assert!(plan.applied_receipts.is_empty());
    assert_eq!(game.business_state_hash().unwrap(), before_hash);
    assert!(game.step_with_commit_evidence().is_err(), "真实生产入口不能安装部分 book 或财务");
    assert_eq!(game.business_state_hash().unwrap(), before_hash);
    assert_eq!(game.tick(), 0);
    assert_eq!(game.state.markets[&StockId(0)].resting_orders(), before_orders);
    assert_eq!(game.state.markets[&StockId(0)].last_price(), before_price);
    assert_eq!(game.state.markets[&StockId(0)].matching_mode(), crate::orderbook::MatchingMode::Auction);
    assert_eq!(game.state.envelope_ledger, before_ledger);
    let after = game.snapshot_inner(false, true);
    for account in [AccountId(0), AccountId(1)] {
        assert_eq!(after.accounts[&account].cash, before_snapshot.accounts[&account].cash);
        assert_eq!(serde_json::to_value(&after.accounts[&account].positions).unwrap(),
            serde_json::to_value(&before_snapshot.accounts[&account].positions).unwrap());
        assert_eq!(after.accounts[&account].reserved_cash, before_snapshot.accounts[&account].reserved_cash);
        assert_eq!(after.accounts[&account].reserved_sell_qty, before_snapshot.accounts[&account].reserved_sell_qty);
    }
}

#[test]
fn auction_acceptance_rejects_even_one_cent_of_excess_live_cash() {
    let mut rows = [row(1, 9, Side::Buy, 1_000, 100, 0)];
    let original = rows[0].1.clone();
    rows[0].1 = Envelope::tick_start_existing(
        original.key().clone(),
        original.live().cash.add(Money::from_cents(1)).unwrap(),
        0,
        original.audit(),
    );
    assert!(
        coordinator(&rows, opening(0)).is_err(),
        "不能把额外现金静默当作合法预留"
    );
}

#[test]
fn buyer_fee_audit_drift_is_a_typed_fatal_before_auction_receipts_escape() {
    let mut rows = [
        row(1, 1, Side::Buy, 1_100, 100, 0),
        row(2, 2, Side::Sell, 900, 100, 1),
    ];
    let original = rows[0].1.clone();
    let mut audit = original.audit();
    audit.nominal.commission = Money::from_cents(1);
    audit.charged.commission = Money::from_cents(1);
    rows[0].1 =
        Envelope::tick_start_existing(original.key().clone(), original.live().cash, 0, audit);
    let error = finish(&rows, opening(599))
        .err()
        .expect("错误费用审计不能输出清算收据");
    assert!(matches!(
        error,
        crate::session::pipeline::continuous_matching::StockFinishFatal::Other(
            StepFatal::InvariantViolation { .. }
        )
    ));
}
