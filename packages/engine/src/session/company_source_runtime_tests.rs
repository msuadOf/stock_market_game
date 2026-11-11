//! 实际选源调用、月末结算次数与完整候选失败回滚的短门。
use super::*;
use crate::company::source_probe::{self, Counts};
include!("../../test-support/simple_company.rs");

fn month_end_setup(simple: bool) -> SessionSetup {
    let mut setup = npc_working_quote_tests::quote_setup(0);
    setup.start_date = crate::CivilDate::from_iso("2030-03-31").unwrap();
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.ticks_per_day = 1;
    setup.auction_ticks = 0;
    setup.closing_auction_ticks = 0;
    setup.history_len = 1;
    if simple {
        setup.company_system = Some(simple_company_fixture!(crate; ["600888"]));
        setup.dividend_tax_mode = Some(crate::company::CashDividendTaxMode::FlatWithholding);
        setup.flat_withholding_bp = Some(1000);
        setup.par_value_per_share = Some(Money::from_cents(100));
    }
    setup
}

fn assert_no_legacy(counts: Counts) {
    assert_eq!(
        (
            counts.legacy_assembly,
            counts.legacy_prehistory,
            counts.legacy_advance
        ),
        (0, 0, 0)
    );
}

#[test]
fn selected_simple_constructs_real_history_without_any_legacy_call_and_finalizes_once() {
    source_probe::reset();
    let mut session = GameSession::new(month_end_setup(true), 42).unwrap();
    let creation = source_probe::snapshot();
    assert_no_legacy(creation);
    assert!(
        creation.simple_finalize >= 24,
        "探针必须看到真实Simple前史，而非未调用的假零计数"
    );
    let history_before = session.company_system().unwrap().history().len();
    source_probe::reset();
    let report = session.end_civil_day().unwrap();
    assert_eq!(
        report.settled_date,
        crate::CivilDate::from_iso("2030-03-31").unwrap()
    );
    let settled = source_probe::snapshot();
    assert_no_legacy(settled);
    assert_eq!((settled.simple_advance, settled.simple_finalize), (1, 1));
    assert_eq!(
        session.company_system().unwrap().history().len(),
        history_before + 1
    );
    assert_eq!(
        session.company_system().unwrap().advanced_through(),
        report.settled_date
    );
}

#[test]
fn selected_none_really_calls_legacy_assembly_prehistory_and_one_live_advance() {
    source_probe::reset();
    let mut session = GameSession::new(month_end_setup(false), 42).unwrap();
    let creation = source_probe::snapshot();
    assert_eq!(
        (creation.legacy_assembly, creation.legacy_prehistory),
        (1, 1)
    );
    assert_eq!((creation.simple_advance, creation.simple_finalize), (0, 0));
    source_probe::reset();
    session.end_civil_day().unwrap();
    let settled = source_probe::snapshot();
    assert_eq!(settled.legacy_advance, 1);
    assert_eq!(
        (
            settled.legacy_assembly,
            settled.legacy_prehistory,
            settled.simple_advance,
            settled.simple_finalize
        ),
        (0, 0, 0, 0)
    );
}

#[test]
fn selected_simple_month_end_projection_failure_preserves_all_facts_then_retries_once() {
    let setup = month_end_setup(true);
    let mut session = GameSession::new(setup.clone(), 42).unwrap();
    #[cfg(not(target_family = "wasm"))]
    if crate::runtime::step_runtime_enabled() {
        // 先固定合法的静态接管镜像，避免失败前首次 lazy 接管污染存档基线。
        session.construct_runtime_driver_lazy_for_test().unwrap();
    }
    let before = serde_json::to_vec(&session.save().unwrap()).unwrap();
    let before_hash = session.business_state_hash().unwrap();
    let old_history = session.company_system().unwrap().history().len();
    source_probe::reset();
    let result = session.with_prepared_civil_day(|candidate, report| {
        assert_eq!(
            candidate.company_system().unwrap().history().len(),
            old_history + 1
        );
        assert_eq!(
            candidate.company_system().unwrap().advanced_through(),
            report.settled_date
        );
        Err::<(), _>(SessionError::Step(StepFatal::InvariantViolation {
            location: "source_b.final_projection_probe".into(),
            description: "真实Simple已推进并定稿，最后可失败投影主动拒绝".into(),
        }))
    });
    assert!(
        matches!(result, Err(SessionError::Step(StepFatal::InvariantViolation { location, .. }))
        if location == "source_b.final_projection_probe")
    );
    let failed = source_probe::snapshot();
    assert_no_legacy(failed);
    assert_eq!((failed.simple_advance, failed.simple_finalize), (1, 1));
    assert_eq!(
        serde_json::to_vec(&session.save().unwrap()).unwrap(),
        before,
        "完整公司/RNG/公开库/披露游标/个人态/时钟必须逐字保留"
    );
    assert_eq!(session.business_state_hash().unwrap(), before_hash);
    assert!(session.poison_reason().is_none());
    source_probe::reset();
    session.end_civil_day().unwrap();
    let retried = source_probe::snapshot();
    assert_no_legacy(retried);
    assert_eq!((retried.simple_advance, retried.simple_finalize), (1, 1));
    let mut control = GameSession::new(setup, 42).unwrap();
    control.end_civil_day().unwrap();
    assert_eq!(
        serde_json::to_vec(&session.save().unwrap()).unwrap(),
        serde_json::to_vec(&control.save().unwrap()).unwrap(),
        "失败后重试与从未失败的同seed真实日结完全相同，不能重复结算或消耗RNG"
    );
}

#[test]
fn selected_simple_new_publication_failure_preserves_library_cursor_and_rng_then_retries() {
    use crate::information::{
        MonthlyReportDelay, MonthlyReportSchedule, PublicationSource, ReportFrequency,
    };
    let mut setup = month_end_setup(true);
    // 周日隔离未安装市场的 civil 候选拒绝；交易日已安装边界由下一独立用例固定。
    setup.start_date = crate::CivilDate::from_iso("2026-09-06").unwrap();
    setup.report_frequency = Some(ReportFrequency::Monthly {
        schedule: MonthlyReportSchedule::Custom {
            day: 6,
            second_of_day: 20 * 3600,
            delay: MonthlyReportDelay::None,
        },
    });
    let mut session = GameSession::new(setup.clone(), 42).unwrap();
    if matches!(session.civil_clock().phase(), CivilPhase::IntradayTrading) {
        session.step().unwrap();
    }
    assert!(
        session.state.runtime_resting_snapshot.is_none(),
        "本门必须未安装市场"
    );
    assert_eq!(session.tick(), 0, "周日披露不能靠推进市场制造");
    #[cfg(not(target_family = "wasm"))]
    if crate::runtime::step_runtime_enabled() {
        // 先固定合法的静态接管镜像，避免失败前首次 lazy 接管污染存档基线。
        session.construct_runtime_driver_lazy_for_test().unwrap();
    }
    let before_save = session.save().unwrap();
    let old_reports = before_save.public_library.report_count();
    let before = serde_json::to_vec(&before_save).unwrap();
    let before_hash = session.business_state_hash().unwrap();
    source_probe::reset();
    let result = session.with_prepared_civil_day(|candidate, report| {
        let save = candidate.save().unwrap();
        let library = save.public_library.save();
        assert!(
            library.reports.len() > old_reports,
            "必须先真实新增披露再拒绝投影"
        );
        assert!(library
            .reports
            .iter()
            .all(|item| item.source == PublicationSource::SimpleGenerated));
        assert!(library.reports.iter().any(|item| {
            item.published_at.date() == setup.start_date
                && item.published_at.second_of_day() == 20 * 3600
        }));
        assert_eq!(report.disclosure_instant.second_of_day(), 86_399);
        Err::<(), _>(SessionError::Step(StepFatal::InvariantViolation {
            location: "source_b.published_projection_probe".into(),
            description: "真实Simple公开库已新增报告，最后可失败投影主动拒绝".into(),
        }))
    });
    assert!(
        matches!(result, Err(SessionError::Step(StepFatal::InvariantViolation { location, .. }))
        if location == "source_b.published_projection_probe")
    );
    let failed = source_probe::snapshot();
    assert_no_legacy(failed);
    assert_eq!((failed.simple_advance, failed.simple_finalize), (1, 0));
    assert_eq!(
        serde_json::to_vec(&session.save().unwrap()).unwrap(),
        before,
        "新增报告与披露游标必须同公司/RNG/个人态/时钟一起留在失败candidate内"
    );
    assert_eq!(session.business_state_hash().unwrap(), before_hash);
    assert!(session.poison_reason().is_none());
    source_probe::reset();
    session.end_civil_day().unwrap();
    let retried = source_probe::snapshot();
    assert_no_legacy(retried);
    assert_eq!((retried.simple_advance, retried.simple_finalize), (1, 0));
    assert!(session.save().unwrap().public_library.report_count() > old_reports);
    let mut control = GameSession::new(setup, 42).unwrap();
    if matches!(control.civil_clock().phase(), CivilPhase::IntradayTrading) {
        control.step().unwrap();
    }
    control.end_civil_day().unwrap();
    assert_eq!(
        serde_json::to_vec(&session.save().unwrap()).unwrap(),
        serde_json::to_vec(&control.save().unwrap()).unwrap(),
        "新增公开报告失败后重试必须与无失败同seed控制逐字一致"
    );
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn selected_simple_publication_failure_after_market_install_is_terminal_without_retry() {
    use crate::information::{
        MonthlyReportDelay, MonthlyReportSchedule, PublicationSource, ReportFrequency,
    };
    let mut setup = month_end_setup(true);
    setup.start_date = crate::CivilDate::from_iso("2026-01-06").unwrap();
    setup.report_frequency = Some(ReportFrequency::Monthly {
        schedule: MonthlyReportSchedule::Custom {
            day: 6,
            second_of_day: 20 * 3600,
            delay: MonthlyReportDelay::None,
        },
    });
    let mut session = GameSession::new(setup.clone(), 42).unwrap();
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut session).unwrap();
    session.runtime_step_with_install(&mut driver).unwrap();
    assert!(
        session.state.runtime_resting_snapshot.is_some(),
        "必须经过真实市场安装"
    );
    assert_eq!(
        (session.tick(), session.day(), driver.world_committed_tick()),
        (1, 1, 1)
    );
    let installed = session.save().unwrap();
    let installed_bytes = serde_json::to_vec(&installed).unwrap();
    let installed_hash = session.business_state_hash().unwrap();
    let installed_snapshot = serde_json::to_value(session.snapshot_inner(true, true)).unwrap();
    let published = driver.publication().load();
    let old_reports = installed.public_library.report_count();
    let fatal = StepFatal::InvariantViolation {
        location: "source_b.installed_publication_projection_probe".into(),
        description: "真实市场已安装，Simple新增报告候选的最后投影拒绝".into(),
    };
    source_probe::reset();
    let result = session.with_prepared_civil_day(|candidate, report| {
        let save = candidate.save().unwrap();
        let library = save.public_library.save();
        assert!(
            library.reports.len() > old_reports,
            "必须真实新增公开报告后失败"
        );
        assert!(library
            .reports
            .iter()
            .all(|item| item.source == PublicationSource::SimpleGenerated));
        assert!(library
            .reports
            .iter()
            .any(|item| item.published_at.date() == setup.start_date
                && item.published_at.second_of_day() == 20 * 3600));
        assert_eq!(report.disclosure_instant.second_of_day(), 86_399);
        Err::<(), _>(SessionError::Step(fatal.clone()))
    });
    assert!(matches!(result, Err(SessionError::Step(ref actual)) if actual == &fatal));
    let counts = source_probe::snapshot();
    assert_no_legacy(counts);
    assert_eq!((counts.simple_advance, counts.simple_finalize), (1, 0));
    assert_eq!(session.business_state_hash().unwrap(), installed_hash);
    // 只在测试内比较已安装事实投影；对外 save 必须仍走 terminal 健康门拒绝。
    assert_eq!(
        serde_json::to_vec(&session.save_projection(installed.runtime_state.clone())).unwrap(),
        installed_bytes,
        "公司/RNG/公开库/游标与市场已装事实均不得回退或半推进"
    );
    assert_eq!(
        serde_json::to_value(session.snapshot_inner(true, true)).unwrap(),
        installed_snapshot
    );
    assert_eq!(
        (session.tick(), session.day(), driver.world_committed_tick()),
        (1, 1, 1)
    );
    let retained = driver.publication().load();
    assert_eq!(
        (retained.tick, retained.generation),
        (published.tick, published.generation)
    );
    assert!(
        std::sync::Arc::ptr_eq(&retained.retained, &published.retained),
        "已发布retained不能撤回"
    );
    assert_eq!(session.poison_reason(), Some(&fatal));
    assert!(matches!(session.save(), Err(crate::SessionError::Step(actual)) if actual == fatal));
    assert_eq!(session.step().unwrap_err(), fatal);
    assert!(
        matches!(session.end_civil_day(), Err(SessionError::Step(ref actual)) if actual == &fatal)
    );
    assert_eq!(
        session.poison_runtime_step(StepFatal::InvariantViolation {
            location: "source_b.later_failure".into(),
            description: "后错不得覆盖首错".into(),
        }),
        fatal
    );
    assert_eq!(
        source_probe::snapshot(),
        counts,
        "terminal后重试不得再次推进Simple或发布报告"
    );
}

fn monthly_intraday_setup() -> SessionSetup {
    use crate::information::{MonthlyReportDelay, MonthlyReportSchedule, ReportFrequency};
    let mut setup = month_end_setup(true);
    setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
    setup.ticks_per_day = 120;
    setup.auction_ticks = 0;
    setup.closing_auction_ticks = 0;
    setup.report_frequency = Some(ReportFrequency::Monthly {
        schedule: MonthlyReportSchedule::Custom {
            day: 2,
            second_of_day: 10 * 3600 + 30 * 60,
            delay: MonthlyReportDelay::None,
        },
    });
    setup
}

fn monthly_query() -> crate::company::PublicReportQuery {
    crate::company::PublicReportQuery {
        company_id: 0,
        cursor: None,
        page_size: Some(100),
    }
}

#[test]
fn simple_monthly_report_is_public_at_intraday_tick_and_restore_does_not_republish() {
    let setup = monthly_intraday_setup();
    let mut session = GameSession::new(setup.clone(), 42).unwrap();
    for _ in 0..29 {
        session.step_pipeline_contract().unwrap();
    }
    assert!(!session
        .query_public_reports(&monthly_query())
        .unwrap()
        .reports
        .iter()
        .any(|report| report.period == "2029-12-31"
            && matches!(report.kind, crate::company::PublicReportKind::Monthly)));
    let events = session.step_pipeline_contract().unwrap();
    let publication = events
        .iter()
        .find_map(|event| match event {
            Event::CompanyDisclosurePublished {
                publication_id,
                published_at,
                ..
            } if published_at.date() == setup.start_date
                && published_at.second_of_day() == 37_800 =>
            {
                Some(*publication_id)
            }
            _ => None,
        })
        .expect("10:30 交易日必须有真实公开事件");
    let page = session.query_public_reports(&monthly_query()).unwrap();
    let report = page
        .reports
        .iter()
        .find(|report| {
            report.period == "2029-12-31"
                && matches!(report.kind, crate::company::PublicReportKind::Monthly)
        })
        .expect("10:30 后玩家查询必须看到月报");
    assert_eq!(
        session
            .public_report_by_id(report.id.clone())
            .unwrap()
            .period,
        "2029-12-31"
    );
    let saved = session.save().unwrap();
    let mut restored = GameSession::restore(&saved).unwrap();
    let before = restored
        .query_public_reports(&monthly_query())
        .unwrap()
        .reports
        .iter()
        .filter(|report| {
            report.period == "2029-12-31"
                && matches!(report.kind, crate::company::PublicReportKind::Monthly)
        })
        .count();
    assert_eq!(before, 1);
    let events = restored.step_pipeline_contract().unwrap();
    assert!(!events.iter().any(|event| {
        matches!(event, Event::CompanyDisclosurePublished { publication_id, .. } if *publication_id == publication)
    }));
    let after = restored
        .query_public_reports(&monthly_query())
        .unwrap()
        .reports
        .iter()
        .filter(|report| {
            report.period == "2029-12-31"
                && matches!(report.kind, crate::company::PublicReportKind::Monthly)
        })
        .count();
    assert_eq!(after, 1, "恢复续跑不能重复公布同一月报");
}

#[test]
fn failed_tick_after_intraday_monthly_publication_rolls_back_library_and_information() {
    let mut setup = monthly_intraday_setup();
    setup.npcs.inst_count = 1;
    let mut session = GameSession::new(setup.clone(), 42).unwrap();
    let account = AccountId(1);
    let code = StockId(0);
    session
        .state
        .belief_participants
        .get_mut(&account)
        .unwrap()
        .watchlist_mut()
        .record_attention(&code, 0, 0)
        .unwrap();
    for _ in 0..29 {
        session.step_pipeline_contract().unwrap();
    }
    let before_hash = session.business_state_hash().unwrap();
    let before_library = session.state.library.clone();
    let before_information = session.state.belief_participants[&account]
        .information()
        .clone();
    let fatal = StepFatal::InvariantViolation {
        location: "monthly.candidate_ready".into(),
        description: "commit 前失败必须丢弃私有月报".into(),
    };
    session.inject_post_shadow_failure(fatal.clone());
    assert!(matches!(
        session.step_pipeline_contract(),
        Err(StepFatal::InvariantViolation { ref location, .. }) if location == "monthly.candidate_ready"
    ));
    assert_eq!(session.state.library, before_library);
    assert_eq!(
        session.state.belief_participants[&account].information(),
        &before_information
    );
    assert_eq!(session.business_state_hash().unwrap(), before_hash);

    // 同夹具控制组证明若未注入失败，该拍确实会新增公开材料；
    // 因而上面的零漂移不是“本来没有候选”的空断言。
    let mut control = GameSession::new(setup, 42).unwrap();
    for _ in 0..29 {
        control.step_pipeline_contract().unwrap();
    }
    assert!(control
        .step_pipeline_contract()
        .unwrap()
        .iter()
        .any(|event| matches!(
            event,
            Event::CompanyDisclosurePublished { published_at, .. }
                if published_at.second_of_day() == 37_800
        )));
}

#[test]
fn subscribed_institution_learns_intraday_monthly_report_at_its_publication_time() {
    let mut setup = monthly_intraday_setup();
    setup.npcs.inst_count = 1;
    let mut session = GameSession::new(setup, 42).unwrap();
    let account = AccountId(1);
    let code = StockId(0);
    session
        .state
        .belief_participants
        .get_mut(&account)
        .unwrap()
        .watchlist_mut()
        .record_attention(&code, 0, 0)
        .unwrap();
    for _ in 0..29 {
        session.step_pipeline_contract().unwrap();
    }
    let events = session.step_pipeline_contract().unwrap();
    let publication = events
        .iter()
        .find_map(|event| match event {
            Event::CompanyDisclosurePublished {
                publication_id,
                published_at,
                ..
            } if published_at.second_of_day() == 37_800 => Some(*publication_id),
            _ => None,
        })
        .expect("交易日盘中必须真实公布月报");
    let saved = session.save().unwrap();
    let information = saved.information_states.get(&account).unwrap();
    let record = information
        .records_for_company(&crate::company::CompanyId(0))
        .iter()
        .find(|record| record.id == publication)
        .expect("订阅机构必须在公布时点真实获知月报");
    assert_eq!(record.observed_at.second_of_day(), 37_800);
    let belief = saved
        .belief_books
        .get(&account)
        .unwrap()
        .entry(&code)
        .unwrap();
    assert!(belief.used_report_ids.contains(&record.id));
}

#[test]
fn default_session_step_path_publishes_intraday_monthly_report() {
    let setup = monthly_intraday_setup();
    let mut session = GameSession::new(setup, 42).unwrap();
    for _ in 0..29 {
        session.step().unwrap();
    }
    let events = session.step().unwrap();
    let publication = events
        .iter()
        .find_map(|event| match event {
            Event::CompanyDisclosurePublished {
                publication_id,
                published_at,
                ..
            } if published_at.second_of_day() == 37_800 => Some(*publication_id),
            _ => None,
        })
        .expect("默认 Session step 必须真实公布盘中月报");
    assert_eq!(
        session
            .query_public_reports(&monthly_query())
            .unwrap()
            .reports
            .iter()
            .filter(|report| {
                report.period == "2029-12-31"
                    && matches!(report.kind, crate::company::PublicReportKind::Monthly)
            })
            .count(),
        1
    );
    let saved = session.save().unwrap();
    let mut restored = GameSession::restore(&saved).unwrap();
    let events = restored.step().unwrap();
    assert!(!events.iter().any(|event| {
        matches!(event, Event::CompanyDisclosurePublished { publication_id, .. } if *publication_id == publication)
    }));
    assert_eq!(
        restored
            .query_public_reports(&monthly_query())
            .unwrap()
            .reports
            .iter()
            .filter(|report| {
                report.period == "2029-12-31"
                    && matches!(report.kind, crate::company::PublicReportKind::Monthly)
            })
            .count(),
        1
    );
}

#[test]
fn monthly_report_stays_visible_after_last_market_tick_before_civil_day_end() {
    let setup = monthly_intraday_setup();
    let mut session = GameSession::new(setup, 42).unwrap();
    for _ in 0..29 {
        session.step_pipeline_contract().unwrap();
    }
    session.step_pipeline_contract().unwrap();
    let report = session
        .query_public_reports(&monthly_query())
        .unwrap()
        .reports
        .into_iter()
        .find(|report| {
            report.period == "2029-12-31"
                && matches!(report.kind, crate::company::PublicReportKind::Monthly)
        })
        .unwrap();
    for _ in 30..120 {
        session.step_pipeline_contract().unwrap();
    }
    assert_eq!(session.tick(), 120);
    assert_eq!(session.day(), 1);
    assert_eq!(
        session
            .public_report_by_id(report.id.clone())
            .unwrap()
            .period,
        "2029-12-31",
        "最后交易 tick 后、自然日日结前不得回退观察时点"
    );
    let restored = GameSession::restore(&session.save().unwrap()).unwrap();
    assert_eq!(
        restored.public_report_by_id(report.id).unwrap().period,
        "2029-12-31"
    );
}

#[test]
fn save_rejects_disclosure_cursor_beyond_current_observation() {
    let setup = monthly_intraday_setup();
    let mut session = GameSession::new(setup, 42).unwrap();
    for _ in 0..29 {
        session.step_pipeline_contract().unwrap();
    }
    session.step_pipeline_contract().unwrap();
    let saved = session.save().unwrap();
    let mut value = serde_json::to_value(&saved).unwrap();
    value["disclosures"]["published_through"] = serde_json::json!({
        "date": "2030-01-02",
        "second_of_day": 54_000
    });
    let tampered: crate::session::SaveSlot = serde_json::from_value(value).unwrap();
    let error = match GameSession::restore(&tampered) {
        Ok(_) => panic!("未来披露游标必须显式拒绝"),
        Err(error) => error,
    };
    assert!(
        matches!(error, crate::SessionError::InvalidSave(ref fault) if fault.code == 2106),
        "未来披露游标必须显式拒绝：{error:?}"
    );
}

#[test]
fn runtime_publishes_at_1030_and_due_subscriber_learns_at_1100() {
    let mut setup = monthly_intraday_setup();
    setup.npcs.retail_count = 1;
    let mut session = GameSession::new(setup, 42).unwrap();
    let account = AccountId(1);
    let code = StockId(0);
    session
        .state
        .belief_participants
        .get_mut(&account)
        .unwrap()
        .watchlist_mut()
        .record_attention(&code, 0, 0)
        .unwrap();
    let today = session.civil_date();
    let attention = session.state.npc_attention.get_mut(&account).unwrap();
    attention.information_cadence =
        crate::session::notices::NpcInformationCadence::EveryDays { days: 1 };
    attention.next_information_check = crate::CivilInstant::new(today, 11 * 3600).unwrap();
    for _ in 0..29 {
        session.step().unwrap();
    }
    let events = session.step().unwrap();
    let publication = events
        .iter()
        .find_map(|event| match event {
            Event::CompanyDisclosurePublished {
                publication_id,
                published_at,
                ..
            } if published_at.second_of_day() == 37_800 => Some(*publication_id),
            _ => None,
        })
        .expect("native runtime 必须在 10:30 真实公布月报");
    assert!(
        session.state.belief_participants[&account]
            .information()
            .observed_at_of(publication)
            .is_none(),
        "cadence 未到期前不能冒充 10:30 获知"
    );
    for _ in 31..=46 {
        session.step().unwrap();
    }
    let saved = session.save().unwrap();
    let record = saved
        .information_states
        .get(&account)
        .unwrap()
        .records_for_company(&crate::company::CompanyId(0))
        .iter()
        .find(|record| record.id == publication)
        .expect("11:00 到期后现有 INFO 链必须真实获知月报");
    assert_eq!(record.observed_at.second_of_day(), 11 * 3600);
    let next = saved.npc_attention[&account].next_information_check;
    assert_eq!(next.date(), today.next().unwrap());
    assert_eq!(next.second_of_day(), 11 * 3600);
}

#[test]
fn restored_intraday_monthly_report_is_learned_at_1100_without_republishing() {
    let mut setup = monthly_intraday_setup();
    setup.npcs.retail_count = 1;
    let mut session = GameSession::new(setup, 42).unwrap();
    let account = AccountId(1);
    let code = StockId(0);
    session
        .state
        .belief_participants
        .get_mut(&account)
        .unwrap()
        .watchlist_mut()
        .record_attention(&code, 0, 0)
        .unwrap();
    let today = session.civil_date();
    let attention = session.state.npc_attention.get_mut(&account).unwrap();
    attention.information_cadence =
        crate::session::notices::NpcInformationCadence::EveryDays { days: 1 };
    attention.next_information_check = crate::CivilInstant::new(today, 11 * 3600).unwrap();
    for _ in 0..29 {
        session.step().unwrap();
    }
    let events = session.step().unwrap();
    let publication = events
        .iter()
        .find_map(|event| match event {
            Event::CompanyDisclosurePublished {
                publication_id,
                published_at,
                ..
            } if published_at.second_of_day() == 37_800 => Some(*publication_id),
            _ => None,
        })
        .expect("保存前必须在 10:30 真实公布");
    let saved = session.save().unwrap();
    let mut restored = GameSession::restore(&saved).unwrap();
    for _ in 0..16 {
        let events = restored.step().unwrap();
        assert!(!events.iter().any(|event| {
            matches!(event, Event::CompanyDisclosurePublished { publication_id, .. } if *publication_id == publication)
        }));
    }
    let saved = restored.save().unwrap();
    let record = saved
        .information_states
        .get(&account)
        .unwrap()
        .records_for_company(&crate::company::CompanyId(0))
        .iter()
        .find(|record| record.id == publication)
        .expect("恢复后 11:00 到期仍必须真实获知");
    assert_eq!(record.observed_at.second_of_day(), 11 * 3600);
    let next = saved.npc_attention[&account].next_information_check;
    assert_eq!(next.date(), today.next().unwrap());
    assert_eq!(next.second_of_day(), 11 * 3600);
}

#[test]
fn info_delivery_failure_after_intraday_publication_is_terminal_and_reported() {
    let mut setup = monthly_intraday_setup();
    setup.npcs.retail_count = 1;
    let mut session = GameSession::new(setup, 42).unwrap();
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut session).unwrap();
    for _ in 0..29 {
        session.runtime_step_with_install(&mut driver).unwrap();
    }
    let before_reports = session.state.library.report_count();
    driver.inject_info_delivery_failure_for_test();
    let fatal = session
        .runtime_step_with_install(&mut driver)
        .expect_err("INFO 失败必须显式返回首错");
    assert!(matches!(
        &fatal,
        StepFatal::InvariantViolation { location, description }
            if location == "runtime_mirror::information_delivery"
                && description.contains("INFO 投递失败注入")
    ));
    assert!(
        session.state.library.report_count() > before_reports,
        "失败发生在公开安装后的 INFO 阶段，不能伪称公开未提交"
    );
    assert_eq!(session.poison_reason(), Some(&fatal));
    assert!(matches!(session.save(), Err(SessionError::Step(actual)) if actual == fatal));
    assert_eq!(session.step().unwrap_err(), fatal);
    assert_eq!(
        session.state.library.report_count(),
        before_reports + 1,
        "terminal 后不得重试并重复发布"
    );
}

#[test]
#[cfg(not(target_family = "wasm"))]
fn 实际月报投递的非空回执后宿主故障锁定_session_并拒绝存档() {
    fn prepared() -> (GameSession, crate::runtime::drive::RuntimeDriver) {
        let mut setup = monthly_intraday_setup();
        setup.npcs.retail_count = 2;
        let mut session = GameSession::new(setup, 42).unwrap();
        for account in [AccountId(1), AccountId(2)] {
            session.state.belief_participants.get_mut(&account).unwrap()
                .watchlist_mut().record_attention(&StockId(0), 0, 0).unwrap();
            let attention = session.state.npc_attention.get_mut(&account).unwrap();
            attention.information_cadence = crate::session::notices::NpcInformationCadence::Immediate;
        }
        let mut driver = crate::runtime::drive::RuntimeDriver::from_session_grouped(&mut session, 2).unwrap();
        for _ in 0..29 { session.runtime_step_with_install(&mut driver).unwrap(); }
        (session, driver)
    }

    let (mut control, mut control_driver) = prepared();
    let events = control.runtime_step_with_install(&mut control_driver).unwrap();
    let publication = events.iter().find_map(|event| match event {
        Event::CompanyDisclosurePublished { publication_id, .. } => Some(*publication_id),
        _ => None,
    }).expect("对照组必须实际公布月报");
    for account in [AccountId(1), AccountId(2)] {
        assert!(control.state.belief_participants[&account].information().observed_at_of(publication).is_some(), "对照组必须实际获知月报");
    }

    let (mut session, mut driver) = prepared();
    let before_world_tick = driver.world_committed_tick();
    let before_reports = session.state.library.report_count();
    let before_information: Vec<_> = [AccountId(1), AccountId(2)].iter()
        .map(|account| session.state.belief_participants[account].information().clone()).collect();
    driver.inject_delivery_host_abort_for_test(1);
    let fatal = session.runtime_step_with_install(&mut driver).expect_err("投递阶段宿主故障必须终止 Session");
    assert!(matches!(&fatal, StepFatal::InvariantViolation { location, description }
        if location == "runtime_mirror::information_delivery" && description.contains("HostAborted")));
    assert!(driver.first_delivery_reply_updates_for_test().is_some_and(|count| count > 0), "故障前必须真实收到非空更新");
    assert_eq!(driver.world_committed_tick(), before_world_tick + 1, "World tick 已在信息投递之前提交");
    assert_eq!(session.state.library.report_count(), before_reports + 1, "故障发生前月报已真实公开");
    assert!(session.state.library.save().reports.iter().any(|report| report.id == publication), "故障组必须包含对照组同一公开事实");
    let after_information: Vec<_> = [AccountId(1), AccountId(2)].iter()
        .map(|account| session.state.belief_participants[account].information().clone()).collect();
    assert_eq!(after_information, before_information, "部分信息回执不得安装到 Session");
    assert_eq!(session.runtime_attention_full_states(), driver.delivery_start_attention_for_test().expect("必须记录真实投递前的注意力"), "部分回执不得安装注意力");
    assert_eq!(session.poison_reason(), Some(&fatal));
    assert!(matches!(session.save(), Err(SessionError::Step(actual)) if actual == fatal));
    let tick_after_failure = session.tick();
    assert_eq!(session.step().unwrap_err(), fatal);
    assert_eq!(session.tick(), tick_after_failure);
}

#[test]
#[cfg(not(target_family = "wasm"))]
fn 内置_driver_实际月报投递宿主故障锁定首错并拒绝存档() {
    assert!(crate::runtime::step_runtime_enabled(), "本测试必须使用生产 runtime");
    fn prepared() -> GameSession {
        let mut setup = monthly_intraday_setup();
        setup.npcs.retail_count = 2;
        let mut session = GameSession::new_with_runtime_groups(setup, 42, 2).unwrap();
        for account in [AccountId(1), AccountId(2)] {
            session.state.belief_participants.get_mut(&account).unwrap()
                .watchlist_mut().record_attention(&StockId(0), 0, 0).unwrap();
            session.state.npc_attention.get_mut(&account).unwrap().information_cadence =
                crate::session::notices::NpcInformationCadence::Immediate;
        }
        for _ in 0..29 { session.step().unwrap(); }
        session
    }

    let mut control = prepared();
    let events = control.step().unwrap();
    let publication = events.iter().find_map(|event| match event {
        Event::CompanyDisclosurePublished { publication_id, .. } => Some(*publication_id),
        _ => None,
    }).expect("对照组必须实际公布月报");
    for account in [AccountId(1), AccountId(2)] {
        assert!(control.state.belief_participants[&account].information().observed_at_of(publication).is_some());
    }

    let mut session = prepared();
    let before_reports = session.state.library.report_count();
    let before_information: Vec<_> = [AccountId(1), AccountId(2)].iter()
        .map(|account| session.state.belief_participants[account].information().clone()).collect();
    let before_world_tick = {
        let mut driver = session.runtime_driver_for_test().expect("必须使用内置 driver").lock().unwrap();
        let tick = driver.world_committed_tick();
        driver.inject_delivery_host_abort_for_test(1);
        tick
    };
    let fatal = session.step().expect_err("信息投递宿主故障必须终止 Session");
    assert!(matches!(&fatal, StepFatal::InvariantViolation { location, description }
        if location == "runtime_mirror::information_delivery" && description.contains("HostAborted")));
    {
        let driver = session.runtime_driver_for_test().expect("失败后仍须可观察原 driver").lock().unwrap();
        assert_eq!(driver.world_committed_tick(), before_world_tick + 1);
        assert!(driver.world_terminal_for_test(), "宿主永久故障必须使内置 World 判死");
        assert!(driver.first_delivery_reply_updates_for_test().is_some_and(|count| count > 0));
        assert_eq!(session.runtime_attention_full_states(), driver.delivery_start_attention_for_test().expect("必须记录投递起始注意力"));
    }
    assert_eq!(session.state.library.report_count(), before_reports + 1);
    assert!(session.state.library.save().reports.iter().any(|report| report.id == publication));
    let after_information: Vec<_> = [AccountId(1), AccountId(2)].iter()
        .map(|account| session.state.belief_participants[account].information().clone()).collect();
    assert_eq!(after_information, before_information, "部分回执不得安装 Session 信息态");
    assert_eq!(session.poison_reason(), Some(&fatal));
    assert!(matches!(session.save(), Err(SessionError::Step(actual)) if actual == fatal));
    let tick_after_failure = session.tick();
    assert_eq!(session.step().unwrap_err(), fatal);
    assert_eq!(session.tick(), tick_after_failure);
}

#[test]
fn restored_day_end_monthly_report_is_learned_at_next_day_1100() {
    use crate::information::{MonthlyReportDelay, MonthlyReportSchedule, ReportFrequency};
    let mut setup = monthly_intraday_setup();
    setup.npcs.retail_count = 1;
    setup.report_frequency = Some(ReportFrequency::Monthly {
        schedule: MonthlyReportSchedule::Custom {
            day: 2,
            second_of_day: 20 * 3600,
            delay: MonthlyReportDelay::None,
        },
    });
    let mut session = GameSession::new(setup, 42).unwrap();
    let account = AccountId(1);
    let code = StockId(0);
    session
        .state
        .belief_participants
        .get_mut(&account)
        .unwrap()
        .watchlist_mut()
        .record_attention(&code, 0, 0)
        .unwrap();
    let next_day = session.civil_date().next().unwrap();
    let attention = session.state.npc_attention.get_mut(&account).unwrap();
    attention.information_cadence =
        crate::session::notices::NpcInformationCadence::EveryDays { days: 1 };
    attention.next_information_check = crate::CivilInstant::new(next_day, 11 * 3600).unwrap();
    for _ in 0..120 {
        session.step().unwrap();
    }
    let report = session.end_civil_day().unwrap();
    let publication = report
        .events
        .iter()
        .find_map(|event| match event {
            Event::CompanyDisclosurePublished {
                publication_id,
                published_at,
                ..
            } if published_at.second_of_day() == 20 * 3600 => Some(*publication_id),
            _ => None,
        })
        .expect("自然日日结必须在 20:00 排期真实公开月报");
    assert_eq!(session.civil_date(), next_day);
    let saved = session.save().unwrap();
    let mut restored = GameSession::restore(&saved).unwrap();
    for _ in 0..46 {
        let events = restored.step().unwrap();
        assert!(!events.iter().any(|event| {
            matches!(event, Event::CompanyDisclosurePublished { publication_id, .. } if *publication_id == publication)
        }));
    }
    let saved = restored.save().unwrap();
    let record = saved
        .information_states
        .get(&account)
        .unwrap()
        .records_for_company(&crate::company::CompanyId(0))
        .iter()
        .find(|record| record.id == publication)
        .expect("日终公开后恢复，次日 11:00 到期仍必须获知");
    assert_eq!(record.observed_at.date(), next_day);
    assert_eq!(record.observed_at.second_of_day(), 11 * 3600);
}

#[test]
fn non_every_tick_sync_policies_still_publish_monthly_at_1030() {
    use crate::runtime::sync_policy::RuntimeSyncPolicy;
    for policy in [RuntimeSyncPolicy::DayEnd, RuntimeSyncPolicy::OnDemand] {
        let mut setup = monthly_intraday_setup();
        setup.runtime_sync_policy = policy;
        let mut session = GameSession::new(setup, 42).unwrap();
        for _ in 0..29 {
            session.step().unwrap();
        }
        let events = session.step().unwrap();
        assert!(
            events.iter().any(|event| matches!(
                event,
                Event::CompanyDisclosurePublished { published_at, .. }
                    if published_at.second_of_day() == 37_800
            )),
            "{policy:?} 下 10:30 必须真实公开"
        );
        assert_eq!(
            session
                .query_public_reports(&monthly_query())
                .unwrap()
                .reports
                .iter()
                .filter(|report| {
                    report.period == "2029-12-31"
                        && matches!(report.kind, crate::company::PublicReportKind::Monthly)
                })
                .count(),
            1,
            "{policy:?} 下查询必须看到已安装月报"
        );
    }
}

#[test]
fn monthly_disclosure_survives_runtime_step_frame_projection() {
    let setup = monthly_intraday_setup();
    let mut session = GameSession::new(setup, 42).unwrap();
    for _ in 0..29 {
        session.step().unwrap();
    }
    let (frame, _) = session
        .step_frame_and_commit_via_runtime(|projection, facts| {
            let frame = projection.project_frame(facts)?;
            frame
                .validate()
                .map_err(|error| StepFatal::InvariantViolation {
                    location: "monthly.step_frame_validate".into(),
                    description: error.to_string(),
                })?;
            Ok(frame)
        })
        .unwrap();
    assert!(frame.facts.iter().any(|fact| matches!(
        fact.event,
        Event::CompanyDisclosurePublished { published_at, .. }
            if published_at.second_of_day() == 37_800
    )));
    assert_eq!(
        session
            .query_public_reports(&monthly_query())
            .unwrap()
            .reports
            .iter()
            .filter(|report| {
                report.period == "2029-12-31"
                    && matches!(report.kind, crate::company::PublicReportKind::Monthly)
            })
            .count(),
        1
    );
}

/// 默认名册规模的新增 cadence 扫描预算：缓存命中时每拍只做 O(1) 比较；
/// 仅在缓存缺失或跨过最早到期时点后扫描一次。显式用长验收 deadline 运行。
#[test]
#[ignore = "performance measurement; run explicitly under a 300s external deadline"]
fn q23_default_roster_cadence_scan_stays_bounded() {
    use crate::session::account_paged_map::AccountPagedMap;
    use std::time::{Duration, Instant};
    let start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
    let next_day = start_date.next().unwrap();
    let now = crate::CivilInstant::new(start_date, 11 * 3600).unwrap();
    let mut attention = AccountPagedMap::default();
    for id in 1..=20_000u64 {
        attention.insert(
            AccountId(id),
            crate::session::NpcAttentionState {
                information_cadence: crate::session::notices::NpcInformationCadence::EveryDays {
                    days: 1,
                },
                next_information_check: crate::CivilInstant::new(next_day, 11 * 3600).unwrap(),
                base_probability: 0.5,
                next_attention_candidate_tick: 0,
                rng_state: 0,
            },
        );
    }
    let scan_started = Instant::now();
    let mut floor = attention
        .values()
        .filter(|state| {
            state.information_cadence != crate::session::notices::NpcInformationCadence::Immediate
        })
        .map(|state| state.next_information_check)
        .min();
    let scan_elapsed = scan_started.elapsed();
    let cached_started = Instant::now();
    for _ in 0..15_300 {
        if floor.is_none_or(|value| now >= value) {
            floor = attention
                .values()
                .filter(|state| {
                    state.information_cadence
                        != crate::session::notices::NpcInformationCadence::Immediate
                })
                .map(|state| state.next_information_check)
                .min();
        }
        std::hint::black_box(floor);
    }
    let cached_elapsed = cached_started.elapsed();
    eprintln!(
        "Q23 default roster cadence cache: initial_scan={scan_elapsed:?}, 15300 cached ticks={cached_elapsed:?}"
    );
    assert!(
        cached_elapsed < Duration::from_secs(5),
        "default roster cached cadence gate exceeded 5s: {cached_elapsed:?}"
    );
}

#[test]
fn runtime_1100_acquisition_survives_immediate_save_restore() {
    let mut setup = monthly_intraday_setup();
    setup.npcs.retail_count = 1;
    let mut session = GameSession::new(setup, 42).unwrap();
    let account = AccountId(1);
    let code = StockId(0);
    session
        .state
        .belief_participants
        .get_mut(&account)
        .unwrap()
        .watchlist_mut()
        .record_attention(&code, 0, 0)
        .unwrap();
    let today = session.civil_date();
    let attention = session.state.npc_attention.get_mut(&account).unwrap();
    attention.information_cadence =
        crate::session::notices::NpcInformationCadence::EveryDays { days: 1 };
    attention.next_information_check = crate::CivilInstant::new(today, 11 * 3600).unwrap();
    for _ in 0..30 {
        session.step().unwrap();
    }
    for _ in 0..15 {
        session.step().unwrap();
    }
    let saved = session.save().unwrap();
    let restored = GameSession::restore(&saved).unwrap();
    let record = restored.state.belief_participants[&account]
        .information()
        .records_for_company(&crate::company::CompanyId(0))
        .iter()
        .find(|record| record.observed_at.second_of_day() == 11 * 3600)
        .expect("11:00 当拍 save 必须包含本次真实获知");
    assert_eq!(record.observed_at.second_of_day(), 11 * 3600);
}

#[test]
fn runtime_immediate_acquisition_survives_same_tick_save_restore() {
    let mut setup = monthly_intraday_setup();
    setup.npcs.inst_count = 1;
    let mut session = GameSession::new(setup, 42).unwrap();
    let account = AccountId(1);
    let code = StockId(0);
    session
        .state
        .belief_participants
        .get_mut(&account)
        .unwrap()
        .watchlist_mut()
        .record_attention(&code, 0, 0)
        .unwrap();
    for _ in 0..29 {
        session.step().unwrap();
    }
    let events = session.step().unwrap();
    let publication = events
        .iter()
        .find_map(|event| match event {
            Event::CompanyDisclosurePublished {
                publication_id,
                published_at,
                ..
            } if published_at.second_of_day() == 37_800 => Some(*publication_id),
            _ => None,
        })
        .expect("Immediate 订阅者所在拍必须公开月报");
    let saved = session.save().unwrap();
    let restored = GameSession::restore(&saved).unwrap();
    let record = restored.state.belief_participants[&account]
        .information()
        .records_for_company(&crate::company::CompanyId(0))
        .iter()
        .find(|record| record.id == publication)
        .expect("Immediate 10:30 当拍 save 必须包含获知");
    assert_eq!(record.observed_at.second_of_day(), 37_800);
}

#[test]
fn serial_fallback_publishes_at_1030_and_learns_at_1100() {
    let mut setup = monthly_intraday_setup();
    setup.npcs.retail_count = 1;
    let mut session = GameSession::new(setup, 42).unwrap();
    let account = AccountId(1);
    let code = StockId(0);
    session
        .state
        .belief_participants
        .get_mut(&account)
        .unwrap()
        .watchlist_mut()
        .record_attention(&code, 0, 0)
        .unwrap();
    let today = session.civil_date();
    let attention = session.state.npc_attention.get_mut(&account).unwrap();
    attention.information_cadence =
        crate::session::notices::NpcInformationCadence::EveryDays { days: 1 };
    attention.next_information_check = crate::CivilInstant::new(today, 11 * 3600).unwrap();
    for _ in 0..29 {
        session.step_pipeline_contract().unwrap();
    }
    let events = session.step_pipeline_contract().unwrap();
    let publication = events
        .iter()
        .find_map(|event| match event {
            Event::CompanyDisclosurePublished {
                publication_id,
                published_at,
                ..
            } if published_at.second_of_day() == 37_800 => Some(*publication_id),
            _ => None,
        })
        .expect("串行 fallback 必须在 10:30 公开月报");
    let floor_after_publish = session.information_check_floor();
    assert_eq!(
        floor_after_publish,
        Some(crate::CivilInstant::new(today, 11 * 3600).unwrap()),
        "首次公开必须清掉无 baseline 哨兵并装回真实 cadence floor"
    );
    session.step_pipeline_contract().unwrap();
    assert_eq!(
        session.information_check_floor(),
        floor_after_publish,
        "成功串行 commit 必须移动 candidate floor，后续拍保持缓存"
    );
    for _ in 0..14 {
        session.step_pipeline_contract().unwrap();
    }
    let saved = session.save().unwrap();
    let restored = GameSession::restore(&saved).unwrap();
    let record = restored.state.belief_participants[&account]
        .information()
        .records_for_company(&crate::company::CompanyId(0))
        .iter()
        .find(|record| record.id == publication)
        .expect("串行 fallback 必须在 11:00 真实获知");
    assert_eq!(record.observed_at.second_of_day(), 11 * 3600);
}

#[test]
fn non_every_tick_info_uses_group_cadence_and_keeps_group_rng() {
    let mut setup = monthly_intraday_setup();
    setup.npcs.retail_count = 1;
    setup.runtime_sync_policy = crate::runtime::sync_policy::RuntimeSyncPolicy::DayEnd;
    let mut session = GameSession::new(setup, 42).unwrap();
    let today = session.civil_date();
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut session).unwrap();
    session.runtime_step_with_install(&mut driver).unwrap();
    let cached = driver.collect_belief().unwrap();
    let mut patched = cached[0].1[1].as_ref().unwrap().clone();
    patched.attention.information_cadence =
        crate::session::notices::NpcInformationCadence::EveryDays { days: 1 };
    patched.attention.next_information_check = crate::CivilInstant::new(today, 11 * 3600).unwrap();
    patched.attention.rng_state = 0x1234_5678;
    patched.attention.next_attention_candidate_tick = 777;
    patched
        .watchlist
        .record_attention(&StockId(0), 0, 0)
        .unwrap();
    driver.install_personal_for_test(1, patched);
    session.runtime_step_with_install(&mut driver).unwrap();
    let group = driver.collect_belief_authority_for_test().unwrap();
    let authority = group[0]
        .1
        .iter()
        .find(|(local, _)| *local == 1)
        .map(|(_, snapshot)| snapshot)
        .unwrap();
    assert_eq!(authority.attention.rng_state, 0x1234_5678);
    assert_eq!(authority.attention.next_attention_candidate_tick, 777);
    let mut events = Vec::new();
    for _ in 0..28 {
        events = session.runtime_step_with_install(&mut driver).unwrap();
    }
    let publication = events
        .iter()
        .find_map(|event| match event {
            Event::CompanyDisclosurePublished {
                publication_id,
                published_at,
                ..
            } if published_at.second_of_day() == 37_800 => Some(*publication_id),
            _ => None,
        })
        .expect("10:30 必须公开月报");
    let account = AccountId(1);
    assert!(
        session.state.belief_participants[&account]
            .information()
            .observed_at_of(publication)
            .is_none(),
        "组内 cadence 已推进到 11:00，10:30 投递不得提前获知"
    );
    assert_eq!(
        session.information_check_floor(),
        Some(crate::CivilInstant::new(today, 11 * 3600).unwrap()),
        "INFO 回执必须把组权威 floor 写回 Session，避免每拍重复全量投递"
    );
    for _ in 0..15 {
        session.runtime_step_with_install(&mut driver).unwrap();
    }
    assert_eq!(
        session.information_check_floor(),
        Some(crate::CivilInstant::new(today.next().unwrap(), 11 * 3600).unwrap()),
        "有 dirty 个人态安装但没有新 cadence 变更时，已验证的未来 floor 必须保持"
    );
    let record = session.state.belief_participants[&account]
        .information()
        .records_for_company(&crate::company::CompanyId(0))
        .iter()
        .find(|record| record.id == publication)
        .expect("11:00 到期仍必须获知");
    assert_eq!(record.observed_at.second_of_day(), 11 * 3600);
}

/// 真实 GameSession floor/set/invalidate 路径的默认名册失效预算。
#[test]
#[ignore = "performance measurement; run explicitly under a 300s external deadline"]
fn q23_production_floor_cache_invalidation_stays_bounded() {
    use std::time::{Duration, Instant};
    let mut setup = monthly_intraday_setup();
    setup.npcs.retail_count = 20_000;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.ticks_per_day = 1;
    let mut session = GameSession::new(setup, 42).unwrap();
    let scan_started = Instant::now();
    let floor = session.scan_information_check_floor();
    let scan_elapsed = scan_started.elapsed();
    session.set_information_check_floor(floor);
    let cached_started = Instant::now();
    for _ in 0..15_300 {
        assert_eq!(session.information_check_floor(), floor);
    }
    let cached_elapsed = cached_started.elapsed();
    let account = AccountId(1);
    let mut due = session.state.npc_attention[&account].clone();
    due.information_cadence = crate::session::notices::NpcInformationCadence::EveryDays { days: 1 };
    due.next_information_check = session.observation_civil_instant();
    session.set_runtime_information_checks(&[(account, due)]);
    assert!(session.information_check_floor().is_none());
    let invalidation_started = Instant::now();
    let _ = session.scan_information_check_floor();
    let invalidation_elapsed = invalidation_started.elapsed();
    eprintln!(
        "Q23 production floor cache: scan={scan_elapsed:?}, 15300 O(1) hits={cached_elapsed:?}, invalidate+rescan={invalidation_elapsed:?}"
    );
    assert!(cached_elapsed < Duration::from_secs(5));
    assert!(scan_elapsed < Duration::from_secs(5));
    assert!(invalidation_elapsed < Duration::from_secs(5));
}
