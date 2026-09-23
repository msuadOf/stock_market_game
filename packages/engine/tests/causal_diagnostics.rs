#[path = "diagnostic_parity.rs"]
mod fixture;

use engine::diagnostics::causal::{CausalError, CausalFactKind, CausalReport, Termination};
use engine::{AccountId, Event, GameSession, Intent, Money, Side, StockCode};

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
                price: Money::from_cents(995),
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

#[test]
fn npc_execution_reconciles_every_share_and_preserves_provenance() {
    let mut setup = fixture::setup();
    setup.stocks[0].code = StockCode("000812".to_owned());
    setup.stocks[0].exchange = engine::StockExchange::Shenzhen;
    setup.stocks[0].initial_price = Money::from_cents(285);
    setup.stocks[0].total_shares = 1_000_000;
    setup.stocks[0].float_shares = 400_000;
    setup.npcs.inst_count = 20;
    let mut session = GameSession::new(setup, 7).unwrap();
    for _ in 0..600 {
        session.step().expect("healthy step");
    }
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
                price: Money::from_cents(995),
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
    let mut setup = fixture::setup();
    setup.stocks[0].code = StockCode("000812".to_owned());
    setup.stocks[0].exchange = engine::StockExchange::Shenzhen;
    setup.stocks[0].initial_price = Money::from_cents(285);
    setup.stocks[0].total_shares = 1_000_000;
    setup.stocks[0].float_shares = 400_000;
    setup.npcs.inst_count = 20;
    let mut session = GameSession::new(setup, 7).unwrap();
    for _ in 0..600 {
        session.step().expect("healthy step");
    }
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
