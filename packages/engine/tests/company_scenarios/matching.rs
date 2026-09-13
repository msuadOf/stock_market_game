use super::*;
use engine::plans::quote_policy::{QuoteAction, QuoteDecision, QuoteReason};
use engine::plans::{AllocationGrant, PlanBook, PlanOpen};
use engine::session::{PlanExecutionDisposition, PlanExecutionRequest};
use engine::{AccountId, OpinionSource, PlanOpinion, PlanStatus, PlanTarget, Side, Urgency};

pub(crate) fn matching_session(float_shares: u32) -> GameSession {
    let mut setup = setup("2030-01-07");
    setup.stocks = vec![StockSpec {
        code: code("600101"),
        exchange: StockExchange::Shanghai,
        initial_price: Money::from_cents(1_000),
        category: SecurityCategory::MainBoard,
        limit_pct: 0.10,
        tick: Money::from_cents(1),
        total_shares: 10_000_000,
        float_shares,
    }];
    setup.npcs = NpcSetup {
        retail_count: 1,
        inst_count: 1,
        hot_count: 0,
        retail_cash_median: Money::from_cents(10_000_000),
    };
    setup.auction_ticks = 0;
    setup.closing_auction_ticks = 0;
    setup.float_allocation = FloatAllocation::ByKind {
        retail: 1.0,
        inst: 0.0,
        hot: 0.0,
    };
    GameSession::new(setup, SEED).expect("controlled matching session is valid")
}

pub(crate) fn request(plan_id: engine::PlanId, code: StockCode, qty: u32) -> PlanExecutionRequest {
    let fixture = fixture();
    PlanExecutionRequest {
        plan_id,
        allocation: AllocationGrant {
            plan_id,
            code,
            allocated_cash: Money::from_cents(10_000_000),
            constraint: None,
        },
        decision: QuoteDecision {
            action: QuoteAction::Submit {
                price: Money::from_cents(fixture.matching.price_cents),
                qty,
            },
            reason: QuoteReason::OppositeQuoteProbe,
        },
        trading_day: 0,
    }
}

pub(crate) fn plan(
    book: &mut PlanBook,
    account: AccountId,
    code: StockCode,
    side: Side,
    qty: u32,
) -> engine::PlanId {
    book.create(PlanOpen {
        account,
        code,
        direction: side,
        target: PlanTarget::ShareCount(qty),
        opinion: PlanOpinion {
            signal_score_bp: if side == Side::Buy { 3_000 } else { -3_000 },
            source: OpinionSource::Blended,
        },
        confidence_bp: 8_000,
        urgency: Urgency::Normal,
        horizon_trading_days: 5,
        created_trading_day: 0,
    })
    .expect("controlled plan is valid")
}

#[test]
fn real_partial_fill_reconciles_plan_orders_accounts_and_active_candle() {
    // Given: actual session inventory, one 100-share seller plan, and one 400-share buyer plan.
    let fixture = fixture();
    let mut session = matching_session(1_000);
    let code = code("600101");
    let mut plans = PlanBook::default();
    let seller = plan(
        &mut plans,
        AccountId(1),
        code.clone(),
        Side::Sell,
        fixture.matching.seller_shares,
    );
    let buyer = plan(
        &mut plans,
        AccountId(2),
        code.clone(),
        Side::Buy,
        fixture.matching.buyer_target_shares,
    );
    let seller_cash_before = session.account(AccountId(1)).expect("seller account").cash;
    let buyer_cash_before = session.account(AccountId(2)).expect("buyer account").cash;
    let seller_position_before = session
        .account(AccountId(1))
        .expect("seller account")
        .positions[&code]
        .qty;
    let trade_value = Money::from_cents(100_000);
    let config = &session.save().setup.config;
    let commission = config.commission(trade_value).unwrap();
    let transfer = config.transfer_fee(trade_value).unwrap();
    let stamp = config.stamp_tax(trade_value).unwrap();
    session
        .execute_plan_observation(
            &mut plans,
            request(seller, code.clone(), fixture.matching.seller_shares),
        )
        .expect("seller child enters the real book");

    // When: the buyer crosses the authoritative orderbook.
    let report = session
        .execute_plan_observation(
            &mut plans,
            request(buyer, code.clone(), fixture.matching.buyer_target_shares),
        )
        .expect("buyer child crosses the real book");

    // Then: one actual fill advances only 100 shares and leaves a live 300-share child/freeze.
    assert!(
        report.events.iter().any(|event| matches!(
            event,
            engine::session::Event::Trade { code: traded, qty: 100, .. } if traded == &code
        )),
        "crossing buyer must receive the real seller fill: {report:?}"
    );
    assert!(matches!(
        report.disposition,
        PlanExecutionDisposition::Submitted { .. }
    ));
    session
        .synchronize_plan_execution(&mut plans)
        .expect("actual fill synchronizes to the plan book");
    assert_eq!(plans.plan(buyer).expect("buyer plan").filled_qty, 100);
    assert_eq!(
        plans.plan(buyer).expect("buyer plan").status,
        PlanStatus::Active
    );
    let save = session.save();
    assert_eq!(
        save.parent_orders[&AccountId(2)][&code].active_child_remaining_qty,
        Some(300)
    );
    assert_eq!(save.snapshot.active_daily_candles[&code].volume, 100);
    assert_eq!(
        save.snapshot.active_daily_candles[&code]
            .trade_stats
            .as_ref()
            .expect("real trade stats")
            .turnover_cents,
        100_000
    );
    assert!(session.account(AccountId(1)).expect("seller account").cash > seller_cash_before);
    assert_eq!(
        session
            .account(AccountId(1))
            .expect("seller account")
            .cash
            .cents(),
        seller_cash_before.cents() + trade_value.cents()
            - commission.cents()
            - transfer.cents()
            - stamp.cents()
    );
    assert_eq!(
        session
            .account(AccountId(2))
            .expect("buyer account")
            .cash
            .cents(),
        buyer_cash_before.cents() - trade_value.cents() - commission.cents() - transfer.cents()
    );
    assert_eq!(
        session
            .account(AccountId(2))
            .expect("buyer account")
            .positions[&code]
            .qty,
        100
    );
    assert_eq!(
        session
            .account(AccountId(1))
            .expect("seller account")
            .positions[&code]
            .qty,
        seller_position_before - 100
    );
    assert_eq!(
        save.snapshot.accounts[&AccountId(2)].reserved_cash,
        Money::from_cents(300_003)
    );
    assert_eq!(
        save.snapshot.accounts[&AccountId(1)]
            .reserved_sell_qty
            .get(&code),
        None
    );
    println!(
        "{{\"scenario\":\"partial_fill\",\"qty\":100,\"turnover_cents\":100000,\"buyer_cash\":{},\"seller_cash\":{},\"buyer_reserved_cash\":{}}}",
        session.account(AccountId(2)).unwrap().cash.cents(),
        session.account(AccountId(1)).unwrap().cash.cents(),
        save.snapshot.accounts[&AccountId(2)].reserved_cash.cents()
    );
}

#[test]
fn no_counterparty_preserves_zero_trade_volume_and_unfilled_plan() {
    // Given: the real session fixture with no float, so no account can provide a counterparty.
    let mut session = matching_session(0);

    // When: real plans submit a noncrossing buy through the real orderbook.
    let code = code("600101");
    let mut plans = PlanBook::default();
    let buyer = plan(&mut plans, AccountId(1), code.clone(), Side::Buy, 100);
    session
        .execute_plan_observation(&mut plans, request(buyer, code.clone(), 100))
        .expect("unfilled child may rest legally");

    // Then: no injected liquidity appears, and no plan progress or candle volume is fabricated.
    session
        .synchronize_plan_execution(&mut plans)
        .expect("unfilled plan synchronizes");
    assert_eq!(plans.plan(buyer).expect("buyer plan").filled_qty, 0);
    assert!(session
        .save()
        .snapshot
        .active_daily_candles
        .get(&code)
        .is_none());
    println!("{{\"scenario\":\"no_counterparty\",\"filled_qty\":0,\"volume\":0}}");
}
