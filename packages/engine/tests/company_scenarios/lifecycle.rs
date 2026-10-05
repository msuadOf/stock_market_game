use super::*;
use engine::accounting::consolidation::{MemberId, ScopeId};
use engine::accounting::reports::ReportKind;
use engine::accounting::AccountingPeriod;
use engine::session::CivilPhase;

fn annual_close_version_count(save: &engine::session::SaveSlot) -> usize {
    let company_id = engine::company::CompanyId("C-600101".into());
    let scope = ScopeId::Standalone(MemberId(company_id.0.clone()));
    let period = AccountingPeriod::from_ymd(2030, 12).unwrap();
    let system = serde_json::to_value(&save.company_system).expect("CompanySystem serializes");
    let finance_value = &system["implementation"]["state"]["companies"]["C-600101"]["finance"];
    let finance: engine::company::simple::SimpleFinanceState =
        serde_json::from_value(finance_value.clone()).expect("保存された Simple 财务状态可恢复");
    finance
        .closing()
        .versions(&scope, period, ReportKind::Annual)
        .len()
}

#[test]
fn civil_information_chain_keeps_unread_beliefs_stable_and_closed_days_tick_free() {
    // Given: an actual Friday GameSession with seeded accounting, published history, and NPCs.
    let mut session = representative_session("2030-02-01");
    let initial = session.decision_chain_diagnostics();
    assert!(
        initial.library_publications > 0,
        "prehistory must provide public material"
    );

    // When: Friday completes, then Friday and one closed civil day settle.
    run_trading_day(&mut session);
    let beliefs_before = serde_json::to_vec(&session.save().expect("healthy save").belief_books)
        .expect("belief books serialize");
    let friday = session.end_civil_day().expect("completed Friday settles");
    assert_eq!(
        friday.settled_date,
        engine::CivilDate::from_iso("2030-02-01").unwrap()
    );
    let before_closed = session.save().expect("healthy save");
    assert_eq!(session.civil_clock().phase(), CivilPhase::ClosedDay);
    session
        .end_civil_day()
        .expect("Saturday operations and disclosure settle");

    // Then: 公司基本面与披露在闭市日推进，且不产生市场 tick 或未读信念变化。
    let after_closed = session.save().expect("healthy save");
    assert_eq!(before_closed.snapshot.tick, after_closed.snapshot.tick);
    assert_eq!(
        GameSession::restore(&before_closed).unwrap().day(),
        GameSession::restore(&after_closed).unwrap().day()
    );
    assert_eq!(before_closed.rng_state, after_closed.rng_state);
    assert_eq!(
        beliefs_before,
        serde_json::to_vec(&after_closed.belief_books).unwrap()
    );
    assert!(
        after_closed.company_system.advanced_through() > friday.settled_date,
        "闭市自然日必须推进 CompanySystem 的基本面事实"
    );
}

#[test]
#[ignore = "long validation: same authoritative session must span year close through scheduled annual disclosure"]
fn year_boundary_keeps_company_system_and_disclosure_state_authoritative() {
    // Given: 年末 GameSession 持有完整的公司基本面系统与披露状态。
    let base = focused_disclosure_session("2030-12-31");
    let mut controlled = base.save().expect("healthy save");
    controlled.plans = Default::default();
    controlled.parent_orders.clear();
    controlled.npc_order_lifecycles.clear();
    for attention in controlled.npc_attention.values_mut() {
        attention.next_attention_candidate_tick = u64::MAX;
    }
    let mut year_end = GameSession::restore(&controlled).unwrap();

    // When: 年末交易阶段和自然日结算通过 GameSession 执行。
    run_focused_trading_day(&mut year_end);
    let before_close = year_end.save().expect("healthy save");
    let annual_versions_before = annual_close_version_count(&before_close);
    let report = year_end
        .end_civil_day()
        .expect("year-end civil day settles");

    // Then: 年份推进时保留 CompanySystem 基本面与披露进度。
    assert_eq!(
        year_end.civil_date(),
        engine::CivilDate::from_iso("2031-01-01").unwrap()
    );
    let after_close = year_end.save().expect("healthy save");
    assert!(
        annual_close_version_count(&after_close) > annual_versions_before,
        "年末结算必须在真实 SimpleFinanceState 的 ClosingEngine 中增加年度报告版本"
    );
    assert_eq!(
        after_close.company_system.advanced_through(),
        report.settled_date,
        "年末结算后 CompanySystem 保存事实应推进至结算日"
    );
    let annual_published = |game: &GameSession| {
        game.query_public_reports(&engine::company::PublicReportQuery {
            company_id: "C-600101".into(),
            cursor: None,
            page_size: Some(100),
        })
        .expect("public report query must succeed")
        .reports
        .into_iter()
        .any(|item| {
            matches!(item.kind, engine::company::PublicReportKind::Annual)
                && item.period == "2030-12-31"
        })
    };
    let mut closed_days_without_trade = 0_u32;
    while !annual_published(&year_end) {
        if year_end.civil_clock().phase() == CivilPhase::IntradayTrading {
            run_focused_trading_day(&mut year_end);
        } else {
            let before = (year_end.tick(), year_end.day());
            year_end.end_civil_day().unwrap();
            let after = (year_end.tick(), year_end.day());
            assert_eq!(before, after);
            closed_days_without_trade += 1;
            continue;
        }
        year_end.end_civil_day().unwrap();
    }
    let save = year_end.save().expect("healthy save");
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
        annual_close_version_count(&save)
    );
}

#[test]
fn malformed_future_observation_and_unbalanced_accounting_save_are_rejected_without_mutation() {
    // Given: a seasoned real session whose save contains published reports, individual reads, and books.
    let mut prepared = focused_disclosure_session("2030-01-07")
        .save()
        .expect("healthy save");
    let reader = *prepared
        .information_states
        .keys()
        .next()
        .expect("reader exists");
    // The first SplitMix draw is zero, so even a quiet market accepts this
    // account's observation when the next-tick NPC queue is prepared.
    let attention = prepared.npc_attention.get_mut(&reader).unwrap();
    attention.next_attention_candidate_tick = prepared.snapshot.tick;
    attention.rng_state = 0_u64.wrapping_sub(0x9E37_79B9_7F4A_7C15);
    let mut session = GameSession::restore(&prepared).expect("controlled reader restores");
    run_focused_trading_day(&mut session);
    session.end_civil_day().expect("first civil day settles");
    run_focused_trading_day(&mut session);
    session
        .end_civil_day()
        .expect("observation consumption day settles");
    let original =
        serde_json::to_vec(&session.save().expect("healthy save")).expect("save serializes");
    let mut future =
        serde_json::to_value(session.save().expect("healthy save")).expect("save value serializes");
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
    let mut unbalanced =
        serde_json::to_value(session.save().expect("healthy save")).expect("save value serializes");
    let reports = unbalanced["public_library"]["reports"]
        .as_array_mut()
        .expect("reports exist");
    reports[0]["reports"]["balance_sheet"]["total_assets"] = serde_json::json!("1.01");
    let unbalanced_bytes = serde_json::to_vec(&unbalanced).expect("tampered save serializes");
    assert!(engine::session::decode_save_slot(&unbalanced_bytes, &Default::default()).is_err());
    assert_eq!(
        serde_json::to_vec(&session.save().expect("healthy save")).unwrap(),
        original
    );
}
