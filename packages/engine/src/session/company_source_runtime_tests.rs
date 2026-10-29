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
