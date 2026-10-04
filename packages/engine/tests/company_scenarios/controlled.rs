use super::*;
use engine::company::CompanyId;
use engine::company::CompanyKind;
use engine::information::{NpcObservationContext, PublicationId};
use engine::strategy::{BeliefCause, BeliefInputs};
use engine::AccountId;

fn prepared_prior_session() -> (GameSession, AccountId, AccountId, StockCode, u32) {
    let stock = code("600101");
    let company = CompanyId("C-600101".into());
    // The company domain has its own seed stream. Read the actual disclosure
    // offset from published history instead of deriving it from the game seed.
    let schedule_source = focused_disclosure_session("2031-01-01")
        .save()
        .expect("healthy schedule source");
    let offset = schedule_source
        .public_library
        .reports_for_company(
            &company,
            schedule_source
                .public_library
                .latest_published_instant()
                .unwrap(),
        )
        .last()
        .expect("seeded history contains a scheduled report")
        .origin
        .scheduled()
        .expect("seeded report records its schedule")
        .2;
    let annual_date = engine::information::scheduled_instant(
        engine::information::ScheduledReportKind::Annual,
        2030,
        offset,
    )
    .expect("fixture annual schedule must be valid")
    .date();
    let base = focused_disclosure_session(&annual_date.to_iso());
    let mut save = base.save().expect("healthy save");
    let accounts: Vec<AccountId> = save.belief_books.keys().copied().take(2).collect();
    let [first, second] = [accounts[0], accounts[1]];
    let reports: Vec<_> = save
        .public_library
        .reports_for_company(
            &company,
            engine::CivilInstant::from_hms(
                engine::CivilDate::from_iso("2099-12-31").unwrap(),
                23,
                59,
                59,
            )
            .unwrap(),
        )
        .into_iter()
        .filter(|report| report.reports.kind == engine::accounting::reports::ReportKind::Annual)
        .collect();
    let first_report = *reports
        .last()
        .expect("seeded library must contain a prior annual report");
    assert_eq!(first_report.reports.period.year(), 2029);
    let observed = first_report.published_at;
    let state = save.information_states.get_mut(&first).unwrap();
    state
        .record_acquisition(first, &save.public_library, first_report.id, observed)
        .unwrap();
    let context = NpcObservationContext::new(first, state, &save.public_library, &()).unwrap();
    let spec = save.company_operations.company(&company).unwrap().spec();
    save.belief_books
        .get_mut(&first)
        .unwrap()
        .apply_cause(
            &stock,
            BeliefCause::NewMaterial {
                report: first_report.id,
            },
            &BeliefInputs {
                ctx: &context,
                company: company.clone(),
                kind: CompanyKind::Industrial,
                total_issued_shares: spec.issued_shares,
                as_of_trading_day: 0,
            },
        )
        .unwrap();
    let prior_information = save.information_states.clone();
    let prior_beliefs = save.belief_books.clone();
    for attention in save.npc_attention.values_mut() {
        attention.next_attention_candidate_tick = u64::MAX;
    }
    let mut game = GameSession::restore(&save).unwrap();
    let annual_report_id = |game: &GameSession| {
        game.query_public_reports(&engine::company::PublicReportQuery {
            company_id: company.0.clone(),
            cursor: None,
            page_size: Some(100),
        })
        .expect("public report query must succeed")
        .reports
        .into_iter()
        .find(|report| {
            matches!(report.kind, engine::company::PublicReportKind::Annual)
                && report.period == "2030-12-31"
        })
        .map(|report| report.id.parse::<u32>().expect("publication id is decimal"))
    };
    assert_eq!(
        game.civil_clock().phase(),
        engine::session::CivilPhase::IntradayTrading,
        "the deterministic annual publication date is a trading day in the frozen calendar"
    );
    assert!(
        annual_report_id(&game).is_none(),
        "current annual report is not public before civil settlement"
    );
    run_focused_trading_day(&mut game);
    game.end_civil_day().unwrap();
    let mut ready = game.save().expect("healthy save");
    let current = annual_report_id(&game).expect("year-end progression must publish the report");
    // The publication-day opening review can acquire older material even when
    // attention is disabled. Install the controlled personal histories after
    // that preparation day, before either account reads the new report.
    ready.information_states = prior_information;
    ready.belief_books = prior_beliefs;
    assert!(ready.belief_books[&first].entry(&stock).is_some());
    assert!(ready.belief_books[&second].entry(&stock).is_none());
    for account in [first, second] {
        assert!(ready.information_states[&account]
            .observed_at_of(PublicationId::new(current))
            .is_none());
        let attention = ready.npc_attention.get_mut(&account).unwrap();
        attention.next_attention_candidate_tick = ready.snapshot.tick;
        attention.rng_state = 0_u64.wrapping_sub(0x9E37_79B9_7F4A_7C15);
    }
    (
        GameSession::restore(&ready).unwrap(),
        first,
        second,
        stock,
        current,
    )
}

#[test]
fn same_current_public_report_revises_two_session_owned_priors_differently() {
    // Given: two restored session-owned books formed from different acquired annual histories.
    let (mut game, first, second, stock, current_report) = prepared_prior_session();
    let before_first = game.belief_debug(first, &stock).unwrap();
    let before_second = game.belief_debug(second, &stock);

    // When: the first tick schedules the controlled observation; the next
    // consumes it through the real next-tick NPC queue and reads the report.
    let mut events = game.step().expect("observation is scheduled");
    game.end_civil_day()
        .expect("one-tick preparation day settles");
    events.extend(game.step().expect("scheduled observation is consumed"));
    let save = game.save().expect("healthy save");

    // Then: both own the same report while their personal valuations revise materially differently.
    assert!(save.information_states[&first]
        .observed_at_of(PublicationId::new(current_report))
        .is_some());
    assert!(save.information_states[&second]
        .observed_at_of(PublicationId::new(current_report))
        .is_some());
    let after_first = game.belief_debug(first, &stock).unwrap();
    let after_second = game.belief_debug(second, &stock).unwrap();
    assert_ne!(after_first, before_first);
    assert_ne!(Some(after_second.clone()), before_second);
    assert_ne!(
        after_first.per_share_optimistic_cents,
        after_second.per_share_optimistic_cents
    );
    println!(
        "{{\"scenario\":\"session_same_report\",\"report_id\":{},\"owners\":[{},{}],\"events\":{},\"first_after\":{},\"second_after\":{}}}",
        current_report,
        first.0,
        second.0,
        events.len(),
        after_first.per_share_optimistic_cents,
        after_second.per_share_optimistic_cents
    );
}

#[test]
fn reading_diagnostics_does_not_change_authoritative_events_or_saves() {
    // Given: same-seed twins; only one reads the real diagnostic API between commands.
    let mut plain = representative_session(&fixture().start_date);
    let mut observed = representative_session(&fixture().start_date);
    let mut plain_events = Vec::new();
    let mut observed_events = Vec::new();

    // When: both run identical commands and one reads diagnostics every tick.
    for _ in 0..TICKS_PER_DAY {
        plain_events.extend(plain.step().expect("healthy step"));
        let _ = observed.decision_chain_diagnostics();
        observed_events.extend(observed.step().expect("healthy step"));
        let _ = observed.plans_debug();
    }

    // Then: canonical event and save bytes are identical.
    let plain_event_bytes = serde_json::to_vec(&plain_events).unwrap();
    let observed_event_bytes = serde_json::to_vec(&observed_events).unwrap();
    let plain_save = serde_json::to_vec(&plain.save().expect("healthy save")).unwrap();
    let observed_save = serde_json::to_vec(&observed.save().expect("healthy save")).unwrap();
    assert_eq!(plain_event_bytes, observed_event_bytes);
    assert_eq!(plain_save, observed_save);
    println!(
        "{{\"scenario\":\"diagnostics_byte_parity\",\"event_bytes\":{},\"save_bytes\":{}}}",
        plain_event_bytes.len(),
        plain_save.len()
    );
}
