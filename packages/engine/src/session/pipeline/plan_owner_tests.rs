//! 架构项 #8 红测：计划生命周期唯一 owner（PlanBook/TradingPlan）。
//!
//! 真实 GameSession 链（计划 → 委托 → 部分成交 → 撤销 → 重试 → 存档恢复）驱动。
//! 旧实现里 `ParentOrderPlan` 持有第二份可变计划状态：已确认撤销只清母单镜像
//! （`record_parent_order_canceled`），不回写 `TradingPlan`，产生 owner 漂移，
//! 使下一次计划报价以 `IncompatibleExecutionState` 致命失败。生产触发面包括
//! NPC 工作单对账撤单与 ExpiryShadow（二者最终都依赖同一确认撤销副作用）。
//! 本组测试断言唯一 owner 语义；在双 owner 旧实现上必须失败（红）。

use super::*;
use crate::plans::quote_policy::{QuoteAction, QuoteDecision, QuoteReason};
use crate::plans::{AllocationGrant, PlanBook, PlanOpen, PlanOpinion, PlanTarget, Urgency};
use crate::session::npc_working_quote_tests::two_stock_quote_setup;
use crate::session::plan_chain_candidates::PlanChainOperationBatch;
use crate::session::PlanExecutionDisposition;
use crate::session::PlanExecutionRequest;
use crate::session::failure::StepFatal;
use crate::{AccountId, Event, Intent, Money, Side, StockId};

use super::market_tick_transaction::apply_tick_shadow_market_transaction_with_roots_for_test;
use super::continuous_tick_transaction_tests::live_buy_plan_case;

/// 使用真实已登记 NPC 请求驱动普通撤单；不伪造成功事实，不手工释放资源。
fn confirm_cancel_tick(
    authority: &mut GameSession,
    owner: AccountId,
    code: StockId,
    order_id: crate::OrderId,
) {
    let received = authority
        .receive_private_intent(owner, Intent::Cancel { code, id: order_id })
        .unwrap();
    crate::session::pipeline::staged_decision::stage(crate::session::pipeline::NpcDecisionHandover::new(authority.state.tick, Vec::new(), vec![received], Vec::new()));
    let before = authority.state.plans.clone();
    let mut candidate = plan_tick(PhaseInput::of(authority)).unwrap();
    let output = apply_tick_shadow_market_transaction_with_roots_for_test(
        &mut candidate,
        PlanChainOperationBatch::empty(),
    )
    .unwrap();
    // 脊树 MarketTransactionOutput 以 facts:Vec<EventFact> 承载事件（无 events 字段）；
    // o8 测试按其旧形态书写，此处按合并树类型适配（事件断言语义不变）。
    assert!(output.facts.iter().any(|fact| matches!(
        &fact.event,
        Event::OrderCanceled { id, .. } if *id == order_id
    )));
    assert!(output.receipts.iter().any(|receipt| {
        receipt.envelope.order == order_id && receipt.kind == super::ReceiptKind::Release
    }));
    assert_eq!(authority.state.plans, before, "候选不得提前安装计划 authority");
    super::candidate_commit::prepare_tick_shadow_plan_commit(authority, candidate)
        .unwrap()
        .commit();
}

fn open_buy_plan(
    authority: &mut GameSession,
    account: AccountId,
    code: StockId,
    target: u32,
) -> crate::plans::PlanId {
    let plan_id = authority
        .state
        .plans
        .create(PlanOpen {
            account,
            code,
            direction: Side::Buy,
            target: PlanTarget::ShareCount(target),
            opinion: PlanOpinion {
                signal_score_bp: 3_000,
                source: crate::plans::OpinionSource::Blended,
            },
            confidence_bp: 8_000,
            urgency: Urgency::Normal,
            horizon_trading_days: 5,
            created_trading_day: u64::from(authority.state.day),
        })
        .unwrap();
    // 与真实生命周期一致：开户时记录首次本人观察基线，避免后续 tick 触发复核。
    let issuer = authority.state.issuer_of_stock(StockId(0)).unwrap();
    let acquired_count = authority.state.belief_participants[&account]
        .information()
        .records_for_company(&issuer)
        .len();
    authority
        .state
        .plans
        .record_review(
            plan_id,
            u64::from(authority.state.day),
            authority.state.markets[&code].last_price(),
            u32::try_from(acquired_count).unwrap(),
        )
        .unwrap();
    plan_id
}

fn owner_book(target: u32) -> (PlanBook, crate::plans::PlanId) {
    let mut book = PlanBook::default();
    let id = book.create(PlanOpen {
        account: AccountId(1), code: StockId(0), direction: Side::Buy,
        target: PlanTarget::ShareCount(target),
        opinion: PlanOpinion { signal_score_bp: 3000, source: crate::plans::OpinionSource::Blended },
        confidence_bp: 8000, urgency: Urgency::Normal,
        horizon_trading_days: 5, created_trading_day: 0,
    }).unwrap();
    book.install_execution(id, 100, Money::from_cents(1000), 240).unwrap();
    (book, id)
}

fn commit_roots(authority: &mut GameSession, roots: PlanChainOperationBatch) -> Vec<Event> {
    let phase = authority.phase();
    let mut tick = plan_tick(PhaseInput::of(authority)).unwrap();
    // 脊树统一路径（T5 八十二别名收口后）：统一入口=market_tick_transaction，
    // 输出为 MarketTransactionOutput（facts 承载事件，无 events/auction 字段）。
    let events = if phase == crate::TradingPhase::Continuous {
        apply_tick_shadow_market_transaction_with_roots_for_test(&mut tick, roots)
            .unwrap().facts.iter().map(|fact| fact.event.clone()).collect()
    } else {
        super::market_tick_transaction::apply_tick_shadow_market_transaction_with_roots_for_test(
            &mut tick, roots,
        ).unwrap().facts.iter().map(|fact| fact.event.clone()).collect()
    };
    super::candidate_commit::prepare_tick_shadow_plan_commit(authority, tick).unwrap().commit();
    events
}

fn closing_child() -> (GameSession, crate::plans::PlanId, crate::OrderId) {
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
    setup.ticks_per_day = 2;
    setup.closing_auction_ticks = 1;
    let mut authority = GameSession::new(setup, 47).unwrap();
    authority.state.attention_scheduler.clear();
    authority.state.npc_attention.get_mut(&AccountId(1)).unwrap().next_attention_candidate_tick = u64::MAX;
    let id = open_buy_plan(&mut authority, AccountId(1), StockId(0), 400);
    let mut roots = PlanChainOperationBatch::empty();
    roots.push_execution(submit_request(&authority, id, StockId(0), 1000, 100));
    commit_roots(&mut authority, roots);
    assert_eq!(authority.phase(), crate::TradingPhase::ClosingAuction);
    let child = authority.state.plans.plan(id).unwrap().active_child_order_id().unwrap();
    (authority, id, child)
}

#[test]
fn only_matching_settled_child_advances_and_completion_retains_linked_parent() {
    let (mut book, id) = owner_book(100);
    assert!(!book.record_child_submitted(AccountId(1), &StockId(0), Side::Sell, crate::OrderId(1), 100, 0).unwrap());
    assert!(book.record_child_submitted(AccountId(1), &StockId(0), Side::Buy, crate::OrderId(1), 100, 0).unwrap());
    let before = book.clone();
    assert!(!book.record_child_fill(AccountId(1), &StockId(0), Side::Sell, crate::OrderId(1), 10, 0).unwrap());
    assert!(!book.record_child_fill(AccountId(1), &StockId(0), Side::Buy, crate::OrderId(2), 10, 0).unwrap());
    assert!(!book.record_child_released(AccountId(1), &StockId(0), crate::OrderId(2), 0).unwrap());
    assert_eq!(book, before);
    book.record_child_fill(AccountId(1), &StockId(0), Side::Buy, crate::OrderId(1), 40, 0).unwrap();
    assert_eq!(book.plan(id).unwrap().filled_qty(), 40);
    assert_eq!(book.plan(id).unwrap().active_child_remaining_qty(), Some(60));
    book.record_child_fill(AccountId(1), &StockId(0), Side::Buy, crate::OrderId(1), 60, 0).unwrap();
    let owner = book.plan(id).unwrap();
    assert_eq!(owner.filled_qty(), 100);
    assert_eq!(owner.status(), crate::plans::PlanStatus::Completed);
    assert_eq!(owner.active_child_order_id(), None);
    assert_eq!(owner.active_child_remaining_qty(), None);
    assert!(owner.execution().is_none());
    assert!(book.active_plan(AccountId(1), &StockId(0)).is_none());
    assert_eq!(before.plan(id).unwrap().filled_qty(), 0, "未修改的 snapshot 不受 owner 更新影响");
}

#[test]
fn auction_invalid_transitions_preserve_candidate_parent() {
    let (mut book, id) = owner_book(400);
    book.record_child_submitted(AccountId(1), &StockId(0), Side::Buy, crate::OrderId(1), 100, 0).unwrap();
    let before = book.clone();
    assert!(book.record_child_submitted(AccountId(1), &StockId(0), Side::Buy, crate::OrderId(2), 100, 0).is_err());
    assert!(book.record_child_fill(AccountId(1), &StockId(0), Side::Buy, crate::OrderId(1), 101, 0).is_err());
    assert_eq!(book, before);
    book.record_child_released(AccountId(1), &StockId(0), crate::OrderId(1), 0).unwrap();
    assert_eq!(book.plan(id).unwrap().filled_qty(), 0);
    assert_eq!(book.plan(id).unwrap().active_child_order_id(), None);
}

#[test]
fn continuous_assert_and_auction_checked_fill_keep_distinct_failure_boundaries() {
    // 两种撮合事实最终都经过同一 owner；非法 fill 必须显式失败且不污染进度。
    for target in [100, 400] {
        let (mut book, id) = owner_book(target);
        book.record_child_submitted(AccountId(1), &StockId(0), Side::Buy, crate::OrderId(1), 100, 0).unwrap();
        let before = book.clone();
        assert!(book.record_child_fill(AccountId(1), &StockId(0), Side::Buy, crate::OrderId(1), 101, 0).is_err());
        assert_eq!(book, before);
        assert_eq!(book.plan(id).unwrap().filled_qty(), 0);
        assert_eq!(book.plan(id).unwrap().active_child_remaining_qty(), Some(100));
    }
}

#[test]
fn unlinked_parent_reverse_rebuilds_execution_from_the_new_target() {
    let (mut book, id) = owner_book(400);
    book.record_child_submitted(AccountId(1), &StockId(0), Side::Buy, crate::OrderId(17), 100, 0).unwrap();
    book.record_child_fill(AccountId(1), &StockId(0), Side::Buy, crate::OrderId(17), 40, 0).unwrap();
    book.record_child_released(AccountId(1), &StockId(0), crate::OrderId(17), 0).unwrap();
    book.apply(id, crate::plans::PlanEvent::Revised { revision: crate::plans::PlanRevision {
        reason: crate::plans::RevisionReason::SignalShift, trading_day: 0,
        direction: Side::Sell, target: PlanTarget::ShareCount(800),
        opinion: PlanOpinion { signal_score_bp: -3000, source: crate::plans::OpinionSource::Blended },
        confidence_bp: 8000, urgency: Urgency::Normal, below_filled_rationale: None,
    }}).unwrap();
    book.install_execution(id, 200, Money::from_cents(980), 247).unwrap();
    let owner = book.plan(id).unwrap();
    assert_eq!(owner.direction(), Side::Sell);
    assert_eq!(owner.target(), PlanTarget::ShareCount(800));
    assert_eq!(owner.filled_qty(), 0);
    assert_eq!(owner.active_child_order_id(), None);
    let leg = owner.execution().unwrap();
    assert_eq!(leg.child_qty(), 200);
    assert_eq!(leg.limit_price(), Money::from_cents(980));
    assert_eq!(leg.expires_market_minute(), 247);
    assert_eq!(owner.plan_id(), id, "反向修订仍由原 owner 管理，不创建另一份执行计划");
}

fn assert_sub_lot_is_not_overbought() {
    let (mut authority, id, code) = live_buy_plan_case(350, 350);
    let child = authority.state.plans.plan(id).unwrap().active_child_order_id().unwrap();
    confirm_cancel_tick(&mut authority, AccountId(1), code, child);
    let before = authority.state.plans.clone();
    let mut roots = PlanChainOperationBatch::empty();
    roots.push_execution(submit_request(&authority, id, code, 1010, 100));
    let mut tick = plan_tick(PhaseInput::of(&authority)).unwrap();
    let output = apply_tick_shadow_market_transaction_with_roots_for_test(&mut tick, roots).unwrap();
    assert!(output.plan_reports.iter().any(|report| matches!(report.disposition,
        PlanExecutionDisposition::RemainingBelowBoardLot { remaining_qty: 50 })));
    super::candidate_commit::prepare_tick_shadow_plan_commit(&mut authority, tick).unwrap().commit();
    let owner = authority.state.plans.plan(id).unwrap();
    assert_eq!(owner.filled_qty(), 350);
    assert_eq!(owner.remaining_share_qty(), Some(50));
    assert_eq!(owner.target(), PlanTarget::ShareCount(400));
    assert!(authority.state.markets[&code].resting_orders_for(AccountId(1)).is_empty());
    assert_eq!(authority.state.accounts[&AccountId(1)].positions()[&code].qty(), 350);
    assert_eq!(before.plan(id).unwrap().filled_qty(), 350);
}

#[test]
fn parent_buy_does_not_overbuy_when_an_odd_lot_partial_fill_leaves_a_sub_lot_target() {
    assert_sub_lot_is_not_overbought();
}

#[test]
fn same_side_reprice_replaces_the_leg_and_keeps_progress_and_target() {
    // 生产同向再报价走 QuoteAction::Replace → 撤旧子单 → 新 submit →
    // install_execution 替换执行腿：限价与子单规模更新，进度与目标保留。
    let (mut authority, id, code) = live_buy_plan_case(100, 200);
    let child = authority.state.plans.plan(id).unwrap().active_child_order_id().unwrap();
    confirm_cancel_tick(&mut authority, AccountId(1), code.clone(), child);
    let mut plans = std::mem::take(&mut authority.state.plans);
    plans.install_execution(id, 100, Money::from_cents(1_010), 240).unwrap();
    // 同向（Buy）替换成功：限价更新、进度与目标保持、剩余目标不受影响。
    let leg = plans.plan(id).unwrap().execution().unwrap();
    assert_eq!(leg.limit_price(), Money::from_cents(1_010));
    assert_eq!(leg.child_qty(), 100);
    assert_eq!(plans.plan(id).unwrap().filled_qty(), 100);
    assert_eq!(plans.plan(id).unwrap().target(), PlanTarget::ShareCount(400));
    // 反向（Sell）提交不采纳为子单：owner 状态不变（旧 revise 反向拒绝语义）。
    let before = plans.clone();
    assert!(!plans
        .record_child_submitted(AccountId(1), &code, Side::Sell, crate::OrderId(77), 100, 0)
        .unwrap());
    assert_eq!(plans, before);
    authority.state.plans = plans;
    // 再次同向替换限价：进度仍保留（同向修订不重置执行进度）。
    let mut plans = std::mem::take(&mut authority.state.plans);
    plans.install_execution(id, 100, Money::from_cents(1_020), 240).unwrap();
    assert_eq!(plans.plan(id).unwrap().execution().unwrap().limit_price(), Money::from_cents(1_020));
    assert_eq!(plans.plan(id).unwrap().filled_qty(), 100);
    authority.state.plans = plans;
}

#[test]
fn closing_auction_does_not_duplicate_a_parent_child_still_resting_in_continuous_book() {
    let (mut authority, id, child) = closing_child();
    let mut roots = PlanChainOperationBatch::empty();
    roots.push_execution(submit_request(&authority, id, StockId(0), 1000, 100));
    let events = commit_roots(&mut authority, roots);
    assert!(!events.iter().any(|event| matches!(event, Event::OrderAccepted { .. })));
    assert!(events.iter().any(|event| matches!(event, Event::OrderCanceled { id, remaining_qty: 100, .. } if *id == child)));
    assert_eq!(authority.state.plans.plan(id).unwrap().filled_qty(), 0);
    assert_eq!(authority.state.plans.plan(id).unwrap().active_child_order_id(), None);
    // auction_orders 状态快照随统一路径删除（owner8 合并树无此字段）；等价覆盖=上方腿清空断言。
}

#[test]
fn closing_auction_restores_a_parent_child_that_remains_in_continuous_book() {
    let (authority, id, child) = closing_child();
    let saved = authority.save().unwrap();
    let restored = GameSession::restore(&saved).unwrap();
    assert_eq!(restored.phase(), crate::TradingPhase::ClosingAuction);
    assert_eq!(restored.state.plans.plan(id).unwrap().active_child_order_id(), Some(child));
    assert_eq!(restored.state.plans.plan(id).unwrap().active_child_remaining_qty(), Some(100));
    assert_eq!(restored.state.plans, authority.state.plans);
    assert_eq!(restored.reserved_cash_for_account(AccountId(1)).unwrap(), authority.reserved_cash_for_account(AccountId(1)).unwrap());
}

#[test]
fn closing_auction_never_duplicates_a_live_continuous_child_and_expiry_stops_intents() {
    let (mut authority, id, _) = closing_child();
    let before_next = authority.state.next_order_id;
    let mut roots = PlanChainOperationBatch::empty();
    roots.push_execution(submit_request(&authority, id, StockId(0), 1000, 100));
    let events = commit_roots(&mut authority, roots);
    assert!(!events.iter().any(|event| matches!(event, Event::OrderAccepted { .. })));
    assert_eq!(authority.state.next_order_id, before_next);
    assert!(authority.state.plans.plan(id).unwrap().execution().is_none());
    assert_eq!(authority.state.plans.plan(id).unwrap().filled_qty(), 0);
}

fn ordinary_quotes(with_old_owner: bool, prices: &[crate::LimitPrice]) {
    let mut authority = GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 47).unwrap();
    authority.state.attention_scheduler.clear();
    authority.state.npc_attention.get_mut(&AccountId(1)).unwrap().next_attention_candidate_tick = u64::MAX;
    let existing = if with_old_owner {
        let id = open_buy_plan(&mut authority, AccountId(1), StockId(0), 400);
        authority.state.plans.install_execution(id, 100, Money::from_cents(950), 240).unwrap();
        Some((id, authority.state.plans.clone()))
    } else { None };
    let received = prices.iter().map(|price| authority.receive_private_intent(AccountId(1), Intent::PlaceLimit {
        code: StockId(0), side: Side::Buy, price: *price, qty: 100,
    }).unwrap()).collect();
    crate::session::pipeline::staged_decision::stage(crate::session::pipeline::NpcDecisionHandover::new(authority.state.tick, Vec::new(), received, Vec::new()));
    let mut tick = plan_tick(PhaseInput::of(&authority)).unwrap();
    let output = apply_tick_shadow_market_transaction_with_roots_for_test(&mut tick, PlanChainOperationBatch::empty()).unwrap();
    let submitted = output.candidates.candidates().iter().filter(|candidate| matches!(candidate.intent(), Intent::PlaceLimit { .. })).count();
    assert_eq!(submitted, prices.len(), "固定与符号报价均须进入权威校验，不能被旧母单适配器吞掉");
    super::candidate_commit::prepare_tick_shadow_plan_commit(&mut authority, tick).unwrap().commit();
    if let Some((id, _before)) = existing {
        // 与旧镜像一致：计划账户/股票/方向的已接受 Buy 委托被唯一 owner 采纳，
        // 但受理本身绝不推进成交进度。
        let owner = authority.state.plans.plan(id).unwrap();
        assert_eq!(owner.filled_qty(), 0, "受理不推进成交");
    } else {
        assert!(authority.state.plans.plan_ids().next().is_none());
    }
}

fn submit_request(
    authority: &GameSession,
    plan_id: crate::plans::PlanId,
    code: StockId,
    price: i64,
    qty: u32,
) -> PlanExecutionRequest {
    PlanExecutionRequest {
        plan_id,
        allocation: AllocationGrant {
            plan_id,
            code,
            allocated_cash: Money::from_cents(1_000_000),
            constraint: None,
        },
        decision: QuoteDecision {
            action: QuoteAction::Submit {
                price: Money::from_cents(price),
                qty,
            },
            reason: QuoteReason::OppositeQuoteProbe,
        },
        trading_day: u64::from(authority.state.day),
    }
}

/// 场景一（红）：真实部分成交后的在途子单被已确认撤销（生产上由 NPC 对账撤单、
/// ExpiryShadow 等非计划路由触发，均收敛到同一确认撤销副作用）取消后，计划本体的
/// 子单引用必须同步清除，且计划可以立即重新报价推进剩余目标。
#[test]
fn confirmed_cancel_clears_the_plan_child_and_allows_resubmission() {
    let (mut authority, plan_id, code) = live_buy_plan_case(100, 200);
    let owner = AccountId(1);
    assert_eq!(authority.state.plans.plan(plan_id).unwrap().filled_qty(), 100);
    let child_id = authority
        .state
        .plans
        .plan(plan_id)
        .unwrap()
        .active_child_order_id()
        .expect("partial fill leaves the child in flight");

    confirm_cancel_tick(&mut authority, owner, code, child_id);

    // 唯一 owner 语义：撤销事实回写计划本体，不留漂移引用。
    let plan = authority.state.plans.plan(plan_id).unwrap();
    assert_eq!(
        plan.active_child_order_id(),
        None,
        "a confirmed cancel must clear the plan-owned child reference"
    );
    assert_eq!(plan.filled_qty(), 100, "cancellation keeps real fill progress");
    assert_eq!(authority.state.markets[&code].resting_order_count(), 0);

    // 重试：撤销后的计划报价必须能继续推进剩余目标，而不是致命失败。
    let request = submit_request(&authority, plan_id, code.clone(), 1_000, 100);
    let mut roots = PlanChainOperationBatch::empty();
    roots.push_execution(request);
    let mut tick = plan_tick(PhaseInput { session: &authority })
    .unwrap();
    let output =
        apply_tick_shadow_market_transaction_with_roots_for_test(&mut tick, roots)
            .expect("the plan must resubmit after a confirmed cancel");
    assert!(
        output.plan_reports.iter().any(|report| matches!(
            report.disposition,
            PlanExecutionDisposition::Submitted { .. }
        )),
        "resubmission must succeed: {:?}",
        output.plan_reports
    );
}

/// 场景二（红）：跨股多计划资源依赖。同一账户两只股票各持一个买入计划，共享
/// 账户现金预算；撤销股票 A 的子单必须只影响 A 的计划与 A 的现金预留，股票 B
/// 的计划子单与预留保持不变，并继续按真实成交推进。
#[test]
fn cross_stock_plans_keep_independent_children_and_escrow_after_a_cancel() {
    let mut authority = GameSession::new(two_stock_quote_setup(), 47).unwrap();
    authority
        .state
        .npc_attention
        .get_mut(&AccountId(1))
        .unwrap()
        .next_attention_candidate_tick = u64::MAX;
    authority.state.attention_scheduler.clear();
    authority.state.plans = PlanBook::default();
    let plan_a = open_buy_plan(&mut authority, AccountId(1), StockId(0), 400);
    let plan_b = open_buy_plan(&mut authority, AccountId(1), StockId(1), 400);

    let mut roots = PlanChainOperationBatch::empty();
    roots.push_execution(submit_request(&authority, plan_a, StockId(0), 1_000, 400));
    roots.push_execution(submit_request(&authority, plan_b, StockId(1), 1_000, 400));
    let mut tick = plan_tick(PhaseInput { session: &authority })
    .unwrap();
    let output = apply_tick_shadow_market_transaction_with_roots_for_test(&mut tick, roots)
        .expect("both plan children submit");
    assert_eq!(output.plan_reports.len(), 2);
    super::candidate_commit::prepare_tick_shadow_plan_commit(&mut authority, tick)
        .unwrap()
        .commit();

    let child_a = authority
        .state
        .plans
        .plan(plan_a)
        .unwrap()
        .active_child_order_id()
        .expect("plan A child in flight");
    let child_b = authority
        .state
        .plans
        .plan(plan_b)
        .unwrap()
        .active_child_order_id()
        .expect("plan B child in flight");
    let reserved_both = authority.reserved_cash_for_account(AccountId(1)).unwrap();
    assert!(reserved_both > Money::ZERO, "two buy children both escrow cash");

    // 已确认撤销只针对股票 A 的子单。
    confirm_cancel_tick(&mut authority, AccountId(1), StockId(0), child_a);

    let plan_a_now = authority.state.plans.plan(plan_a).unwrap();
    assert_eq!(
        plan_a_now.active_child_order_id(),
        None,
        "canceled child must clear plan A's reference"
    );
    assert_eq!(plan_a_now.filled_qty(), 0);
    // 股票 B 的计划子单不受影响。
    assert_eq!(
        authority.state.plans.plan(plan_b).unwrap().active_child_order_id(),
        Some(child_b),
        "plan B's child must stay in flight"
    );
    let reserved_after = authority.reserved_cash_for_account(AccountId(1)).unwrap();
    assert!(
        reserved_after < reserved_both,
        "cancel must release plan A's escrow"
    );
    assert!(reserved_after > Money::ZERO, "plan B's escrow must survive");

    // 股票 B 继续按真实成交推进（玩家对手盘卖出）。
    authority
        .fixture_account_mut(&AccountId(0))
        .unwrap()
        .grant_position(StockId(1), 100, Money::from_cents(1_000))
        .unwrap();
    authority
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: StockId(1),
                side: Side::Sell,
                price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
                qty: 100,
            },
        )
        .unwrap();
    let events = authority.step().expect("healthy step fills plan B's child");
    // 腿方向语义（2026-10-16 契约）：计划子单=买腿=taker。
    assert!(
        events.iter().any(|event| matches!(
            event,
            Event::Trade { code: traded, qty: 100, taker: AccountId(1), .. } if *traded == StockId(1)
        )),
        "plan B's child must really fill: {events:?}"
    );
    assert_eq!(
        authority.state.plans.plan(plan_b).unwrap().filled_qty(),
        100,
        "plan B advances by the real fill"
    );
    assert_eq!(
        authority.state.plans.plan(plan_a).unwrap().filled_qty(),
        0,
        "plan A stays untouched by the other stock's fill"
    );
}

/// 场景三（红）：存档恢复。撤销后的计划状态进入存档再恢复，不得复活已撤销的
/// 子单引用；恢复会话的计划执行子状态与真实订单簿一致，并能继续推进。
#[test]
fn save_restore_never_resurrects_a_canceled_plan_child() {
    let (mut authority, plan_id, code) = live_buy_plan_case(100, 200);
    let owner = AccountId(1);
    let child_id = authority
        .state
        .plans
        .plan(plan_id)
        .unwrap()
        .active_child_order_id()
        .expect("child in flight");

    // 撤销前存档恢复：唯一 owner 的执行子状态与真实订单簿一致。
    let saved = authority.save().expect("healthy save");
    let restored = GameSession::restore(&saved).expect("healthy restore");
    assert_eq!(
        restored.state.plans.plan(plan_id).unwrap().active_child_order_id(),
        Some(child_id)
    );
    assert_eq!(
        restored.state.plans.plan(plan_id).unwrap().filled_qty(),
        100
    );

    confirm_cancel_tick(&mut authority, owner, code, child_id);
    assert_eq!(
        authority.state.plans.plan(plan_id).unwrap().active_child_order_id(),
        None
    );

    let saved_after_cancel = authority.save().expect("healthy save after cancel");
    let restored_after_cancel = GameSession::restore(&saved_after_cancel).unwrap();
    assert_eq!(
        restored_after_cancel
            .state
            .plans
            .plan(plan_id)
            .unwrap()
            .active_child_order_id(),
        None,
        "restore must not resurrect a canceled child reference"
    );
    assert_eq!(
        restored_after_cancel.state.plans.plan(plan_id).unwrap().filled_qty(),
        100
    );
}

// ===== Astra 终审 P1：采纳入口上限防御（红→绿三形态） =====

/// 接线增量八遗留的未调用 helper：固定/符号报价在有/无旧 owner 计划时都必须进入权威校验。
#[test]
fn ordinary_quotes_reach_authority_with_and_without_old_owner() {
    ordinary_quotes(false, &[crate::strategy::LimitPrice::Fixed(Money::from_cents(1_000))]);
    ordinary_quotes(true, &[crate::strategy::LimitPrice::Fixed(Money::from_cents(1_000))]);
}

/// Astra 反例形状：target=400 / filled=100 / 子单已撤、execution 保留——仅剩 300 额度。
fn adopted_plan_with_released_child(authority: &mut GameSession) -> crate::plans::PlanId {
    let id = open_buy_plan(authority, AccountId(1), StockId(0), 400);
    authority
        .state
        .plans
        .install_execution(id, 100, Money::from_cents(950), 240)
        .unwrap();
    authority
        .state
        .plans
        .record_child_submitted(
            AccountId(1),
            &StockId(0),
            Side::Buy,
            crate::OrderId(1),
            100,
            u64::from(authority.state.day),
        )
        .unwrap();
    authority
        .record_parent_order_fills(
            &StockId(0),
            &[crate::session::OrderFillSettlement {
                account: AccountId(1),
                side: Side::Buy,
                order_id: crate::OrderId(1),
                qty: 100,
            }],
        )
        .unwrap();
    authority
        .record_parent_order_canceled(AccountId(1), &StockId(0), crate::OrderId(1))
        .unwrap();
    id
}

/// 挂起一张普通来源 Buy 委托（走公开 step 路径，与 quote_expiry 失败注入测试同款驱动）。
fn enqueue_ordinary_buy(authority: &mut GameSession, qty: u32) {
    authority.state.attention_scheduler.clear();
    authority
        .state
        .npc_attention
        .get_mut(&AccountId(1))
        .unwrap()
        .next_attention_candidate_tick = u64::MAX;
    let received = authority
        .receive_private_intent(
            AccountId(1),
            Intent::PlaceLimit {
                code: StockId(0),
                side: Side::Buy,
                price: crate::strategy::LimitPrice::Fixed(Money::from_cents(1_000)),
                qty,
            },
        )
        .unwrap();
    crate::session::pipeline::staged_decision::stage(crate::session::pipeline::NpcDecisionHandover::new(authority.state.tick, Vec::new(), vec![received], Vec::new()));
}

/// 形态 1（红→绿）：普通来源超剩余数量必须拒绝——step 失败、不采纳、计划无残留。
#[test]
fn ordinary_adoption_rejects_exceeding_remaining_quantity() {
    let mut authority = GameSession::new(
        crate::session::npc_working_quote_tests::quote_setup(0),
        49,
    )
    .unwrap();
    let id = adopted_plan_with_released_child(&mut authority);
    let before = authority.state.plans.clone();
    enqueue_ordinary_buy(&mut authority, 400);
    assert!(
        authority.step().is_err(),
        "仅剩 300 额度的计划不得采纳普通来源 400 股委托（采纳入口上限防御）"
    );
    let plan = authority.state.plans.plan(id).unwrap();
    assert_eq!(plan.active_child_order_id(), None, "失败的采纳不得残留");
    assert_eq!(authority.state.plans, before);
}

/// 形态 2（守卫）：正常范围内放行——采纳成功且受理不推进成交进度。
#[test]
fn ordinary_adoption_within_remaining_passes() {
    let mut authority = GameSession::new(
        crate::session::npc_working_quote_tests::quote_setup(0),
        53,
    )
    .unwrap();
    let id = adopted_plan_with_released_child(&mut authority);
    enqueue_ordinary_buy(&mut authority, 300);
    assert!(authority.step().is_ok());
    let plan = authority.state.plans.plan(id).unwrap();
    assert_eq!(plan.filled_qty(), 100, "受理不推进成交");
    assert!(plan.active_child_order_id().is_some(), "范围内应被采纳");
}

/// 形态 3（守卫）：candidate 提交失败整体回滚，采纳不残留到 authority。
#[test]
fn failed_candidate_rolls_back_adoption_without_residue() {
    let mut authority = GameSession::new(
        crate::session::npc_working_quote_tests::quote_setup(0),
        59,
    )
    .unwrap();
    let id = adopted_plan_with_released_child(&mut authority);
    let before = authority.state.plans.clone();
    enqueue_ordinary_buy(&mut authority, 300);
    authority.inject_post_shadow_failure(StepFatal::InvariantViolation {
        description: "controlled post-shadow failure".to_owned(),
        location: "plan_owner_tests".to_owned(),
    });
    assert!(authority.step().is_err(), "注入失败必须使提交失败");
    assert_eq!(
        authority.state.plans, before,
        "失败的候选不得把采纳残留到 authority"
    );
    assert_eq!(
        authority.state.plans.plan(id).unwrap().active_child_order_id(),
        None
    );
}
