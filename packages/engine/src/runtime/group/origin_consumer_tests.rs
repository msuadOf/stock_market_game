//! 共同计划消费者必须穷尽处理 origin；公司执行仍受未接线门禁约束。

use super::*;
use crate::plans::{PlanBook, RepurchaseExecutionOpen};
use crate::{AccountId, StockId};

fn company_book() -> PlanBook {
    let mut book = PlanBook::default();
    book.create_issuer_repurchase(RepurchaseExecutionOpen {
        owner: AccountId(17),
        issuer: crate::company::CompanyId(3),
        stock: StockId(0),
    })
    .unwrap();
    book
}

fn state(book: PlanBook) -> GroupState {
    let mut state = GroupState::new(
        1,
        vec![GroupAccount::new(0, 1)],
        8,
        100,
        Arc::new(GameConfig::proposed_defaults()),
        240,
        None,
        Vec::new(),
        Box::new(|_, _| TickPlan::default()),
    );
    state.plan_book = book;
    state
}

#[test]
fn origin_seed_accepts_company_without_child_and_rejects_any_unconnected_execution() {
    let book = company_book();
    let prepared = prepare_plan_seeds(vec![book.clone()], &[AccountId(17)], 1, &[vec![]]).unwrap();
    assert_eq!(prepared[0].book, book);
    assert!(prepared[0].tickets.is_empty());
    let mut wire = serde_json::to_value(&book).unwrap();
    wire["plans"][0]["IssuerRepurchase"]["execution"] = serde_json::json!({
        "child_qty":100, "limit_price":"1234", "expires_market_minute":30,
        "active_child_remaining_qty":null,
    });
    let unsupported = serde_json::from_value(wire).unwrap();
    assert!(
        matches!(prepare_plan_seeds(vec![unsupported], &[AccountId(17)], 1, &[vec![]]),
        Err(RuntimeError::ProtocolInvariant { detail }) if detail.contains("公司计划尚未接入真实子单来源"))
    );
}

#[test]
fn origin_state_hash_covers_issuer_stock_status_and_complete_execution_fields() {
    let original = company_book();
    let original_hash = state(original.clone()).state_hash();
    for (field, value) in [
        ("issuer", serde_json::json!(4)),
        ("stock", serde_json::json!(1)),
        ("status", serde_json::json!("Draining")),
        ("status", serde_json::json!("Closed")),
    ] {
        let mut wire = serde_json::to_value(&original).unwrap();
        wire["plans"][0]["IssuerRepurchase"][field] = value;
        assert_ne!(
            state(serde_json::from_value(wire).unwrap()).state_hash(),
            original_hash,
            "公司字段不得漏hash：{field}"
        );
    }
    let mut wire = serde_json::to_value(&original).unwrap();
    wire["plans"][0]["IssuerRepurchase"]["execution"] = serde_json::json!({
        "child_qty":100, "limit_price":"1234", "expires_market_minute":30,
        "active_child_remaining_qty":null,
    });
    let execution_hash = state(serde_json::from_value(wire.clone()).unwrap()).state_hash();
    assert_ne!(execution_hash, original_hash);
    for (field, value) in [
        ("child_qty", serde_json::json!(200)),
        ("limit_price", serde_json::json!("1235")),
        ("expires_market_minute", serde_json::json!(31)),
        ("active_child_remaining_qty", serde_json::json!(100)),
    ] {
        let mut changed = wire.clone();
        changed["plans"][0]["IssuerRepurchase"]["execution"][field] = value;
        assert_ne!(
            state(serde_json::from_value(changed).unwrap()).state_hash(),
            execution_hash,
            "公司执行字段不得漏hash：{field}"
        );
    }
}

#[test]
fn origin_group_day_end_keeps_company_authorization_and_shared_owner_sequence() {
    let book = company_book();
    let mut state = state(book.clone());
    let date = crate::calendar::CivilDate::from_iso("2030-01-02").unwrap();
    let bindings = vec![(StockId(0), crate::calendar::CalendarExchange::Sse)];
    let source = crate::session::market_calendar_context::MarketCalendarSource::new(
        crate::calendar::TradingCalendar::from_policy(
            crate::calendar::CalendarPolicy::current_default_policy().unwrap()).unwrap(), date, bindings.clone(),
    ).unwrap();
    let day = source.day(date).unwrap();
    state.bind_calendar(source, bindings);
    state.cur_tick = 1;
    state.committed_tick = 1;
    state
        .handle_day_end(&DayEndDirective {
            tick: 1,
            day: 0,
            civil_date: date,
            calendar: super::super::protocol::DayEndCalendar::ManualDayEnd(day),
            rights: vec![],
        })
        .unwrap();
    assert_eq!(state.plan_book, book);
    assert!(state.child_in_flight.is_empty());
}


#[test]
fn origin_mixed_stock_day_end_uses_real_group_and_rejects_closed_company_execution_atomically() {
    use crate::calendar::{CalendarExchange, CalendarPolicy, CivilDate, OfficialCoverageEntry, TradingCalendar};
    use crate::plans::{OpinionSource, PlanOpen, PlanOpinion, PlanTarget, Urgency};
    use crate::session::market_calendar_context::MarketCalendarSource;
    let date = |value| CivilDate::from_iso(value).unwrap();
    let first = date("2030-01-02");
    let ended = date("2030-01-08");
    let mut policy = CalendarPolicy::current_default_policy().unwrap().spec();
    // 2030 沪深差异仅为 synthetic-test，不冒充官方交易所安排。
    policy.official_coverage = vec![OfficialCoverageEntry::new(
        CalendarExchange::Sse, 2030, vec![(date("2030-01-01"), ended)],
        "synthetic-origin-group-day-end".into(), "synthetic-origin-group-day-end-source".into(),
    )];
    let bindings = vec![(StockId(0), CalendarExchange::Szse), (StockId(1), CalendarExchange::Sse)];
    let source = MarketCalendarSource::new(
        TradingCalendar::from_policy(CalendarPolicy::from_parts(policy).unwrap()).unwrap(), first, bindings.clone(),
    ).unwrap();
    let day = source.day(ended).unwrap();
    assert_eq!(day.day_model().stock(StockId(0)).unwrap().sessions_before_date(), 4);
    assert!(matches!(day.day_model().stock(StockId(0)).unwrap().status(), crate::calendar::DayStatus::Trading));
    assert!(matches!(day.day_model().stock(StockId(1)).unwrap().status(), crate::calendar::DayStatus::Closed(_)));
    let open = |owner, stock| PlanOpen {
        account: AccountId(owner), code: StockId(stock), direction: Side::Buy,
        target: PlanTarget::ShareCount(100),
        opinion: PlanOpinion { signal_score_bp: 3000, source: OpinionSource::Blended },
        confidence_bp: 8000, urgency: Urgency::Normal, horizon_trading_days: 5, created_trading_day: 0,
    };
    let mut book = PlanBook::default();
    let investor_open = book.create(open(1, 0)).unwrap();
    let investor_closed = book.create(open(2, 1)).unwrap();
    let company = book.create_issuer_repurchase(RepurchaseExecutionOpen {
        owner: AccountId(17), issuer: crate::company::CompanyId(3), stock: StockId(1),
    }).unwrap();
    let authorization = book.entry(company).unwrap().clone();
    let closed_plan = book.entry(investor_closed).unwrap().clone();
    let mut accounts = vec![GroupAccount::new(0, 2)];
    accounts[0].shares[0] = 100; accounts[0].shares[1] = 100;
    accounts[0].frozen_today[0] = 100; accounts[0].frozen_today[1] = 100;
    let mut group = GroupState::new(0, accounts, 8, 100,
        Arc::new(GameConfig::proposed_defaults()), 2, None, Vec::new(), Box::new(|_, _| TickPlan::default()));
    group.bind_calendar(Arc::clone(&source), bindings);
    group.plan_book = book.clone();
    group.cur_tick = 9; group.committed_tick = 9;
    let directive = DayEndDirective { tick: 9, day: 4, civil_date: ended,
        calendar: super::super::protocol::DayEndCalendar::ManualDayEnd(day), rights: vec![] };
    group.handle_day_end(&directive).unwrap();
    assert!(group.plan_book.plan(investor_open).unwrap().is_terminal(), "开市股须按本股 ordinal 到期");
    assert_eq!(group.plan_book.entry(investor_closed).unwrap(), &closed_plan, "休市股不得被共享 day 老化");
    assert_eq!(group.plan_book.entry(company).unwrap(), &authorization);
    assert_eq!(group.accounts[0].frozen_today.as_slice(), &[0, 100], "仅开市股可以 T+1 解冻");
    let mut wire = serde_json::to_value(&book).unwrap();
    for entry in wire["plans"].as_array_mut().unwrap() {
        if let Some(company) = entry.get_mut("IssuerRepurchase") {
            company["execution"] = serde_json::json!({"child_qty":100,"limit_price":"1000",
                "expires_market_minute":30,"active_child_remaining_qty":null});
        }
    }
    group.plan_book = serde_json::from_value(wire).unwrap();
    let before_book = group.plan_book.clone();
    let before_accounts = group.accounts.clone();
    let before_undo = group.undo.len();
    let error = group.handle_day_end(&directive).unwrap_err();
    assert!(error.to_string().contains("尚未接线") || error.to_string().contains("IssuerChildNotConnected"), "{error}");
    assert_eq!(group.plan_book, before_book, "休市公司 execution 必须先拒绝且整簿不半迁移");
    assert_eq!(group.accounts, before_accounts, "拒绝前不得解冻或安装资产");
    assert_eq!(group.undo.len(), before_undo, "公司拒绝前不得退役 undo");
    let mut wire = serde_json::to_value(&book).unwrap();
    for entry in wire["plans"].as_array_mut().unwrap() {
        if let Some(investor) = entry.get_mut("Investor") {
            if investor["code"] == serde_json::json!(0) {
                investor["last_event_trading_day"] = serde_json::json!(5);
            }
        }
    }
    group.plan_book = serde_json::from_value(wire).unwrap();
    let before_book = group.plan_book.clone();
    let error = group.handle_day_end(&directive).unwrap_err();
    assert!(error.to_string().contains("precedes"), "{error}");
    assert_eq!(group.plan_book, before_book, "本股事件时间回拨须前置拒绝");
    assert_eq!(group.accounts, before_accounts);
    assert_eq!(group.undo.len(), before_undo);

    // 真实 Drain 批次先即时落资产，再由 DayEnd 重放 F5/F6；迁移不能覆盖这些事实。
    let mut drain_book = PlanBook::default();
    let mut drain_open = open(1, 0);
    drain_open.target = PlanTarget::ShareCount(300);
    let drain_plan = drain_book.create(drain_open).unwrap();
    drain_book.install_execution(drain_plan, 200, crate::Money::from_cents(1000), 30).unwrap();
    group.plan_book = drain_book;
    group.root_projection = Some(Arc::new(crate::runtime::world::RootProjection {
        catalog: crate::stock::StockCatalog::from_metas(vec![
            crate::stock::StockMeta::new("000001"), crate::stock::StockMeta::new("600001"),
        ]).unwrap(),
        initial_company_facts: Arc::new(crate::company::CompanyReadFacts::new(
            (0..2).map(|id| (StockId(id), crate::company::CompanyId(id))).collect(),
            (0..2).map(|id| (crate::company::CompanyId(id),
                (crate::company::CompanyKind::Industrial, 1_000_000))).collect(),
        ).unwrap()),
        account_ids: vec![AccountId(1)], auction_ticks: 0, closing_auction_ticks: 0,
        chain_params: vec![None], session_seed: 1,
        urgency_policy: crate::plans::UrgencyPolicy::default(),
        max_order_qty_limit: Default::default(), tick_size_cents: Default::default(),
    }));
    let tick = directive.calendar.day().tick(
        crate::session::exchange_calendar_model::SessionTickLayout {
            ticks_per_day: 2, auction_ticks: 0, closing_auction_ticks: 0,
        },
        crate::session::exchange_calendar_model::SharedClockInput {
            tick: 8, completed_shared_sessions: 4,
            progress: crate::session::exchange_calendar_model::SharedDayProgress::Trading { completed_ticks: 0 },
        },
    ).unwrap();
    group.snapshot = Some(Arc::new(super::super::protocol::TickSnapshot {
        calendar: tick, tick: 9, civil_date: ended, market_minute: 0, stocks: Arc::new(Vec::new()),
    }));
    let mut account = GroupAccount::new(0, 2);
    account.buy_purse_held_cents = 200_000;
    account.foreign_active[0] = 1;
    account.shares[1] = 100;
    account.frozen_today[1] = 100;
    group.accounts = vec![Arc::new(account)];
    group.child_in_flight.insert((AccountId(1), StockId(0)), ChildTicket {
        qty: 200, side: Side::Buy, price: 1000, order_id: None, remaining: 200, charged_cents: 0,
    });
    group.seal_acked_len = Some(group.undo.len());
    group.apply_batch_immediate(DeskBatch {
        repurchase: Vec::new(),
        stock: 0, tick: 9, round: 0,
        entities: vec![Entity::BoughtShares { local: 0, qty: 100 },
            Entity::CashRefund { local: 0, amount: 100_000 },
            Entity::Closed { local: 0, order_serial: 5 }],
        fills: vec![FillFact { order: (0, 5), owner: (0, 0), side: Side::Buy,
            price: 1000, qty: 100, commission: 0, stamp_tax: 0, transfer_fee: 0 }],
        receipts: vec![],
    }, 9, 0).unwrap();
    group.handle_day_end(&directive).unwrap();
    let after = group.plan_book.plan(drain_plan).unwrap();
    assert_eq!(after.filled_qty(), 100, "Drain 成交量不得被旧预检簿覆盖");
    assert!(after.is_terminal());
    assert!(after.execution().is_none());
    assert!(group.child_in_flight.is_empty(), "Drain Closed 必须释放真实余单票");
    assert_eq!(group.accounts[0].cash, 100_000);
    assert_eq!(group.accounts[0].shares.as_slice(), &[100, 100]);
    assert_eq!(group.accounts[0].frozen_today.as_slice(), &[0, 100]);
    assert_eq!(group.accounts[0].buy_purse_held_cents, 0);
    assert_eq!(group.accounts[0].foreign_active[0], 0);
}
