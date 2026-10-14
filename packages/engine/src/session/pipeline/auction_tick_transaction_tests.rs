use super::account_validation_context::build_account_validation_context;
use super::adaptive_plan_chain::AdaptivePlanChainCoordinator;
use super::market_tick_transaction::{
    apply_tick_shadow_market_transaction_with_roots_for_test, prepare_market_tick,
};
use super::*;
use crate::plans::quote_policy::{QuoteAction, QuoteDecision, QuoteReason};
use crate::plans::{
    AllocationGrant, OpinionSource, PlanOpen, PlanOpinion, PlanStatus, PlanTarget, Urgency,
};
use crate::session::pipeline::DecisionResourceSnapshot;
use crate::session::plan_chain_candidates::PlanChainOperationBatch;
use crate::session::{PlanExecutionDisposition, PlanExecutionRequest};
use crate::strategy::ZiNoiseStrategy;
use crate::{AccountId, Event, GameSession, Intent, Money, OrderId, RejectionReason, Side};

fn fixture() -> (GameSession, PlanExecutionRequest) {
    let (mut session, request) = crate::session::plan_chain_candidates_tests::execution_fixture();
    session.state.setup.auction_ticks = 900;
    session.state.setup.ticks_per_day = 15_300;
    session.state.tick = 0;
    session.fixture_sync_market_modes_for_test();
    assert_eq!(session.phase(), crate::TradingPhase::CallAuction);
    (session, request)
}

fn player_only_auction_session() -> GameSession {
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(900);
    setup.npcs.inst_count = 0;
    GameSession::new(setup, 42).unwrap()
}

#[test]
fn real_retail_auction_review_cancels_old_quote_before_accepting_new_quote() {
    let mut setup = crate::session::npc_working_quote_tests::retail_quote_setup();
    setup.auction_ticks = 900;
    setup.ticks_per_day = 15_300;
    let mut authority = GameSession::new(setup, 41).unwrap();
    let retail = AccountId(1);
    let code = authority
        .stock_id(&authority.state.setup.stocks[0].code.0)
        .unwrap();
    authority
        .fixture_account_mut(&retail)
        .unwrap()
        .set_strategy(Box::new(ZiNoiseStrategy::new(1.0, 100, 0.5).unwrap()));
    let mut setup_events = Vec::new();
    authority.seed_auction_order_for_test(
        retail,
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: crate::LimitPrice::Fixed(Money::from_cents(900)),
            qty: 100,
        },
        &mut setup_events,
    );
    let old_id = setup_events
        .iter()
        .find_map(|event| match event {
            Event::OrderAccepted { id, .. } => Some(*id),
            _ => None,
        })
        .expect("fixture old auction quote must be accepted");
    authority.hydrate_or_validate_envelope_ledger().unwrap();
    let tick = authority.state.tick;
    crate::session::npc_working_quote_tests::force_attention_candidate(
        &mut authority,
        retail,
        tick,
    );
    super::npc_tick_preparation::queue_npc_for_next_tick(&mut authority).unwrap();

    let committed = prepare_market_tick(&mut authority)
        .expect("real retail auction review must complete")
        .commit();
    let cancel_seq = committed
        .commit
        .tick
        .event_values()
        .iter()
        .find_map(|event| match event {
            Event::OrderCanceled {
                seq, account, id, ..
            } if *account == retail && *id == old_id => Some(*seq),
            _ => None,
        })
        .expect("review must cancel the old quote");
    let (accept_seq, new_id, new_price) = committed
        .commit
        .tick
        .event_values()
        .iter()
        .find_map(|event| match event {
            Event::OrderAccepted {
                seq,
                account,
                id,
                price,
                ..
            } if *account == retail && *id != old_id => Some((*seq, *id, *price)),
            _ => None,
        })
        .expect("review must accept one new quote");

    // seq 只标识事件交付，不代表业务先后；直接核对本股实际执行的 typed facts。
    use super::continuous_matching::{
        ContinuousCancelFact, ContinuousExecutionOutcome, ContinuousPlaceFact,
    };
    let cancel_position = committed.output.execution_facts.iter().position(|fact| matches!(
        &fact.outcome, ContinuousExecutionOutcome::Cancel(ContinuousCancelFact::Canceled { account, order_id, .. })
            if *account == retail && *order_id == old_id,
    )).expect("必须保留实际撤旧事实");
    let place_position = committed.output.execution_facts.iter().position(|fact| matches!(
        &fact.outcome, ContinuousExecutionOutcome::Place { fact: ContinuousPlaceFact::Resting { account, order_id, .. }, .. }
            if *account == retail && *order_id == new_id,
    )).expect("必须保留实际新单挂入事实");
    assert!(
        cancel_position < place_position,
        "同股真实执行必须先撤旧再挂新，不能从输出 seq 推断"
    );
    assert_ne!(cancel_seq, accept_seq);
    assert_ne!(new_price, Money::from_cents(900));
    let snapshot_after = authority.snapshot_inner(false, true);
    let reserved = snapshot_after.accounts[&retail].reserved_cash;
    let one_order = crate::session::buy_order_reservation(
        &authority.state.setup.config,
        new_price,
        100,
        Money::ZERO,
    )
    .unwrap();
    assert_eq!(reserved, one_order, "同一 tick 的换单不得双冻结旧单与新单");
    let projected = authority.auction_order_projection_for_test();
    let own_orders = projected
        .get(&code)
        .map(|orders| {
            orders
                .iter()
                .filter(|order| order.owner == retail)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    assert!(
        matches!(own_orders.as_slice(), [order] if order.order_id == new_id.0 && order.limit == new_price)
    );
}

#[test]
fn production_market_rejection_consumes_id_and_does_not_refund_account_validation_budget() {
    let mut authority = player_only_auction_session();
    let code = request_code(&authority);
    let protective_price = authority.state.markets[&code].up_stop().unwrap();
    let one_order_cash = crate::session::buy_order_reservation(
        &authority.state.setup.config,
        protective_price,
        100,
        Money::ZERO,
    )
    .unwrap();
    authority
        .fixture_account_mut(&AccountId(0))
        .unwrap()
        .fixture_set_cash(one_order_cash);
    for _ in 0..2 {
        authority
            .enqueue_player_intent(
                AccountId(0),
                Intent::PlaceMarket {
                    code: code.clone(),
                    side: Side::Buy,
                    qty: 100,
                },
            )
            .unwrap();
    }
    let rejected_id = OrderId(authority.state.next_order_id);

    let committed = prepare_market_tick(&mut authority)
        .expect("市场拒绝属于普通 StockProcessing 结果")
        .commit();

    assert!(matches!(
        committed.output.validation.results(),
        [
            CandidateValidationResult::Accepted { .. },
            CandidateValidationResult::Rejected {
                reason: RejectionReason::InsufficientCash,
                ..
            }
        ]
    ));
    assert_eq!(authority.state.next_order_id, rejected_id.0 + 1);
    assert_eq!(committed.output.receipts.len(), 1);
    assert_eq!(committed.output.receipts[0].kind, ReceiptKind::Reject);
    assert_eq!(committed.output.receipts[0].envelope.order, rejected_id);
    assert!(committed.commit.tick.event_values().iter().any(|event| matches!(
        event,
        Event::IntentRejected {
            reason: RejectionReason::AuctionLimitOrderRequired,
            ..
        }
    )));
    assert!(committed.commit.tick.event_values().iter().any(|event| matches!(
        event,
        Event::IntentRejected {
            reason: RejectionReason::InsufficientCash,
            ..
        }
    )));
    assert_eq!(
        authority.state.accounts[&AccountId(0)].cash(),
        one_order_cash
    );
    assert_eq!(
        authority.snapshot().accounts[&AccountId(0)].reserved_cash,
        Money::ZERO
    );
}

#[test]
fn production_market_rejection_consumes_the_shared_account_validation_budget_before_a_later_limit()
{
    let mut authority = player_only_auction_session();
    let code = request_code(&authority);
    let protective_price = authority.state.markets[&code].up_stop().unwrap();
    let one_order_cash = crate::session::buy_order_reservation(
        &authority.state.setup.config,
        protective_price,
        100,
        Money::ZERO,
    )
    .unwrap();
    authority
        .fixture_account_mut(&AccountId(0))
        .unwrap()
        .fixture_set_cash(one_order_cash);
    authority
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceMarket {
                code: code.clone(),
                side: Side::Buy,
                qty: 100,
            },
        )
        .unwrap();
    authority
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code,
                side: Side::Buy,
                price: crate::LimitPrice::Fixed(protective_price),
                qty: 100,
            },
        )
        .unwrap();
    let rejected_market_id = OrderId(authority.state.next_order_id);

    let committed = prepare_market_tick(&mut authority)
        .expect("Auction 市场拒绝属于普通 StockProcessing 结果")
        .commit();

    // AccountValidation 按 protective price 预算受理市价请求；A 股集合竞价仅允许限价，stock_processing 随后拒绝。
    // 已分配 identity 与 AccountValidation 预算均不退还给后续限价请求。
    assert!(matches!(
        committed.output.validation.results(),
        [
            CandidateValidationResult::Accepted { .. },
            CandidateValidationResult::Rejected {
                reason: RejectionReason::InsufficientCash,
                ..
            }
        ]
    ));
    assert_eq!(authority.state.next_order_id, rejected_market_id.0 + 1);
    assert_eq!(committed.output.receipts.len(), 1);
    assert_eq!(
        committed.output.receipts[0].envelope.order,
        rejected_market_id
    );
    assert!(committed.commit.tick.event_values().iter().any(|event| matches!(
        event,
        Event::IntentRejected {
            reason: RejectionReason::AuctionLimitOrderRequired,
            ..
        }
    )));
    assert!(committed.commit.tick.event_values().iter().any(|event| matches!(
        event,
        Event::IntentRejected {
            reason: RejectionReason::InsufficientCash,
            ..
        }
    )));
}

#[test]
fn production_market_validation_rejects_quantity_and_shares_before_allocating_id() {
    let mut authority = player_only_auction_session();
    let code = request_code(&authority);
    for intent in [
        Intent::PlaceMarket {
            code: code.clone(),
            side: Side::Buy,
            qty: 1,
        },
        Intent::PlaceMarket {
            code: code.clone(),
            side: Side::Sell,
            qty: 100,
        },
        Intent::PlaceMarket {
            code: code.clone(),
            side: Side::Buy,
            qty: 100,
        },
    ] {
        authority
            .enqueue_player_intent(AccountId(0), intent)
            .unwrap();
    }
    let rejected_id = OrderId(authority.state.next_order_id);

    let committed = prepare_market_tick(&mut authority)
        .expect("AccountValidation 业务拒绝与 StockProcessing 市场拒绝必须原子提交")
        .commit();

    let results = committed.output.validation.results();
    assert_eq!(results.len(), 3);
    let result_for = |index| {
        results
            .iter()
            .find(|result| result.key() == &IntentCandidateKey::player(index))
            .unwrap()
    };
    assert!(matches!(
        result_for(0),
        CandidateValidationResult::Rejected {
            reason: RejectionReason::InvalidQuantity,
            ..
        }
    ));
    assert!(matches!(
        result_for(1),
        CandidateValidationResult::Rejected {
            reason: RejectionReason::InsufficientShares,
            ..
        }
    ));
    assert!(matches!(
        result_for(2),
        CandidateValidationResult::Accepted { .. }
    ));
    // 现金请求保持账户局部受理先后；卖单使用独立股份预算，可位于任一买单前后。
    let result_position = |index| {
        results
            .iter()
            .position(|result| result.key() == &IntentCandidateKey::player(index))
            .unwrap()
    };
    assert!(result_position(0) < result_position(2));
    assert!(result_for(0).sealed_index() < result_for(2).sealed_index());
    let identities = committed.output.validation.identities().collect::<Vec<_>>();
    assert_eq!(identities.len(), 3);
    for (index, expected_id) in [(0, None), (1, None), (2, Some(rejected_id))] {
        let identity = identities
            .iter()
            .find(|(key, _, _)| *key == &IntentCandidateKey::player(index))
            .unwrap();
        assert_eq!(identity.2, expected_id);
    }
    assert_eq!(authority.state.next_order_id, rejected_id.0 + 1);
    assert_eq!(committed.output.receipts.len(), 1);
    assert_eq!(committed.output.receipts[0].kind, ReceiptKind::Reject);
    assert_eq!(committed.output.receipts[0].envelope.order, rejected_id);
}

fn opening_completion_fixture(
    target_qty: u32,
    seller_qty: u32,
) -> (GameSession, PlanExecutionRequest) {
    let mut session = GameSession::new(
        crate::session::npc_working_quote_tests::quote_setup(900),
        47,
    )
    .unwrap();
    session.state.tick = 599;
    let code = request_code(&session);
    let plan_id = session
        .state
        .plans
        .create(PlanOpen {
            account: AccountId(1),
            code: code.clone(),
            direction: Side::Buy,
            target: PlanTarget::ShareCount(target_qty),
            opinion: PlanOpinion {
                signal_score_bp: 3_000,
                source: OpinionSource::Blended,
            },
            confidence_bp: 8_000,
            urgency: Urgency::Normal,
            horizon_trading_days: 5,
            created_trading_day: 0,
        })
        .unwrap();
    session
        .fixture_account_mut(&AccountId(0))
        .unwrap()
        .grant_position(code.clone(), seller_qty, Money::from_cents(900))
        .unwrap();
    session
        .fixture_append_auction_orders(
            code.clone(),
            vec![crate::AuctionOrderSnap {
                owner: AccountId(0),
                side: Side::Sell,
                limit: Money::from_cents(900),
                qty: seller_qty,
                order_id: 30,
            }],
        )
        .unwrap();
    session.state.next_order_id = 31;
    session.hydrate_or_validate_envelope_ledger().unwrap();
    (
        session,
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
                    price: Money::from_cents(1_100),
                    qty: target_qty,
                },
                reason: QuoteReason::OppositeQuoteProbe,
            },
            trading_day: 0,
        },
    )
}

fn commit_with_roots(authority: &mut GameSession, request: PlanExecutionRequest) {
    drop(commit_with_roots_returning(authority, request));
}

fn commit_with_roots_returning(
    authority: &mut GameSession,
    request: PlanExecutionRequest,
) -> Result<Vec<Event>, ()> {
    super::npc_tick_preparation::queue_npc_for_next_tick(authority).unwrap();
    let mut roots = PlanChainOperationBatch::empty();
    roots.push_execution(request);
    let mut plan = plan_tick(PhaseInput { session: authority })
    .unwrap();
    apply_tick_shadow_market_transaction_with_roots_for_test(&mut plan, roots).unwrap();
    Ok(
        super::candidate_commit::prepare_tick_shadow_plan_commit(authority, plan)
            .unwrap()
            .commit()
            .tick
            .event_values(),
    )
}

fn install_original_child(session: &mut GameSession, request: &PlanExecutionRequest) -> OrderId {
    let mut batch = PlanChainOperationBatch::empty();
    batch.push_execution(request.clone());
    let events = commit_injected_plan_roots_for_test(session, batch);
    let accepted = events
        .iter()
        .filter_map(|event| match event {
            Event::OrderAccepted { id, .. } => Some(*id),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(accepted.len(), 1, "expected one accepted auction child");
    accepted[0]
}

#[test]
fn parent_transaction_seam_rolls_back_every_authoritative_family_on_later_round_failure() {
    let (mut authority, mut request) = fixture();
    let old_id = install_original_child(&mut authority, &request);
    authority
        .state
        .envelope_ledger
        .rebase_live_for_next_tick()
        .unwrap();
    authority.state.next_order_id = crate::orderbook::js_safe_u64::MAX;
    request.decision.action = QuoteAction::Replace {
        order_id: old_id,
        price: Money::from_cents(901),
        qty: 100,
    };
    let mut roots = PlanChainOperationBatch::empty();
    roots.push_execution(request);
    let business_before = authority.business_state_hash().unwrap();
    let session_before = authority.session_state_hash().unwrap();
    let next_order_before = authority.state.next_order_id;
    let queue_before =
        serde_json::to_value(&authority.auction_order_projection_for_test()).unwrap();
    let ledger_before = authority.state.envelope_ledger.clone();
    let plans_before = serde_json::to_value(&authority.state.plans).unwrap();
    let mut plan = plan_tick(PhaseInput { session: &authority })
    .unwrap();

    let result = apply_tick_shadow_market_transaction_with_roots_for_test(&mut plan, roots);
    eprintln!("DBG late failure result {:?}", result.as_ref().err());

    assert!(matches!(
        result,
        Err(
            super::market_tick_transaction::MarketTransactionError::Preparation(
                StepFatal::InvariantViolation { .. }
            )
        )
    ));
    assert!(plan.execute(|_| Ok(())).is_err());
    assert_eq!(authority.business_state_hash().unwrap(), business_before);
    assert_eq!(authority.session_state_hash().unwrap(), session_before);
    assert_eq!(authority.state.next_order_id, next_order_before);
    assert_eq!(
        serde_json::to_value(&authority.auction_order_projection_for_test()).unwrap(),
        queue_before
    );
    assert_eq!(authority.state.envelope_ledger, ledger_before);
    assert_eq!(
        serde_json::to_value(&authority.state.plans).unwrap(),
        plans_before
    );
    // 母单镜像断言随镜像删除（owner8 执行腿唯一所有权）；无漂移不变量由 plans_before 覆盖。
}

#[test]
fn prepared_parent_auction_seam_commits_one_complete_tick() {
    let mut authority = player_only_auction_session();
    let code = request_code(&authority);
    authority
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
                qty: 100,
            },
        )
        .unwrap();

    let committed = prepare_market_tick(&mut authority).unwrap().commit();

    assert_eq!(
        committed.output.candidates.candidates()[0].key(),
        &IntentCandidateKey::player(0)
    );
    assert_eq!(
        committed
            .output
            .validation
            .accepted()
            .cloned()
            .collect::<Vec<_>>(),
        vec![IntentCandidateKey::player(0)]
    );
    assert!(committed.output.plan_reports.is_empty());
    assert_eq!(
        committed.output.auction_finalizers[0].auction_tail_passes(),
        1
    );
    assert_eq!(
        committed
            .commit
            .tick
            .event_values()
            .iter()
            .filter(|event| matches!(event, Event::AuctionTick { .. }))
            .count(),
        1
    );
    assert!(authority.state.pending_player.is_empty());
    assert_eq!(
        authority.auction_order_projection_for_test()[&code].len(),
        1
    );
    assert_eq!(
        authority.business_state_hash().unwrap(),
        committed.commit.receipt.business_hash()
    );
}

#[test]
fn parent_transaction_applies_opening_completion_full_fill_once() {
    let (mut authority, request) = opening_completion_fixture(100, 100);
    let plan_id = request.plan_id;

    let committed_dbg = commit_with_roots_returning(&mut authority, request).unwrap();
    eprintln!("DBG events {:?}", committed_dbg);
    let plan = authority.state.plans.plan(plan_id).unwrap();
    assert_eq!(plan.filled_qty(), 100);
    assert_eq!(plan.status(), PlanStatus::Completed);
    // 旧 pending_plan_events/parent_orders 空断言随两结构删除；等价断言=完成计划无在途子单。
    assert_eq!(plan.active_child_order_id(), None);
}

#[test]
fn parent_transaction_applies_opening_completion_partial_fill_once() {
    let (mut authority, request) = opening_completion_fixture(200, 100);
    let plan_id = request.plan_id;
    let code = request.allocation.code.clone();

    commit_with_roots(&mut authority, request);

    let plan = authority.state.plans.plan(plan_id).unwrap();
    assert_eq!(plan.filled_qty(), 100);
    assert_eq!(plan.status(), PlanStatus::Active);
    let child_id = plan
        .active_child_order_id()
        .expect("partial child remains active");
    let leg = authority
        .state
        .plans
        .plan(plan_id)
        .unwrap()
        .execution()
        .expect("partial child keeps the execution leg");
    assert_eq!(plan.filled_qty(), 100);
    assert_eq!(plan.active_child_order_id(), Some(child_id));
    assert_eq!(leg.active_child_remaining_qty(), Some(100));
}

fn request_code(session: &GameSession) -> crate::StockId {
    session.state.markets.keys().next().unwrap().clone()
}
