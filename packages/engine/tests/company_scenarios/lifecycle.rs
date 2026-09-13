use super::*;
use engine::accounting::AccountingPeriod;
use engine::accounting::consolidation::{MemberId, ScopeId};
use engine::accounting::reports::ReportKind;
use engine::session::CivilPhase;

#[test]
fn civil_information_chain_keeps_unread_beliefs_stable_and_closed_days_tick_free() {
    // Given: an actual Friday GameSession with seeded accounting, published history, and NPCs.
    let mut session = session("2030-02-01");
    let initial = session.decision_chain_diagnostics();
    assert!(
        initial.library_publications > 0,
        "prehistory must provide public material"
    );

    // When: Friday completes, then Friday and one closed civil day settle.
    run_trading_day(&mut session);
    let beliefs_before =
        serde_json::to_vec(&session.save().belief_books).expect("belief books serialize");
    let friday = session.end_civil_day().expect("completed Friday settles");
    assert_eq!(
        friday.settled_date,
        engine::CivilDate::from_iso("2030-02-01").unwrap()
    );
    let before_closed = session.save();
    assert_eq!(session.civil_clock().phase(), CivilPhase::ClosedDay);
    session
        .end_civil_day()
        .expect("Saturday operations and disclosure settle");

    // Then: operations/disclosures advance on the closed day without a market tick or unread belief mutation.
    let after_closed = session.save();
    assert_eq!(before_closed.snapshot.tick, after_closed.snapshot.tick);
    assert_eq!(before_closed.snapshot.day, after_closed.snapshot.day);
    assert_eq!(before_closed.rng_state, after_closed.rng_state);
    assert_eq!(
        beliefs_before,
        serde_json::to_vec(&after_closed.belief_books).unwrap()
    );
    assert!(
        after_closed.company_operations.next_expected_date() > friday.settled_date,
        "closed civil operations must advance the accounting scheduler"
    );
}

#[test]
fn year_boundary_keeps_company_operations_and_disclosure_state_authoritative() {
    // Given: one year-end session with the full company operations and disclosure state.
    let base = session("2030-12-31");
    let mut controlled = base.save();
    controlled.plans = Default::default();
    controlled.parent_orders.clear();
    controlled.npc_order_lifecycles.clear();
    for attention in controlled.npc_attention.values_mut() {
        attention.next_attention_candidate_tick = u64::MAX;
    }
    let mut year_end = GameSession::restore(&controlled).unwrap();

    // When: the trading session and accounting/disclosure day end run through GameSession.
    run_trading_day(&mut year_end);
    let scope = ScopeId::Standalone(MemberId("C-600101".into()));
    let annual_period = AccountingPeriod::from_ymd(2030, 12).unwrap();
    let closing_before = year_end
        .save()
        .closing_registry
        .versions(&scope, annual_period, ReportKind::Annual)
        .len();
    let report = year_end
        .end_civil_day()
        .expect("year-end civil day settles");

    // Then: the year advances without discarding company operations or disclosure progress.
    assert_eq!(
        year_end.civil_date(),
        engine::CivilDate::from_iso("2031-01-01").unwrap()
    );
    assert!(
        !year_end
            .save()
            .company_operations
            .scheduler()
            .pending()
            .is_empty(),
        "year-end must retain future operating obligations"
    );
    let after_close = year_end.save();
    assert!(
        after_close
            .closing_registry
            .versions(&scope, annual_period, ReportKind::Annual)
            .len()
            > closing_before
    );
    let mut closed_days_without_trade = 0_u32;
    let annual_published = |game: &GameSession| {
        let save = game.save();
        save.public_library
            .reports_for_company(
                &engine::company::CompanyId("C-600101".into()),
                engine::CivilInstant::from_hms(save.civil_clock.current_date, 23, 59, 59).unwrap(),
            )
            .into_iter()
            .any(|item| {
                item.reports.kind == ReportKind::Annual && item.reports.period.year() == 2030
            })
    };
    while !annual_published(&year_end) {
        if year_end.civil_clock().phase() == CivilPhase::IntradayTrading {
            run_trading_day(&mut year_end);
        } else {
            let before = year_end.save();
            year_end.end_civil_day().unwrap();
            let after = year_end.save();
            assert_eq!(before.snapshot.tick, after.snapshot.tick);
            assert_eq!(before.snapshot.day, after.snapshot.day);
            closed_days_without_trade += 1;
            continue;
        }
        year_end.end_civil_day().unwrap();
    }
    let save = year_end.save();
    assert!(closed_days_without_trade > 0);
    let published = save.public_library.latest_published_instant().unwrap();
    assert!(published.date() >= report.disclosure_instant.date());
    assert_eq!(save.disclosures.published_through(), Some(published));
    let annual = save
        .public_library
        .reports_for_company(&engine::company::CompanyId("C-600101".into()), published)
        .into_iter()
        .rfind(|item| {
            item.reports.kind == engine::accounting::reports::ReportKind::Annual
                && item.reports.period.year() == 2030
        })
        .unwrap();
    assert_eq!(annual.reports.period.year(), 2030);
    assert!(annual.reports.balance_sheet.total_assets.is_positive());
    println!(
        "{{\"scenario\":\"year_close\",\"report_id\":{},\"period\":\"{}\",\"published_date\":\"{}\",\"versions\":{}}}",
        annual.id.value(),
        annual.reports.period,
        annual.published_at.date(),
        save.closing_registry
            .versions(&scope, annual_period, ReportKind::Annual)
            .len()
    );
}

#[test]
fn malformed_future_observation_and_unbalanced_accounting_save_are_rejected_without_mutation() {
    // Given: a seasoned real session whose save contains published reports, individual reads, and books.
    let mut session = session("2030-01-07");
    run_trading_day(&mut session);
    session.end_civil_day().expect("first civil day settles");
    let original = serde_json::to_vec(&session.save()).expect("save serializes");
    let mut future = serde_json::to_value(session.save()).expect("save value serializes");
    let state = future["information_states"]
        .as_object_mut()
        .expect("information map");
    let account = state
        .keys()
        .next()
        .expect("belief account exists")
        .to_owned();
    let companies = state[&account]["companies"]
        .as_object_mut()
        .expect("companies map");
    let company = companies
        .keys()
        .next()
        .expect("company read exists")
        .to_owned();
    companies[&company].as_array_mut().expect("records")[0]["observed_at"]["date"] =
        serde_json::json!("2035-01-01");

    // When / Then: future personal material and an unbalanced report are both explicit invalid saves.
    let future_bytes = serde_json::to_vec(&future).expect("tampered save serializes");
    let decoded = engine::session::decode_save_slot(&future_bytes, &Default::default())
        .expect("future observation JSON remains structurally decodable");
    assert!(engine::GameSession::restore(&decoded).is_err());
    let mut unbalanced = serde_json::to_value(session.save()).expect("save value serializes");
    let reports = unbalanced["public_library"]["reports"]
        .as_array_mut()
        .expect("reports exist");
    reports[0]["reports"]["balance_sheet"]["total_assets"] = serde_json::json!("1.01");
    let unbalanced_bytes = serde_json::to_vec(&unbalanced).expect("tampered save serializes");
    assert!(engine::session::decode_save_slot(&unbalanced_bytes, &Default::default()).is_err());
    assert_eq!(serde_json::to_vec(&session.save()).unwrap(), original);
}
