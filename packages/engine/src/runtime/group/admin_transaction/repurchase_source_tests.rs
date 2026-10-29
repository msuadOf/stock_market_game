//! 真实 Group consumer 首红：原 Admin 分配身份、原 Desk 产来源；不证明 request producer。
//! 在途资源与冻结 RequestKey 是明确种入的 consumer 前置，不证明生产 request sender。
use super::*;
use crate::company::{CompanyId, CompanyKind, CompanyReadFacts};
use crate::plans::{PlanId, RepurchaseExecutionStatus};
use crate::runtime::desk::repurchase_group_fixture::SourceDesk;
use crate::runtime::protocol::{
    AdminPlanReference, AdminTreasuryBinding, RepurchaseExecutionProjection,
    RepurchaseMaterialRestriction, RepurchaseProjectionInput,
};
use crate::runtime::{RepurchaseRequestKey, RepurchaseSourceKind};
use crate::{AccountId, StockId};
#[path = "../../information_fixture.rs"]
mod fixture;

const FUNDING: i64 = 1_000_000;
const ACCOUNT: AccountId = AccountId(79);
const STOCK: StockIdx = 3;

fn source() -> Arc<crate::session::market_calendar_context::MarketCalendarSource> {
    static SOURCE: std::sync::OnceLock<
        Arc<crate::session::market_calendar_context::MarketCalendarSource>,
    > = std::sync::OnceLock::new();
    Arc::clone(SOURCE.get_or_init(|| {
        crate::session::market_calendar_context::MarketCalendarSource::new(
            crate::TradingCalendar::current_default_calendar().unwrap(),
            crate::CivilDate::from_iso("2026-01-05").unwrap(),
            bindings(),
        )
        .unwrap()
    }))
}

fn bindings() -> Vec<(StockId, crate::calendar::CalendarExchange)> {
    (0..4)
        .map(|stock| (StockId(stock), crate::calendar::CalendarExchange::Szse))
        .collect()
}

fn state(price: i64, qty: u32) -> (GroupState, SourceDesk, RepurchaseRequestKey, i64) {
    let config = Arc::new(
        GameConfig::new(
            0.000_25,
            Money::from_cents(500),
            0.000_5,
            0.10,
            0.10,
            false,
            100,
            Money::from_cents(1_000_000_000_000),
        )
        .unwrap(),
    );
    let date = crate::CivilDate::from_iso("2026-01-05").unwrap();
    let calendar = source()
        .day(date)
        .unwrap()
        .tick(
            crate::session::exchange_calendar_model::SessionTickLayout {
                ticks_per_day: 100,
                auction_ticks: 0,
                closing_auction_ticks: 0,
            },
            crate::session::exchange_calendar_model::SharedClockInput {
                tick: 0,
                completed_shared_sessions: 0,
                progress: crate::session::exchange_calendar_model::SharedDayProgress::Trading {
                    completed_ticks: 0,
                },
            },
        )
        .unwrap();
    let desk = SourceDesk::new(Arc::clone(&config), Arc::clone(&calendar));
    let mut state = GroupState::new(
        1,
        vec![GroupAccount::new(0, 4); 2],
        8,
        100,
        Arc::clone(&config),
        100,
        None,
        Vec::new(),
        Box::new(|_, _| TickPlan::default()),
    );
    let facts = Arc::new(
        CompanyReadFacts::new(
            (0..4)
                .map(|stock| (StockId(stock), CompanyId(stock)))
                .collect(),
            (0..4)
                .map(|stock| (CompanyId(stock), (CompanyKind::Industrial, 10_000)))
                .collect(),
        )
        .unwrap(),
    );
    let mut root = fixture::projection([11, 37, 51, 79, 113].map(AccountId).to_vec());
    let mutable = Arc::get_mut(&mut root).expect("新建 RootProjection 尚未共享");
    mutable.catalog = crate::stock::StockCatalog::from_metas(
        ["000001", "000002", "000003", "000004"]
            .into_iter()
            .map(crate::stock::StockMeta::new)
            .collect(),
    )
    .unwrap();
    assert_eq!(mutable.catalog.code(StockId(3)), Some("000004"));
    mutable.initial_company_facts = Arc::clone(&facts);
    state.root_projection = Some(root);
    state.roster_offset = 2;
    state.bind_calendar(source(), bindings());
    state.install_initial_company_facts(facts).unwrap();
    state.admin_treasury_bindings = vec![AdminTreasuryBinding {
        owner: (1, 1),
        account: ACCOUNT,
        issuer: CompanyId(3),
        stock: STOCK,
    }];
    let reference = AdminPlanReference::CreatedInThisPrepare { operation_index: 0 };
    let prepared = state
        .prepare_admin_scope(
            AdminTransactionId(1),
            vec![
                AdminOperation::OpenIssuerRepurchase {
                    local: 1,
                    stock: STOCK,
                    issuer: CompanyId(3),
                },
                AdminOperation::SyntheticBudgetCredit {
                    local: 1,
                    stock: STOCK,
                    execution_plan: reference,
                    amount: FUNDING,
                },
            ],
            None,
            Some(vec![RepurchaseProjectionInput {
                local: 1,
                stock: STOCK,
                issuer: CompanyId(3),
                execution_plan: reference,
                projection: RepurchaseExecutionProjection {
                    account: ACCOUNT,
                    issuer: CompanyId(3),
                    stock: STOCK,
                    announced: false,
                    announced_on: date,
                    window_start_on: date,
                    window_deadline_on: date,
                    exchange: crate::calendar::CalendarExchange::Szse,
                    purpose: crate::company::issuer_repurchase::RepurchasePurpose::ReduceCapital,
                    funding_amount: Money::from_cents(FUNDING),
                    price_cap: Money::from_cents(1100),
                    max_shares: 1000,
                    unit_as_of: date,
                    unit_revision: 0,
                    book_through_cursor: 0,
                    book_gross: Money::ZERO,
                    book_fees: Money::ZERO,
                    recycled: Money::ZERO,
                    inventory_shares: 0,
                    last_order_day: None,
                    eligibility_revision: 0,
                    eligibility_as_of: crate::CivilInstant::from_hms(date, 9, 0, 0).unwrap(),
                    issuance_in_progress: None,
                    material_restriction: RepurchaseMaterialRestriction::NotModelled,
                },
            }]),
        )
        .unwrap();
    let id = prepared.repurchase.unwrap().created[0].plan_id;
    state
        .complete_admin(AdminTransactionId(1), AdminPhase::Commit)
        .unwrap();
    assert!(state.admin_pending.is_none());
    assert_eq!(state.accounts[1].cash, FUNDING);
    assert_eq!(id.owner, ACCOUNT);
    let crate::plans::PlanEntry::IssuerRepurchase(canonical) = state.plan_book.entry(id).unwrap()
    else {
        panic!("真实批准必须创建公司来源条目");
    };
    assert_eq!(
        (canonical.stock(), canonical.issuer()),
        (StockId(3), CompanyId(3))
    );
    assert_eq!(
        state
            .plan_book
            .active_entry(ACCOUNT, &StockId(3))
            .unwrap()
            .plan_id(),
        id
    );
    let requested_trading_day = calendar
        .before_projection()
        .stock(StockId(3))
        .unwrap()
        .date_session_ordinal()
        .unwrap();
    let request = RepurchaseRequestKey {
        plan_id: id,
        stock: StockId(3),
        requested_trading_day,
    };
    let purse = buy_purse_limit(&config, Money::from_cents(price), qty, Money::ZERO)
        .unwrap()
        .cents();
    assert!(purse > 0 && purse <= state.accounts[1].cash);
    // 只种入合法已发送前置；公告与资格仍是批准基线，不据此声称生产能发单。
    let account = Arc::make_mut(&mut state.accounts[1]);
    account.cash -= purse;
    account.buy_purse_held_cents = purse;
    account.foreign_active[3] = 1;
    state.child_in_flight.insert(
        (ACCOUNT, StockId(3)),
        ChildTicket {
            qty,
            side: Side::Buy,
            price,
            order_id: None,
            remaining: qty,
            charged_cents: 0,
            repurchase: Some(RepurchaseChildSource {
                request_key: request,
                purse_initial: purse,
                returned_cumulative: 0,
                phase: RepurchaseChildPhase::Submitted,
                last_progress_position: None,
            }),
        },
    );
    state.cur_tick = 1;
    state.snapshot = Some(Arc::new(TickSnapshot {
        calendar,
        tick: 1,
        market_minute: 0,
        civil_date: date,
        stocks: Arc::new(vec![desk.public(); 4]),
    }));
    (state, desk, request, purse)
}

fn consume(state: &mut GroupState, batch: DeskBatch) {
    let result = state.apply_batch_immediate(batch, 1, 0);
    assert!(
        result.is_ok(),
        "真实 Desk Batch 必须进入 Group consumer：{result:?}"
    );
    state
        .apply_deferred()
        .expect("公司来源不能沿普通 Investor 路径拒绝或提前清票");
}

fn assert_funded(state: &GroupState, id: PlanId) {
    let crate::plans::PlanEntry::IssuerRepurchase(plan) = state.plan_book.entry(id).unwrap() else {
        panic!("原公司授权来源必须保留");
    };
    assert_eq!(
        plan.status(),
        RepurchaseExecutionStatus::Funded,
        "真实订单终结不能代替公司计划 Close"
    );
}

#[test]
fn repurchase_x_group_01_source_only_acceptance_binds_real_order_and_abort_restores_pending() {
    let (mut state, mut desk, request, purse) = state(1000, 100);
    let before = state.state_hash();
    let accounts = state.accounts.clone();
    let book = state.plan_book.clone();
    desk.company_buy(request, 1000, 100, purse);
    let batch = desk.flush_company();
    assert!(batch.entities.is_empty() && batch.fills.is_empty() && batch.receipts.is_empty());
    assert_eq!(batch.repurchase.len(), 1);
    let order = batch.repurchase[0].order_ref;
    assert!(matches!(
        batch.repurchase[0].kind,
        RepurchaseSourceKind::Accepted { .. }
    ));
    for _ in 0..2 {
        state.cur_tick = 1;
        consume(&mut state, batch.clone());
        assert_eq!(state.accounts, accounts, "来源确认不能第二次扣原 purse");
        let ticket = &state.child_in_flight[&(ACCOUNT, StockId(3))];
        assert_eq!(ticket.order_id, Some(order_id_of(&order)));
        assert_eq!((ticket.remaining, ticket.charged_cents), (100, 0));
        let tail = &state.repurchase_pending_tails[&request.plan_id];
        assert_eq!(tail.cursor, 1);
        assert_eq!(tail.facts.len(), 1);
        assert_eq!((tail.facts[0].cursor, tail.facts[0].tick, tail.facts[0].round), (1, 1, 0));
        assert_eq!(tail.facts[0].source, batch.repurchase[0]);
        assert_ne!(state.state_hash(), before, "真实受理身份必须进入业务 hash");
        assert_funded(&state, request.plan_id);
        state.rollback();
        assert_eq!(state.accounts, accounts);
        assert_eq!(state.plan_book, book);
        assert_eq!(state.state_hash(), before);
        assert!(state.child_in_flight[&(ACCOUNT, StockId(3))]
            .order_id
            .is_none());
        assert!(state.repurchase_pending_tails.is_empty());
    }
}

#[test]
fn repurchase_x_group_02_real_partial_fill_price_return_and_cancel_apply_resources_once() {
    let (mut state, mut desk, request, purse) = state(1090, 200);
    let before = state.state_hash();
    let accounts = state.accounts.clone();
    let book = state.plan_book.clone();
    desk.sell(1000, 100);
    desk.company_buy(request, 1090, 200, purse);
    let batch = desk.flush_company();
    assert_eq!(batch.fills.len(), 1);
    let fill = batch.fills[0];
    let gross = fill.price * i64::from(fill.qty);
    let charged = gross + fill.commission + fill.transfer_fee;
    let returned: i64 = batch
        .entities
        .iter()
        .filter_map(|entity| match entity {
            Entity::CashRefund { amount, .. } => Some(*amount),
            _ => None,
        })
        .sum();
    assert!(returned > 0 && purse - charged - returned > 0);
    assert!(batch
        .repurchase
        .iter()
        .any(|fact| matches!(fact.kind, RepurchaseSourceKind::Return { .. })));
    consume(&mut state, batch);
    let account = &state.accounts[1];
    assert_eq!(
        (account.cash, account.buy_purse_held_cents),
        (FUNDING - purse + returned, purse - charged - returned)
    );
    assert_eq!(
        (
            account.shares[3],
            account.frozen_today[3],
            account.invested_cents[3]
        ),
        (100, 100, gross)
    );
    assert_eq!(account.foreign_active[3], 1);
    assert_eq!(state.child_in_flight[&(ACCOUNT, StockId(3))].remaining, 100);
    desk.cancel_company();
    let terminal = desk.flush_company();
    assert!(terminal.fills.is_empty());
    consume(&mut state, terminal);
    let account = &state.accounts[1];
    assert_eq!(
        (
            account.cash,
            account.buy_purse_held_cents,
            account.foreign_active[3]
        ),
        (FUNDING - charged, 0, 0)
    );
    assert_eq!(
        (
            account.shares[3],
            account.frozen_today[3],
            account.invested_cents[3]
        ),
        (100, 100, gross)
    );
    assert!(!state.child_in_flight.contains_key(&(ACCOUNT, StockId(3))));
    assert_funded(&state, request.plan_id);
    assert_eq!(state.accounts[0], accounts[0]);
    state.rollback();
    assert_eq!(state.accounts, accounts);
    assert_eq!(state.plan_book, book);
    assert_eq!(state.state_hash(), before);
}

#[test]
fn repurchase_x_group_03_full_maker_zero_held_waits_for_actual_closed_suffix() {
    let (mut state, mut desk, request, purse) = state(1000, 100);
    let before = state.state_hash();
    let accounts = state.accounts.clone();
    let book = state.plan_book.clone();
    desk.company_buy(request, 1000, 100, purse);
    consume(&mut state, desk.flush_company());
    desk.sell(1000, 100);
    let mut prefix = desk.flush_company();
    assert_eq!(prefix.repurchase.len(), 2);
    assert!(matches!(
        prefix.repurchase[0].kind,
        RepurchaseSourceKind::Fill(_)
    ));
    assert_eq!(prefix.repurchase[1].kind, RepurchaseSourceKind::Closed);
    // 仅延迟真实 Closed 后缀；成交、费用和退款一律不伪造或重算。
    let closed_source = prefix.repurchase.pop().unwrap();
    let at = prefix
        .entities
        .iter()
        .position(|entity| matches!(entity, Entity::Closed { .. }))
        .unwrap();
    let closed_entity = prefix.entities.remove(at);
    let suffix = DeskBatch {
        stock: prefix.stock,
        tick: prefix.tick,
        round: prefix.round,
        repurchase: vec![closed_source],
        entities: vec![closed_entity],
        ..Default::default()
    };
    consume(&mut state, prefix);
    assert_eq!(state.accounts[1].buy_purse_held_cents, 0);
    assert_eq!(state.accounts[1].foreign_active[3], 1);
    assert_eq!(
        state.child_in_flight[&(ACCOUNT, StockId(3))].remaining,
        0,
        "全成与 H=0 都不能提前删除等待真实 Closed 的来源票"
    );
    consume(&mut state, suffix);
    assert!(!state.child_in_flight.contains_key(&(ACCOUNT, StockId(3))));
    assert_eq!(
        (
            state.accounts[1].cash,
            state.accounts[1].buy_purse_held_cents,
            state.accounts[1].foreign_active[3]
        ),
        (FUNDING - purse, 0, 0)
    );
    assert_eq!(state.accounts[1].shares[3], 100);
    assert_funded(&state, request.plan_id);
    state.rollback();
    assert_eq!(state.accounts, accounts);
    assert_eq!(state.plan_book, book);
    assert_eq!(state.state_hash(), before);
}

#[test]
fn repurchase_x_group_04_real_rejected_order_returns_original_purse_and_abort_retries() {
    let (mut state, mut desk, request, purse) = state(1200, 100);
    let before = state.state_hash();
    let accounts = state.accounts.clone();
    let book = state.plan_book.clone();
    desk.company_buy(request, 1200, 100, purse);
    let batch = desk.flush_company();
    assert!(batch.fills.is_empty());
    assert_eq!(batch.receipts.len(), 1);
    assert_eq!(batch.repurchase.len(), 3);
    assert!(matches!(
        batch.repurchase[0].kind,
        RepurchaseSourceKind::Rejected { .. }
    ));
    assert_eq!(batch.repurchase[0].order_ref, batch.receipts[0].order);
    for _ in 0..2 {
        state.cur_tick = 1;
        consume(&mut state, batch.clone());
        let expected = GroupAccount::new(FUNDING, 4);
        assert_eq!(
            state.accounts[1].as_ref(),
            &expected,
            "原退款实体只能归还一次，不产生虚假股份或费用"
        );
        assert!(!state.child_in_flight.contains_key(&(ACCOUNT, StockId(3))));
        assert_eq!(state.accounts[0], accounts[0]);
        assert_funded(&state, request.plan_id);
        state.rollback();
        assert_eq!(state.accounts, accounts);
        assert_eq!(state.plan_book, book);
        assert_eq!(state.state_hash(), before);
    }
}


/// 各负控必须在即时资源落账前失败；具体 Abort 重试由前缀测试核对。
fn reject_before_mutation(state: &mut GroupState, batch: DeskBatch) {
    let hash = state.state_hash();
    let accounts = state.accounts.clone();
    let book = state.plan_book.clone();
    let tickets = state.child_in_flight.clone();
    let tails = state.repurchase_pending_tails.clone();
    let undo_len = state.undo.len();
    let fills = state.plan_fills.clone();
    let deferred_len = state.staging_deferred.len();
    let error = state.apply_batch_immediate(batch, 1, 0).expect_err("不一致来源必须拒绝");
    assert!(error.to_string().contains("回购来源校验"), "应命中真实来源校验而非未实现 stub：{error}");
    assert_eq!(state.state_hash(), hash);
    assert_eq!(state.accounts, accounts);
    assert_eq!(state.plan_book, book);
    assert_eq!(state.child_in_flight, tickets);
    assert_eq!(state.repurchase_pending_tails, tails);
    assert_eq!(state.undo.len(), undo_len);
    assert_eq!(state.plan_fills, fills);
    assert_eq!(state.staging_deferred.len(), deferred_len);
}

#[test]
fn repurchase_x_group_05_frozen_request_and_raw_order_identity_reject_before_resources() {
    for mode in 0..12 {
        let (mut state, mut desk, request, purse) = state(1000, 100);
        desk.company_buy(request, 1000, 100, purse);
        let mut batch = desk.flush_company();
        match mode {
            0 => { state.child_in_flight.get_mut(&(ACCOUNT, StockId(3))).unwrap().repurchase = None; }
            1 => batch.repurchase[0].request_key.plan_id.sequence += 1,
            2 => batch.repurchase[0].request_key.plan_id.owner = AccountId(51),
            3 => batch.repurchase[0].request_key.requested_trading_day += 1,
            4 => batch.repurchase[0].request_key.stock = StockId(2),
            5 => batch.repurchase[0].owner.0 = 0,
            6 => batch.repurchase[0].owner.1 = 0,
            7 => batch.repurchase[0].order_ref.0 = 2,
            8 => batch.repurchase[0].order_ref.1 += 1_u64 << 32,
            9 => { batch.stock = 2; }
            10 => { state.admin_treasury_bindings[0].issuer = CompanyId(2); }
            _ => { state.admin_treasury_bindings.push(state.admin_treasury_bindings[0]); }
        }
        reject_before_mutation(&mut state, batch);
    }
}

#[test]
fn repurchase_x_group_06_source_and_original_facts_match_with_multiplicity() {
    for mode in 0..10 {
        let (mut state, mut desk, request, purse) = state(1090, 200);
        desk.sell(1000, 100);
        desk.company_buy(request, 1090, 200, purse);
        let mut batch = desk.flush_company();
        let fill_at = batch.repurchase.iter().position(|f| matches!(f.kind, RepurchaseSourceKind::Fill(_))).unwrap();
        let return_at = batch.repurchase.iter().position(|f| matches!(f.kind, RepurchaseSourceKind::Return { .. })).unwrap();
        match mode {
            0 => batch.repurchase.clear(),
            1 => { batch.repurchase.remove(fill_at); }
            2 => { batch.repurchase.remove(return_at); }
            3 => batch.fills.push(batch.fills[0]),
            4 => batch.repurchase.push(batch.repurchase[fill_at]),
            5 => { if let RepurchaseSourceKind::Fill(ref mut fill) = batch.repurchase[fill_at].kind { fill.commission += 1; } }
            6 => { batch.entities.retain(|e| !matches!(e, Entity::BoughtShares { .. })); }
            7 => { let entity = *batch.entities.iter().find(|e| matches!(e, Entity::CashRefund { .. })).unwrap(); batch.entities.push(entity); }
            8 => { batch.entities.push(Entity::CashProceeds { local: 1, amount: 1 }); }
            _ => { batch.repurchase[return_at].kind = RepurchaseSourceKind::Return { amount: -1, kind: crate::runtime::RepurchaseReturnKind::PriceImprovement }; }
        }
        reject_before_mutation(&mut state, batch);
    }
}

#[test]
fn repurchase_x_group_07_lifecycle_and_accepted_numbers_cannot_reset_the_ticket() {
    for mode in 0..7 {
        let (mut state, mut desk, request, purse) = state(1000, 100);
        desk.company_buy(request, 1000, 100, purse);
        let mut batch = desk.flush_company();
        match mode {
            0 => batch.repurchase.push(batch.repurchase[0]),
            1 => { if let RepurchaseSourceKind::Accepted { ref mut qty, .. } = batch.repurchase[0].kind { *qty += 100; } }
            2 => { if let RepurchaseSourceKind::Accepted { ref mut price, .. } = batch.repurchase[0].kind { *price += 1; } }
            3 => { if let RepurchaseSourceKind::Accepted { ref mut purse_initial, .. } = batch.repurchase[0].kind { *purse_initial += 1; } }
            4 => { batch.repurchase[0].kind = RepurchaseSourceKind::Closed; batch.entities.push(Entity::Closed { local: 1, order_serial: order_id_of(&batch.repurchase[0].order_ref).0 }); }
            5 => { consume(&mut state, batch.clone()); }
            _ => { consume(&mut state, batch.clone()); batch.repurchase[0].order_ref.1 += 1; }
        }
        reject_before_mutation(&mut state, batch);
    }
}


#[test]
fn repurchase_x_group_08_bad_fill_suffix_abort_restores_frozen_baseline() {
    for mode in 0..5 {
        let (mut state, mut desk, request, purse) = state(1000, 100);
        let hash = state.state_hash();
        let accounts = state.accounts.clone();
        let book = state.plan_book.clone();
        let tickets = state.child_in_flight.clone();
        desk.company_buy(request, 1000, 100, purse);
        consume(&mut state, desk.flush_company());
        desk.sell(1000, 100);
        let mut batch = desk.flush_company();
        let index = batch.repurchase.iter().position(|f| matches!(f.kind, RepurchaseSourceKind::Fill(_))).unwrap();
        match mode {
            0 => { batch.repurchase.push(batch.repurchase[index]); batch.fills.push(batch.fills[0]); batch.entities.push(Entity::BoughtShares { local: 1, qty: 100 }); }
            1 => { batch.fills[0].price = i64::MAX; batch.repurchase[index].kind = RepurchaseSourceKind::Fill(batch.fills[0]); }
            2 => { batch.fills[0].qty = 0; batch.repurchase[index].kind = RepurchaseSourceKind::Fill(batch.fills[0]); }
            3 => { batch.fills[0].order.1 += 1; batch.repurchase[index].order_ref = batch.fills[0].order; batch.repurchase[index].kind = RepurchaseSourceKind::Fill(batch.fills[0]); }
            _ => { batch.fills[0].stamp_tax = 1; batch.repurchase[index].kind = RepurchaseSourceKind::Fill(batch.fills[0]); }
        }
        reject_before_mutation(&mut state, batch);
        state.rollback();
        assert_eq!(state.state_hash(), hash);
        assert_eq!(state.accounts, accounts);
        assert_eq!(state.plan_book, book);
        assert_eq!(state.child_in_flight, tickets);
    }
}

#[test]
fn repurchase_x_group_09_rejection_return_and_closed_require_original_ordered_evidence() {
    for mode in 0..6 {
        let (mut state, mut desk, request, purse) = state(1200, 100);
        desk.company_buy(request, 1200, 100, purse);
        let mut batch = desk.flush_company();
        match mode {
            0 => batch.repurchase.swap(0, 1),
            1 => { batch.receipts.clear(); }
            2 => batch.repurchase.swap(1, 2),
            3 => { batch.repurchase.insert(1, crate::runtime::RepurchaseSourceFact { kind: RepurchaseSourceKind::Accepted { price: 1200, qty: 100, purse_initial: purse }, ..batch.repurchase[0] }); }
            4 => { batch.entities.retain(|e| !matches!(e, Entity::Closed { .. })); }
            _ => { batch.repurchase[1].kind = RepurchaseSourceKind::Return { amount: purse, kind: crate::runtime::RepurchaseReturnKind::PriceImprovement }; }
        }
        reject_before_mutation(&mut state, batch);
    }
}

#[test]
fn repurchase_x_group_10_terminal_refund_prefix_preserves_ticket_and_forbids_more_resources() {
    let (mut state, mut desk, request, purse) = state(1000, 100);
    let hash = state.state_hash();
    let accounts = state.accounts.clone();
    let tickets = state.child_in_flight.clone();
    desk.company_buy(request, 1000, 100, purse);
    consume(&mut state, desk.flush_company());
    desk.cancel_company();
    let mut prefix = desk.flush_company();
    let closed = prefix.repurchase.pop().unwrap();
    assert_eq!(closed.kind, RepurchaseSourceKind::Closed);
    let index = prefix.entities.iter().position(|e| matches!(e, Entity::Closed { .. })).unwrap();
    let entity = prefix.entities.remove(index);
    consume(&mut state, prefix.clone());
    assert_eq!(state.accounts[1].buy_purse_held_cents, 0);
    assert_eq!(state.accounts[1].foreign_active[3], 1);
    assert!(state.child_in_flight.contains_key(&(ACCOUNT, StockId(3))));
    reject_before_mutation(&mut state, prefix);
    let suffix = DeskBatch { stock: STOCK, tick: 1, round: 0, repurchase: vec![closed], entities: vec![entity], ..Default::default() };
    consume(&mut state, suffix.clone());
    reject_before_mutation(&mut state, suffix);
    assert!(!state.child_in_flight.contains_key(&(ACCOUNT, StockId(3))));
    state.rollback();
    assert_eq!(state.state_hash(), hash);
    assert_eq!(state.accounts, accounts);
    assert_eq!(state.child_in_flight, tickets);
}


#[test]
fn repurchase_x_group_11_partial_maker_progress_replay_is_rejected_but_next_round_is_valid() {
    let (mut state, mut desk, request, purse) = state(1000, 300);
    let hash = state.state_hash();
    let accounts = state.accounts.clone();
    let tickets = state.child_in_flight.clone();
    desk.company_buy(request, 1000, 300, purse);
    consume(&mut state, desk.flush_company());
    desk.sell(1000, 100);
    let first = desk.flush_company();
    assert_eq!(first.fills.len(), 1);
    assert!(!first.repurchase.iter().any(|f| matches!(f.kind, RepurchaseSourceKind::Accepted { .. } | RepurchaseSourceKind::Closed)));
    consume(&mut state, first.clone());
    assert_eq!(state.child_in_flight[&(ACCOUNT, StockId(3))].remaining, 200);
    reject_before_mutation(&mut state, first);
    desk.sell(1000, 100);
    let next = desk.flush_company_round(1);
    let result = state.apply_batch_immediate(next, 1, 1);
    assert!(result.is_ok(), "原 Desk 输出按下一 round 交付时必须保留相同量 Fill：{result:?}");
    state.apply_deferred().unwrap();
    assert_eq!(state.child_in_flight[&(ACCOUNT, StockId(3))].remaining, 100);
    assert_eq!(state.accounts[1].shares[3], 200);
    state.rollback();
    assert_eq!(state.state_hash(), hash);
    assert_eq!(state.accounts, accounts);
    assert_eq!(state.child_in_flight, tickets);
}

#[test]
fn repurchase_x_group_12_multiple_equal_fills_in_one_real_batch_are_not_replay() {
    let (mut state, mut desk, request, purse) = state(1000, 200);
    let hash = state.state_hash();
    let accounts = state.accounts.clone();
    desk.sell(1000, 100);
    desk.sell(1000, 100);
    desk.company_buy(request, 1000, 200, purse);
    let batch = desk.flush_company();
    assert_eq!(batch.fills.len(), 2);
    assert_eq!(batch.fills[0].qty, batch.fills[1].qty);
    assert_eq!(batch.fills[0].price, batch.fills[1].price);
    let charged: i64 = batch.fills.iter().map(|f| f.price * i64::from(f.qty) + f.commission + f.transfer_fee).sum();
    consume(&mut state, batch);
    assert_eq!(state.accounts[1].shares[3], 200);
    assert_eq!(state.accounts[1].frozen_today[3], 200);
    assert_eq!(state.accounts[1].buy_purse_held_cents, 0);
    assert_eq!(state.accounts[1].cash, FUNDING - charged);
    assert!(!state.child_in_flight.contains_key(&(ACCOUNT, StockId(3))));
    state.rollback();
    assert_eq!(state.state_hash(), hash);
    assert_eq!(state.accounts, accounts);
}

#[test]
fn repurchase_x_group_13_unsplit_real_maker_full_fill_preserves_original_entity_order() {
    let (mut state, mut desk, request, purse) = state(1000, 100);
    let hash = state.state_hash();
    let accounts = state.accounts.clone();
    let tickets = state.child_in_flight.clone();
    desk.company_buy(request, 1000, 100, purse);
    consume(&mut state, desk.flush_company());
    desk.sell(1000, 100);
    let batch = desk.flush_company();
    assert!(matches!(batch.repurchase[0].kind, RepurchaseSourceKind::Fill(_)));
    assert_eq!(batch.repurchase[1].kind, RepurchaseSourceKind::Closed);
    assert!(matches!(batch.entities.as_slice(), [Entity::Closed { .. }, Entity::BoughtShares { qty: 100, .. }]));
    let sources = batch.repurchase.clone();
    consume(&mut state, batch);
    assert_eq!(state.accounts[1].shares[3], 100);
    assert_eq!(state.accounts[1].frozen_today[3], 100);
    assert_eq!(state.accounts[1].buy_purse_held_cents, 0);
    assert_eq!(state.accounts[1].foreign_active[3], 0);
    assert!(!state.child_in_flight.contains_key(&(ACCOUNT, StockId(3))));
    let tail = &state.repurchase_pending_tails[&request.plan_id];
    assert_eq!(tail.cursor, 3);
    assert_eq!(tail.facts[1..].iter().map(|fact| fact.source).collect::<Vec<_>>(), sources);
    state.rollback();
    assert_eq!(state.state_hash(), hash);
    assert_eq!(state.accounts, accounts);
    assert_eq!(state.child_in_flight, tickets);
    assert!(state.repurchase_pending_tails.is_empty());
}

#[test]
fn repurchase_x_group_14_missing_sources_cannot_hide_company_facts_in_another_stock_batch() {
    let (mut state, mut desk, request, purse) = state(1090, 200);
    desk.sell(1000, 100);
    desk.company_buy(request, 1090, 200, purse);
    let mut batch = desk.flush_company();
    assert_eq!(batch.fills.len(), 1);
    assert_eq!(batch.fills[0].order.0, STOCK);
    batch.repurchase.clear();
    batch.stock = 2;
    reject_before_mutation(&mut state, batch);
}
