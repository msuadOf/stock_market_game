#[path = "diagnostic_parity.rs"]
mod fixture;

include!("../test-support/simple_company.rs");

use engine::diagnostics::causal::{CausalError, CausalFactKind, CausalReport, Termination};
use engine::{AccountId, Event, GameSession, Intent, Money, Side, StockCode};

#[test]
fn causal_money_fields_use_full_precision_decimal_cents_strings() {
    use engine::diagnostics::causal::Quote;
    let code = StockCode("600101".to_owned());
    let quote = Quote {
        code: code.clone(),
        bid_cents: Some(i64::MIN),
        ask_cents: Some(i64::MAX),
        bid_depth: 0,
        ask_depth: 0,
    };
    let value = serde_json::to_value(&quote).unwrap();
    assert_eq!(value["bid_cents"], i64::MIN.to_string());
    assert_eq!(value["ask_cents"], i64::MAX.to_string());
    let absent = Quote {
        bid_cents: None,
        ask_cents: None,
        ..quote.clone()
    };
    let value = serde_json::to_value(absent).unwrap();
    assert!(value["bid_cents"].is_null());
    assert!(value["ask_cents"].is_null());
    let budget = CausalFactKind::Budget {
        account: AccountId(0),
        available_cents: i64::MAX,
        allocated_cents: vec![i64::MIN, 0, 9_007_199_254_740_993],
    };
    let value = serde_json::to_value(budget).unwrap();
    assert_eq!(value["Budget"]["available_cents"], i64::MAX.to_string());
    assert_eq!(
        value["Budget"]["allocated_cents"],
        serde_json::json!([i64::MIN.to_string(), "0", "9007199254740993"])
    );
    let fill = CausalFactKind::Filled {
        order: engine::OrderId(1),
        account: AccountId(0),
        code: code.clone(),
        qty: 100,
        value_before: i64::MIN,
        gross: i64::MAX,
    };
    let value = serde_json::to_value(fill).unwrap();
    assert_eq!(value["Filled"]["value_before"], i64::MIN.to_string());
    assert_eq!(value["Filled"]["gross"], i64::MAX.to_string());
    let execution = CausalFactKind::Execution {
        code,
        maker: engine::OrderId(1),
        taker: engine::OrderId(2),
        side: Some(Side::Buy),
        qty: 100,
        price_cents: i64::MAX,
        before: quote,
    };
    let value = serde_json::to_value(execution).unwrap();
    assert_eq!(value["Execution"]["price_cents"], i64::MAX.to_string());
    let mut report = canceled_session(false).causal_diagnostics().unwrap();
    report.orders[0].filled_value = i64::MAX;
    let value = serde_json::to_value(report).unwrap();
    assert_eq!(value["orders"][0]["filled_value"], i64::MAX.to_string());
}

#[test]
fn causal_json_preserves_large_unsigned_values_as_decimal_strings() {
    let session = canceled_session(false);
    let mut report = session.causal_diagnostics().unwrap();
    report.seed = u64::MAX;
    report.submitted_qty = u64::MAX;
    report.orders[0].source_sequence = u64::MAX;
    report.orders[0].lifetime_market_minutes = Some(u64::MAX);
    report.orders[0].origin.decision = Some(u64::MAX);
    report.information_delays = vec![(u64::MAX, 3)];
    report.impacts = vec![engine::diagnostics::causal::ImpactSample {
        execution_sequence: u64::MAX,
        quote_sequence: Some(u64::MAX),
        signed_observational_bp: None,
        absent_reason: Some("no_later_quote"),
    }];
    report.recoveries = vec![engine::diagnostics::causal::RecoverySample {
        loss_sequence: u64::MAX,
        recovered_sequence: None,
        market_minutes: None,
        censored_reason: Some("observation_end"),
    }];
    let artifact = serde_json::json!({"causal_runs": [report]});
    let json = &artifact["causal_runs"][0];
    let expected = serde_json::json!(u64::MAX.to_string());
    assert_eq!(json["seed"], expected);
    assert_eq!(json["submitted_qty"], expected);
    assert_eq!(json["orders"][0]["source_sequence"], expected);
    assert_eq!(json["orders"][0]["lifetime_market_minutes"], expected);
    assert_eq!(json["orders"][0]["origin"]["decision"], expected);
    assert_eq!(json["information_delays"][0][0], expected);
    assert_eq!(json["information_delays"][0][1], 3);
    assert_eq!(json["orders"][0]["filled_qty"], "0");
    assert_eq!(json["impacts"][0]["execution_sequence"], expected);
    assert_eq!(json["impacts"][0]["quote_sequence"], expected);
    assert_eq!(json["recoveries"][0]["loss_sequence"], expected);
    assert!(json["recoveries"][0]["recovered_sequence"].is_null());
    assert!(json["recoveries"][0]["market_minutes"].is_null());
}

#[test]
fn causal_fact_json_preserves_large_time_sequence_and_depth() {
    let session = canceled_session(false);
    let mut fact = session.causal_facts()[0].clone();
    fact.sequence = u64::MAX;
    fact.time.market_minute = u64::MAX;
    fact.kind = CausalFactKind::Quote(engine::diagnostics::causal::Quote {
        code: StockCode("600101".to_owned()),
        bid_cents: None,
        ask_cents: None,
        bid_depth: u64::MAX,
        ask_depth: 0,
    });
    let json = serde_json::to_value(&fact).unwrap();
    assert_eq!(json["sequence"], u64::MAX.to_string());
    assert_eq!(json["time"]["market_minute"], u64::MAX.to_string());
    assert_eq!(json["kind"]["Quote"]["bid_depth"], u64::MAX.to_string());
    assert_eq!(json["kind"]["Quote"]["ask_depth"], "0");
    fact.kind = CausalFactKind::Acquisition {
        account: AccountId(0),
        company: engine::company::CompanyId("diagnostic".to_owned()),
        publication: u64::MAX,
        published: fact.time.civil,
        acquired: fact.time.civil,
    };
    let json = serde_json::to_value(fact).unwrap();
    assert_eq!(
        json["kind"]["Acquisition"]["publication"],
        u64::MAX.to_string()
    );
}

fn canceled_session(auction: bool) -> GameSession {
    let mut setup = fixture::setup();
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    if auction {
        setup.auction_ticks = 12;
    }
    let mut session = GameSession::new(setup, 7).unwrap();
    let code = StockCode("600101".to_owned());
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: engine::LimitPrice::Fixed(Money::from_cents(995)),
                qty: 100,
            },
        )
        .unwrap();
    let events = session.step().expect("healthy step");
    let id = events
        .iter()
        .find_map(|event| match event {
            Event::OrderAccepted { id, .. } => Some(*id),
            _ => None,
        })
        .unwrap();
    session
        .enqueue_player_intent(AccountId(0), Intent::Cancel { code, id })
        .unwrap();
    session.step().expect("healthy step");
    session
}

#[test]
fn reconciles_real_continuous_submission_and_voluntary_cancel() {
    let session = canceled_session(false);
    let report = session.causal_diagnostics().unwrap();
    assert_eq!(
        (report.submitted_qty, report.canceled_qty, report.open_qty),
        (100, 100, 0)
    );
    assert_eq!(
        report.orders[0].terminal_reason,
        Some(Termination::Voluntary)
    );
    assert_eq!(report.orders[0].lifetime_market_minutes, Some(8));
    assert_eq!(report.orders[0].lifetime_civil_seconds, Some(480));
}

#[test]
fn auction_lifetime_uses_civil_seconds_without_continuous_minutes() {
    let session = canceled_session(true);
    let report = session.causal_diagnostics().unwrap();
    assert_eq!(report.orders[0].lifetime_market_minutes, Some(0));
    assert_eq!(report.orders[0].lifetime_civil_seconds, Some(75));
}

#[test]
fn rejects_duplicate_and_mismatched_source_ids() {
    let session = canceled_session(false);
    let mut facts = session.causal_facts().to_vec();
    let submitted = facts
        .iter()
        .find(|fact| matches!(fact.kind, CausalFactKind::Submitted(_)))
        .unwrap()
        .clone();
    let mut duplicate = submitted;
    duplicate.sequence = facts.len() as u64;
    facts.push(duplicate);
    assert!(matches!(
        CausalReport::from_facts(7, &facts),
        Err(CausalError::DuplicateOrder(_))
    ));
    let mut facts = session.causal_facts().to_vec();
    for fact in &mut facts {
        if let CausalFactKind::Terminated { account, .. } = &mut fact.kind {
            *account = AccountId(999);
        }
    }
    assert!(matches!(
        CausalReport::from_facts(7, &facts),
        Err(CausalError::OrderMismatch(_))
    ));
}

#[test]
fn absence_is_not_a_zero_metric() {
    let session = GameSession::new(fixture::setup(), 7).unwrap();
    let report = session.causal_diagnostics().unwrap();
    assert_eq!(report.filled_submitted_ratio, None);
    assert_eq!(report.ratio_absent_reason, Some("no_submissions"));
    assert_eq!(report.direction_persistence, None);
}

fn npc_execution_session() -> GameSession {
    npc_execution_session_with_player_quantity(100)
}

fn npc_execution_session_with_player_quantity(player_quantity: u32) -> GameSession {
    const TRADING_DAYS: u32 = 3;
    let mut setup = fixture::setup();
    setup.stocks[0].code = StockCode("000812".to_owned());
    setup.company_system = simple_company_fixture!(engine; codes = setup.stocks.iter().map(|stock| stock.code.0.as_str()));
    setup.stocks[0].exchange = engine::StockExchange::Shenzhen;
    // 原 285 分场景只有自然买单；价格高于个人估值的场景提供真实 NPC 卖单。
    setup.stocks[0].initial_price = Money::from_cents(600);
    // 与默认公司 fixture 的发行股份及 ST 主板类别一致，避免失真的估值分母。
    setup.stocks[0].total_shares = 1_052_631_579;
    setup.stocks[0].category = engine::SecurityCategory::StMainBoard;
    setup.stocks[0].limit_pct = setup.stocks[0].category.limit_pct();
    setup.stocks[0].float_shares = 400_000;
    setup.npcs.retail_count = 24;
    setup.npcs.inst_count = 20;
    let ticks_per_day = setup.ticks_per_day;
    let mut session = GameSession::new(setup, 7).unwrap();
    let initial = session.save().expect("初始化事实必须可读取");
    let initial_cash: i128 = initial
        .snapshot
        .accounts
        .values()
        .map(|account| i128::from(account.cash.cents()))
        .sum();
    let initial_shares: u64 = initial
        .snapshot
        .accounts
        .values()
        .flat_map(|account| account.positions.values())
        .map(|position| u64::from(position.qty))
        .sum();
    let mut traded_shares = 0_u64;
    let mut player_bought = 0_u64;
    for _ in 0..TRADING_DAYS {
        while session.civil_clock().phase() == engine::session::CivilPhase::ClosedDay {
            session.end_civil_day().expect("休市日正常日结");
        }
        // 玩家买单仅为诊断提供真实对手盘，不保证生产市场的流动性。
        session
            .enqueue_player_intent(
                AccountId(0),
                Intent::PlaceLimit {
                    code: StockCode("000812".to_owned()),
                    side: Side::Buy,
                    price: engine::LimitPrice::Highest,
                    qty: player_quantity,
                },
            )
            .expect("真实玩家买单必须入队");
        for _ in 0..ticks_per_day {
            let events = session.step().expect("healthy step");
            let mut player_bought_this_tick = 0_u32;
            for event in events {
                if let Event::Trade {
                    qty, maker, taker, ..
                } = event
                {
                    assert!(qty > 0);
                    assert_ne!(maker, taker);
                    traded_shares += u64::from(qty);
                    if maker == AccountId(0) || taker == AccountId(0) {
                        player_bought_this_tick += qty;
                        player_bought += u64::from(qty);
                    }
                }
            }
            if player_bought_this_tick > 0 {
                let position = session
                    .account(AccountId(0))
                    .unwrap()
                    .position(&StockCode("000812".to_owned()))
                    .expect("真实买入必须形成持仓");
                assert!(position.t1_locked() >= player_bought_this_tick);
            }
        }
        session.end_civil_day().expect("交易日正常日结");
    }
    assert!(traded_shares > 0, "诊断场景必须经过真实撮合");
    assert!(player_bought > 0, "玩家必须与真实 NPC 成交");
    assert_eq!(
        session.causal_diagnostics().unwrap().filled_qty,
        traded_shares * 2,
        "每笔 Trade 必须对应买卖双方 Filled"
    );
    let final_save = session.save().expect("日终事实必须可读取");
    assert_eq!(
        final_save
            .snapshot
            .accounts
            .values()
            .flat_map(|account| account.positions.values())
            .map(|position| u64::from(position.qty))
            .sum::<u64>(),
        initial_shares,
        "真实对手盘不能制造股份"
    );
    assert!(
        final_save
            .snapshot
            .accounts
            .values()
            .map(|account| i128::from(account.cash.cents()))
            .sum::<i128>()
            < initial_cash,
        "真实成交必须扣费，不能补钱"
    );
    session
}

#[test]
fn large_real_fill_stream_does_not_reject_recovered_position_cost() {
    let session = npc_execution_session_with_player_quantity(10_000);
    assert!(session.causal_diagnostics().unwrap().filled_qty > 0);
}

#[test]
fn npc_execution_fixture_has_real_bilateral_fills() {
    let session = npc_execution_session();
    let report = session.causal_diagnostics().unwrap();
    assert!(
        report.filled_qty > 0,
        "真实成交前置条件不满足：申报量={}, 成交量={}",
        report.submitted_qty,
        report.filled_qty
    );
}

#[test]
fn npc_execution_reconciles_every_share_and_preserves_provenance() {
    let session = npc_execution_session();
    let report = session.causal_diagnostics().unwrap();
    assert!(report.submitted_qty > 0);
    assert!(!report.impacts.is_empty());
    assert!(report
        .impacts
        .iter()
        .all(|sample| sample.signed_observational_bp.is_some() != sample.absent_reason.is_some()));
    assert!(report
        .recoveries
        .iter()
        .all(|sample| sample.market_minutes.is_some() != sample.censored_reason.is_some()));
    assert!(report
        .orders
        .iter()
        .any(|order| order.terminal_reason == Some(Termination::DayEnd)));
    assert!(!report.information_delays.is_empty());
    assert_eq!(
        report.submitted_qty,
        report.filled_qty + report.canceled_qty + report.open_qty + report.aborted_qty
    );
    assert!(report
        .orders
        .iter()
        .any(|order| order.origin.company.is_some()
            && order.origin.plan.is_some()
            && order.origin.decision.is_some()));
}

#[test]
fn closing_auction_remainder_has_explicit_day_end_not_voluntary_cancel() {
    let mut setup = fixture::setup();
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.closing_auction_ticks = 3;
    let mut session = GameSession::new(setup, 7).unwrap();
    for _ in 0..27 {
        session.step().expect("healthy step");
    }
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: StockCode("600101".to_owned()),
                side: Side::Buy,
                price: engine::LimitPrice::Fixed(Money::from_cents(995)),
                qty: 100,
            },
        )
        .unwrap();
    for _ in 0..3 {
        session.step().expect("healthy step");
    }
    let report = session.causal_diagnostics().unwrap();
    assert_eq!(report.orders[0].terminal_reason, Some(Termination::DayEnd));
    assert_eq!(report.orders[0].lifetime_civil_seconds, Some(180));
    assert_eq!(report.orders[0].lifetime_market_minutes, Some(0));
}

#[test]
fn quantity_and_budget_overflows_are_explicit_errors() {
    let session = canceled_session(false);
    let mut facts = session.causal_facts().to_vec();
    let mut budget = facts.last().unwrap().clone();
    budget.sequence = facts.len() as u64;
    budget.kind = CausalFactKind::Budget {
        account: AccountId(0),
        available_cents: i64::MAX,
        allocated_cents: vec![i64::MAX, 1],
    };
    facts.push(budget);
    assert!(matches!(
        CausalReport::from_facts(7, &facts),
        Err(CausalError::Overflow)
    ));
    let mut facts = session.causal_facts().to_vec();
    for fact in &mut facts {
        if let CausalFactKind::Terminated { qty, .. } = &mut fact.kind {
            *qty += 1;
        }
    }
    assert!(matches!(
        CausalReport::from_facts(7, &facts),
        Err(CausalError::Conservation(_))
    ));
}

#[test]
fn restore_reports_missing_observation_history_without_fabricating_origins() {
    let session = canceled_session(false);
    let restored = GameSession::restore(&session.save().expect("healthy save")).unwrap();
    assert!(matches!(
        restored.causal_diagnostics(),
        Err(CausalError::RestoredObservation)
    ));
}

#[test]
fn real_fill_stream_rejects_duplicate_fill_and_wrong_execution_price() {
    let session = npc_execution_session();
    let original = session.causal_facts();
    let fill = original
        .iter()
        .position(|fact| matches!(fact.kind, CausalFactKind::Filled { .. }))
        .unwrap();
    let mut duplicated = original.to_vec();
    duplicated.insert(fill, duplicated[fill].clone());
    for (index, fact) in duplicated.iter_mut().enumerate() {
        fact.sequence = index as u64;
    }
    assert!(matches!(
        CausalReport::from_facts(7, &duplicated),
        Err(CausalError::FillMismatch(_))
    ));
    let mut wrong_price = original.to_vec();
    for fact in &mut wrong_price {
        if let CausalFactKind::Execution { price_cents, .. } = &mut fact.kind {
            *price_cents += 1;
            break;
        }
    }
    assert!(matches!(
        CausalReport::from_facts(7, &wrong_price),
        Err(CausalError::ExecutionMismatch)
    ));
}
