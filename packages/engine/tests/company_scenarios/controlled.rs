use super::*;
use engine::company::CompanyId;
use engine::company::CompanyKind;
use engine::information::{NpcObservationContext, PublicationId};
use engine::strategy::{BeliefCause, BeliefInputs};
use engine::AccountId;

fn prepared_prior_session() -> (GameSession, AccountId, AccountId, StockCode, u32) {
    let base = session("2030-12-31");
    let mut save = base.save();
    let stock = code("600101");
    let company = CompanyId("C-600101".into());
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
    for attention in save.npc_attention.values_mut() {
        attention.next_attention_candidate_tick = u64::MAX;
    }
    let initial_reports = save.public_library.report_count();
    let mut game = GameSession::restore(&save).unwrap();
    while game.save().public_library.report_count() == initial_reports {
        if game.civil_clock().phase() == engine::session::CivilPhase::IntradayTrading {
            run_trading_day(&mut game);
        }
        game.end_civil_day().unwrap();
    }
    let mut ready = game.save();
    let current = ready
        .public_library
        .reports_for_company(
            &company,
            engine::CivilInstant::from_hms(ready.civil_clock.current_date, 23, 59, 59).unwrap(),
        )
        .into_iter()
        .rfind(|report| report.reports.kind == engine::accounting::reports::ReportKind::Annual)
        .expect("year-end progression must publish the next annual report")
        .id;
    for account in [first, second] {
        let attention = ready.npc_attention.get_mut(&account).unwrap();
        attention.next_attention_candidate_tick = ready.snapshot.tick;
        attention.rng_state = 0_u64.wrapping_sub(0x9E37_79B9_7F4A_7C15);
    }
    (
        GameSession::restore(&ready).unwrap(),
        first,
        second,
        stock,
        current.value(),
    )
}

#[test]
fn same_current_public_report_revises_two_session_owned_priors_differently() {
    // Given: two restored session-owned books formed from different acquired annual histories.
    let (mut game, first, second, stock, current_report) = prepared_prior_session();
    let before_first = game.belief_debug(first, &stock).unwrap();
    let before_second = game.belief_debug(second, &stock);

    // When: normal GameSession stepping exposes the same newest public report to both accounts.
    let events = game.step();
    let save = game.save();

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
    let mut plain = fixture_session();
    let mut observed = fixture_session();
    let mut plain_events = Vec::new();
    let mut observed_events = Vec::new();

    // When: both run identical commands and one reads diagnostics every tick.
    for _ in 0..TICKS_PER_DAY {
        plain_events.extend(plain.step());
        let _ = observed.decision_chain_diagnostics();
        observed_events.extend(observed.step());
        let _ = observed.plans_debug();
    }

    // Then: canonical event and save bytes are identical.
    let plain_event_bytes = serde_json::to_vec(&plain_events).unwrap();
    let observed_event_bytes = serde_json::to_vec(&observed_events).unwrap();
    let plain_save = serde_json::to_vec(&plain.save()).unwrap();
    let observed_save = serde_json::to_vec(&observed.save()).unwrap();
    assert_eq!(plain_event_bytes, observed_event_bytes);
    assert_eq!(plain_save, observed_save);
    println!(
        "{{\"scenario\":\"diagnostics_byte_parity\",\"event_bytes\":{},\"save_bytes\":{}}}",
        plain_event_bytes.len(),
        plain_save.len()
    );
}
