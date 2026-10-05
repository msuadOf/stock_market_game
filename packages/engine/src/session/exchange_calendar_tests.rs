use super::*;
use crate::calendar::{CalendarExchange, OfficialCoverageEntry};

fn mixed_session(first_closed: bool) -> GameSession {
    mixed_session_with_institution(first_closed, 0)
}

pub(super) fn mixed_session_with_institution(first_closed: bool, inst_count: u32) -> GameSession {
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
    setup.stocks[0].code = StockCode("600101".into());
    setup.company_system = simple_company_fixture!(crate; codes = setup.stocks.iter().map(|stock| stock.code.0.as_str()));
    setup.stocks[0].float_shares = 0;
    setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = inst_count;
    setup.ticks_per_day = 2;
    let mut shenzhen = setup.stocks[0].clone();
    shenzhen.code = StockCode("000101".into());
    shenzhen.exchange = StockExchange::Shenzhen;
    setup.stocks.push(shenzhen);
    setup.company_system = simple_company_fixture!(crate; codes = setup.stocks.iter().map(|stock| stock.code.0.as_str()));
    if !first_closed {
        setup.stocks.swap(0, 1);
    }
    let session = GameSession::new(setup, 81).unwrap();
    let mut save = session.save().unwrap();
    let date = save.setup.start_date;
    save.civil_clock
        .policy
        .official_coverage
        .push(OfficialCoverageEntry::new(
            CalendarExchange::Sse,
            2030,
            vec![(crate::CivilDate::from_iso("2030-01-01").unwrap(), date)],
            "synthetic-q13-test-only".into(),
            "synthetic-q13-test-only-digest".into(),
        ));
    GameSession::restore(&save).unwrap()
}

#[test]
fn mixed_exchange_restore_rejects_closed_stock_candle_and_minute_fabrication() {
    let mut session = mixed_session(false);
    session.step().unwrap();
    let closed = StockCode("600101".into());
    let open = StockCode("000101".into());
    let original = session.save().unwrap();
    let mut with_candle = original.clone();
    with_candle.snapshot.active_daily_candles.insert(
        closed.clone(),
        original.snapshot.active_daily_candles[&open].clone(),
    );
    assert!(GameSession::restore(&with_candle).is_err());
    let mut with_minutes = original.clone();
    with_minutes
        .market_minute_closes
        .get_mut(&closed)
        .unwrap()
        .push(MarketMinuteClose {
            absolute_trading_minute: 0,
            close: Money::from_cents(1_000),
        });
    assert!(GameSession::restore(&with_minutes).is_err());
    let restored = GameSession::restore(&original).unwrap();
    assert_eq!(restored.snapshot().active_daily_candles.len(), 1);
    assert!(restored.state.market_minute_closes[&closed].is_empty());
}

#[test]
fn reopened_exchange_uses_its_own_minute_keys_and_restore_bounds() {
    let mut session = mixed_session(false);
    for _ in 0..2 {
        session.step().unwrap();
    }
    session.end_civil_day().unwrap();
    let closed_before = StockCode("600101".into());
    let open_before = StockCode("000101".into());
    session.step().unwrap();
    assert_eq!(session.stock_trading_day(&closed_before).unwrap(), 0);
    assert_eq!(session.stock_trading_day(&open_before).unwrap(), 1);
    assert_eq!(
        session.state.market_minute_closes[&closed_before][0].absolute_trading_minute,
        0
    );
    assert_eq!(
        session.state.market_minute_closes[&open_before][0].absolute_trading_minute,
        240
    );
    let saved = session.save().unwrap();
    let restored = GameSession::restore(&saved).unwrap();
    assert_eq!(restored.stock_market_minute(&closed_before).unwrap(), 120);
    assert_eq!(restored.stock_market_minute(&open_before).unwrap(), 360);
    let mut ghost = saved;
    ghost.market_minute_closes.get_mut(&closed_before).unwrap()[0].absolute_trading_minute = 240;
    assert!(GameSession::restore(&ghost).is_err());
}

#[test]
fn mixed_exchange_auction_phases_produce_events_only_for_open_stock() {
    let mut saved = mixed_session(false).save().unwrap();
    saved.setup.ticks_per_day = 12;
    saved.setup.auction_ticks = 3;
    saved.setup.closing_auction_ticks = 3;
    let mut session = GameSession::restore(&saved).unwrap();
    let closed = StockCode("600101".into());
    let open = StockCode("000101".into());
    let mut completions = 0;
    for _ in 0..12 {
        for event in session.step().unwrap() {
            match event {
                Event::AuctionTick { code, .. } | Event::PriceTick { code, .. } => {
                    assert_ne!(code, closed)
                }
                Event::AuctionCompleted { code, .. } => {
                    assert_eq!(code, open);
                    completions += 1;
                }
                _ => {}
            }
        }
    }
    assert_eq!(completions, 2);
    assert_eq!(session.snapshot().daily_candles[&closed].len(), 360);
    assert_eq!(session.snapshot().daily_candles[&open].len(), 361);
    session.end_civil_day().unwrap();
    GameSession::restore(&session.save().unwrap()).unwrap();
}

#[test]
fn mixed_exchange_session_does_not_use_first_stock_as_shared_calendar() {
    for first_closed in [true, false] {
        let session = mixed_session(first_closed);
        assert_eq!(session.civil_clock().phase(), CivilPhase::IntradayTrading);
        assert_eq!(
            session
                .civil_clock()
                .completed_trading_sessions_expected()
                .unwrap(),
            1
        );
    }
}

#[test]
fn closed_step_does_not_advance_business_but_preserves_fatal_poison() {
    let mut session = mixed_session(false);
    session.step().unwrap();
    session.step().unwrap();
    session.end_civil_day().unwrap();
    while session.civil_clock().phase() != CivilPhase::ClosedDay {
        session.step().unwrap();
        session.step().unwrap();
        session.end_civil_day().unwrap();
    }
    let before = session.business_state_hash().unwrap();
    let tick = session.tick();
    let fatal = session.step().unwrap_err();
    assert_eq!(session.tick(), tick);
    assert_eq!(session.business_state_hash().unwrap(), before);
    assert_eq!(session.poison_reason(), Some(&fatal));
    assert_eq!(session.step().unwrap_err(), fatal);
    assert_eq!(session.save().unwrap_err(), fatal);
}

#[test]
fn closing_auction_restores_own_minute_240_memory_and_parent_before_daily_archive() {
    let mut saved = mixed_session_with_institution(false, 1).save().unwrap();
    saved.setup.closing_auction_ticks = 1;
    let mut session = GameSession::restore(&saved).unwrap();
    let account = AccountId(1);
    let code = StockCode("000101".into());
    session
        .state
        .npc_attention
        .get_mut(&account)
        .unwrap()
        .next_attention_candidate_tick = 2;
    session.state.attention_scheduler = [(2, account)].into_iter().collect();
    session.step().unwrap();
    assert_eq!(session.phase(), TradingPhase::ClosingAuction);
    assert_eq!(session.stock_market_minute(&code).unwrap(), 240);
    assert_eq!(session.snapshot().daily_candles[&code].len(), 360);
    let participant = session.state.belief_participants.get_mut(&account).unwrap();
    participant
        .watchlist_mut()
        .record_attention(&code, 240, 240)
        .unwrap();
    participant
        .price_memory_mut()
        .observe_price(&code, Money::from_cents(1_000), 240, 240)
        .unwrap();
    session.materialize_parent_order_intents(
        account,
        vec![Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: LimitPrice::Fixed(Money::from_cents(1_000)),
            qty: 100,
        }],
        session.current_market_minute(),
        WorkingOrderSlices {
            continuous: &[],
            auction: &[],
        },
    );
    assert_eq!(
        session.state.parent_orders[&account][&code].expires_market_minute(),
        240 + PARENT_ORDER_HORIZON_MINUTES
    );
    let restored = GameSession::restore(&session.save().unwrap()).unwrap();
    assert_eq!(restored.stock_market_minute(&code).unwrap(), 240);
    assert_eq!(restored.snapshot().daily_candles[&code].len(), 360);
}

#[test]
fn reopened_stock_price_threshold_wakes_valid_local_day_plan_without_attention() {
    use crate::plans::{OpinionSource, PlanOpen, PlanOpinion, PlanTarget, Urgency};
    let mut session = mixed_session_with_institution(false, 1);
    let account = AccountId(1);
    session
        .state
        .npc_attention
        .get_mut(&account)
        .unwrap()
        .next_attention_candidate_tick = 2;
    session.state.attention_scheduler = [(2, account)].into_iter().collect();
    for _ in 0..2 {
        session.step().unwrap();
    }
    session.end_civil_day().unwrap();
    let code = StockCode("600101".into());
    assert_eq!(session.state.day, 1);
    assert_eq!(session.stock_trading_day(&code).unwrap(), 0);
    session
        .state
        .accounts
        .get_mut(&account)
        .unwrap()
        .fixture_set_strategy(Some(crate::account::StoredStrategy::production(Box::new(
            crate::strategy::BeliefInstitutionStrategy::new(0.05, 100)
                .unwrap()
                .with_institution_style(crate::strategy::InstitutionStyle::DeepValue),
        ))));
    assert!(
        !session.state.accounts[&account]
            .strategy()
            .unwrap()
            .belief_chain_params()
            .unwrap()
            .daily_plan_review
    );
    let plan_id = session
        .state
        .plans
        .create(PlanOpen {
            account,
            code: code.clone(),
            direction: Side::Buy,
            target: PlanTarget::ShareCount(100),
            opinion: PlanOpinion {
                signal_score_bp: 8_000,
                source: OpinionSource::Blended,
            },
            confidence_bp: 6_000,
            urgency: Urgency::Normal,
            horizon_trading_days: 1,
            created_trading_day: 0,
        })
        .unwrap();
    session
        .state
        .plans
        .record_review(plan_id, 0, Money::from_cents(1_000), 0)
        .unwrap();
    session
        .state
        .markets
        .get_mut(&code)
        .unwrap()
        .fixture_set_last_price(Money::from_cents(1_100));
    assert!(!session
        .capture_ready_decision_chain_roots(&[])
        .unwrap()
        .is_empty());
}

#[test]
fn mixed_exchange_closed_stock_has_no_ticks_candles_or_orders_and_restores() {
    let mut session = mixed_session(false);
    let closed = StockCode("600101".into());
    let open = StockCode("000101".into());
    let cash_before = session.snapshot().accounts[&AccountId(0)].cash;
    let pending = session
        .receive_private_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: closed.clone(),
                side: Side::Buy,
                price: LimitPrice::Fixed(Money::from_cents(1_000)),
                qty: 100,
            },
        )
        .unwrap();
    session.state.pending_player.push(pending);
    let mut rejected = 0;
    for _ in 0..2 {
        let events = session.step().unwrap();
        rejected += events.iter().filter(|event| matches!(event,
            Event::IntentRejected { code, reason, .. }
            if code == &closed && serde_json::to_value(reason).unwrap() == serde_json::json!("ExchangeClosed"))).count();
        assert!(!events.iter().any(|event| matches!(event,
            Event::PriceTick { code, .. } | Event::AuctionTick { code, .. }
                | Event::AuctionCompleted { code, .. } | Event::Trade { code, .. }
            if code == &closed)));
    }
    let snapshot = session.snapshot();
    assert_eq!(rejected, 1);
    assert_eq!(snapshot.accounts[&AccountId(0)].cash, cash_before);
    assert_eq!(snapshot.accounts[&AccountId(0)].reserved_cash, Money::ZERO);
    assert_eq!(snapshot.daily_candles[&closed].len(), 360);
    assert_eq!(snapshot.daily_candles[&open].len(), 361);
    assert!(session.state.price_history[&closed].is_empty());
    session.end_civil_day().unwrap();
    let save = session.save().unwrap();
    let restored = GameSession::restore(&save).unwrap();
    assert_eq!(
        serde_json::to_value(restored.save().unwrap()).unwrap(),
        serde_json::to_value(save).unwrap()
    );
}

#[test]
fn closed_exchange_player_input_is_rejected_before_it_can_wait_for_next_day() {
    for shared in [false, true] {
        let mut session = mixed_session(false);
        let code = StockCode("600101".into());
        let intent = Intent::PlaceMarket {
            code,
            side: Side::Buy,
            qty: 100,
        };
        let before = session.save().unwrap();
        let result = if shared {
            session
                .shared_ingress()
                .enqueue_player_intent(AccountId(0), intent)
        } else {
            session.enqueue_player_intent(AccountId(0), intent)
        };
        assert!(matches!(
            result,
            Err(SessionError::Calendar(
                crate::calendar::CalendarError::NotATradingDay { .. }
            ))
        ));
        assert_eq!(
            serde_json::to_value(session.save().unwrap()).unwrap(),
            serde_json::to_value(before).unwrap()
        );
    }
}

#[test]
fn closed_stock_plan_deadline_and_t1_lock_do_not_advance_with_other_exchange() {
    use crate::plans::{OpinionSource, PlanOpen, PlanOpinion, PlanStatus, PlanTarget, Urgency};
    let mut session = mixed_session(false);
    let closed = StockCode("600101".into());
    session
        .state
        .accounts
        .get_mut(&AccountId(0))
        .unwrap()
        .fixture_insert_position(
            closed.clone(),
            crate::Position::from_restored_parts(100, 100, 100_000, 0),
        );
    let plan = session
        .state
        .plans
        .create(PlanOpen {
            account: AccountId(0),
            code: closed.clone(),
            direction: Side::Sell,
            target: PlanTarget::ShareCount(100),
            opinion: PlanOpinion {
                signal_score_bp: -8_000,
                source: OpinionSource::Blended,
            },
            confidence_bp: 6_000,
            urgency: Urgency::Normal,
            horizon_trading_days: 1,
            created_trading_day: 0,
        })
        .unwrap();
    for _ in 0..2 {
        session.step().unwrap();
    }
    assert_eq!(
        session.state.plans.plan(plan).unwrap().status(),
        PlanStatus::Active
    );
    assert_eq!(
        session.state.accounts[&AccountId(0)].positions()[&closed].t1_locked(),
        100
    );
    session.end_civil_day().unwrap();
    let restored = GameSession::restore(&session.save().unwrap()).unwrap();
    assert_eq!(
        restored.state.plans.plan(plan).unwrap().status(),
        PlanStatus::Active
    );
    assert_eq!(
        restored.state.accounts[&AccountId(0)].positions()[&closed].t1_locked(),
        100
    );
}

#[test]
fn reopened_stock_plan_request_uses_its_own_completed_trading_days() {
    use crate::plans::{
        AllocationGrant, OpinionSource, PlanOpen, PlanOpinion, PlanTarget, QuoteAction,
        QuoteDecision, QuoteReason, Urgency,
    };
    let mut session = mixed_session(false);
    let code = StockCode("600101".into());
    for _ in 0..2 {
        session.step().unwrap();
    }
    session.end_civil_day().unwrap();
    let plan_id = session
        .state
        .plans
        .create(PlanOpen {
            account: AccountId(0),
            code: code.clone(),
            direction: Side::Buy,
            target: PlanTarget::ShareCount(100),
            opinion: PlanOpinion {
                signal_score_bp: 8_000,
                source: OpinionSource::Blended,
            },
            confidence_bp: 6_000,
            urgency: Urgency::Normal,
            horizon_trading_days: 1,
            created_trading_day: 0,
        })
        .unwrap();
    let result = session.prepare_plan_observation(
        &session.state.plans,
        PlanExecutionRequest {
            plan_id,
            allocation: AllocationGrant {
                plan_id,
                code,
                allocated_cash: Money::ZERO,
                constraint: None,
            },
            decision: QuoteDecision {
                action: QuoteAction::Wait,
                reason: QuoteReason::PendingReconsideration,
            },
            trading_day: 0,
        },
    );
    assert!(
        result.is_ok(),
        "reopened stock must accept its own day zero: {:?}",
        result.err()
    );
}

#[test]
fn closed_stock_plan_does_not_receive_a_quote_or_soft_budget() {
    use crate::plans::{OpinionSource, PlanOpen, PlanOpinion, PlanTarget, Urgency};
    let mut session = mixed_session_with_institution(false, 1);
    let code = StockCode("600101".into());
    session
        .state
        .plans
        .create(PlanOpen {
            account: AccountId(1),
            code,
            direction: Side::Buy,
            target: PlanTarget::ShareCount(100),
            opinion: PlanOpinion {
                signal_score_bp: 8_000,
                source: OpinionSource::Blended,
            },
            confidence_bp: 6_000,
            urgency: Urgency::Normal,
            horizon_trading_days: 1,
            created_trading_day: 0,
        })
        .unwrap();
    let cursor = session.prepare_plan_quotes_for_account(
        AccountId(1),
        &session.build_market_view(),
        &session.state.plans,
    );
    assert!(
        cursor.is_none(),
        "closed stock must not enter quote or allocation planning"
    );
}
