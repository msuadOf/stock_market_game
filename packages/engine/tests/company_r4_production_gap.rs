//! R4 生产入口缺口的最小代表覆盖：三组尾组名册、空个人组的 Simple 日结、
//! 晚间 Monthly 窗口与季度 18:00 扫描端点。
//!
//! **r4_07 未覆盖声明**：历史 `319bcfac` 的 r4_07 要求交易日 10:30 即时公开、
//! 11:00 本人获知并在下一 step 保留。Q23（`agents/remaining-questions-and-features/q23-report-frequency.md`）已由用户批准“交易日月报在私有 tick candidate 中公布、
//! 玩家查询使用当前观察时刻”，因此该语义是**已批准但 main 尚未实现的缺口**，
//! 不是未批准方案。r4_07 保持未核销，由单独的 Q23 实现任务处理。
//!
//! 本文件只固定现行实现与已批准契约都成立的事实：休市日按自然日日结公布/获知、
//! Monthly 排期秒值保留在 `published_at`、日终派发端点为 23:59:59、季度保持 18:00、
//! 三组尾组名册与空个人组推进。任何“交易日盘中不可见/只能在日终获知”的断言都不属于
//! 本文件，以免把 Q23 缺口锁成期望行为。

#[path = "publications/session_fixture.rs"]
mod session_fixture;
include!("../test-support/simple_company.rs");

use engine::accounting::{reports::ReportKind, AccountingPeriod};
use engine::company::{CompanyId, PublicReportQuery};
use engine::information::{
    MonthlyReportDelay, MonthlyReportSchedule, PublicationSource, PublishedReport, ReportFrequency,
};
use engine::session::NpcInformationCadence;
use engine::strategy::StrategyProfile;
use engine::{AccountId, CivilDate, CivilInstant, GameSession, SaveSlot, SessionSetup};

const SEED: u64 = 7;
const RUNTIME_GROUPS: usize = 3;

fn date(value: &str) -> CivilDate {
    CivilDate::from_iso(value).expect("夹具日期合法")
}

fn instant(day: CivilDate, second: u32) -> CivilInstant {
    CivilInstant::new(day, second).expect("夹具时点合法")
}

fn monthly(second: u32) -> ReportFrequency {
    ReportFrequency::Monthly {
        schedule: MonthlyReportSchedule::Custom {
            day: 1,
            second_of_day: second,
            delay: MonthlyReportDelay::None,
        },
    }
}

/// 真实 Simple 会话：5 retail + 1 inst + 1 hot，玩家与 7 名 NPC 共 8 个账户
/// 按 id 序均分给 3 个 runtime 组（末组只剩 2 个真实账户）。
fn trading_setup(start: &str, frequency: ReportFrequency) -> SessionSetup {
    let mut setup = session_fixture::civil_setup(date(start));
    setup.npcs.retail_count = 5;
    setup.npcs.inst_count = 1;
    setup.npcs.hot_count = 1;
    setup.ticks_per_day = 8;
    setup.report_frequency = Some(frequency);
    setup.company_system = Some(simple_company_fixture!(engine; ["600101"]));
    setup.dividend_tax_mode =
        Some(engine::company::cash_dividend_tax::CashDividendTaxMode::AShareIndividual);
    setup.par_value_per_share = Some(engine::Money::from_cents(100));
    setup
}

/// 空个人组：只有玩家和 Hot，真实 NPC 个人分析行为空；仍保留 3 个 runtime 组。
fn no_personal_setup(start: &str, frequency: ReportFrequency) -> SessionSetup {
    let mut setup = trading_setup(start, frequency);
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup
}

fn report_for_month(saved: &SaveSlot, year: i32, month: u8, now: CivilInstant) -> PublishedReport {
    saved
        .public_library
        .latest_report(
            &CompanyId(0),
            ReportKind::Monthly,
            AccountingPeriod::from_ymd(year, month).expect("夹具月份合法"),
            now,
        )
        .expect("真实排期经过后应已公布当期 Monthly")
        .clone()
}

#[test]
fn quarterly_closed_day_keeps_the_18_for_scan_endpoint() {
    let mut session = GameSession::new(session_fixture::civil_setup(date("2030-06-01")), SEED)
        .expect("休市初局可构造");
    assert!(session.company_system().is_none());
    let tick_before = session.tick();
    session
        .end_civil_day()
        .expect("无首次 market step 的休市局可日结");
    assert_eq!(session.tick(), tick_before, "休市日不得制造 market tick");
    assert_eq!(
        session
            .save()
            .expect("可读日结事实")
            .disclosures
            .published_through(),
        Some(instant(date("2030-06-01"), 18 * 3600)),
        "非 Monthly 频率的披露扫描以 18:00 为终点"
    );
}

#[test]
fn empty_personal_groups_settle_simple_company_at_month_end() {
    let mut session = GameSession::new_with_runtime_groups(
        no_personal_setup("2030-06-30", monthly(84_600)),
        SEED,
        RUNTIME_GROUPS,
    )
    .expect("空个人组仍可构造三组会话");
    let report = session.end_civil_day().expect("周日月末完成真实日结");
    assert_eq!(report.settled_date, date("2030-06-30"));
    assert_eq!(session.tick(), 0, "闭市日不得制造 market tick");
    assert_eq!(
        session.company_system().unwrap().advanced_through(),
        date("2030-06-30"),
        "没有个人重估仍须推进选中的 Simple 公司"
    );
}

#[test]
fn closed_day_monthly_is_published_at_schedule_second_but_observed_only_at_day_end() {
    let mut session = GameSession::new_with_runtime_groups(
        trading_setup("2030-06-01", monthly(10 * 3600 + 30 * 60)),
        SEED,
        RUNTIME_GROUPS,
    )
    .expect("休市初局可构造三组会话");
    let owners: Vec<AccountId> = session
        .save()
        .expect("可读初始事实")
        .information_states
        .keys()
        .copied()
        .collect();
    assert!(!owners.is_empty());

    let report = session.end_civil_day().expect("周六月报日完成真实日结");
    assert_eq!(report.settled_date, date("2030-06-01"));
    assert_eq!(session.tick(), 0, "休市日不得制造 market tick");
    let saved = session.save().expect("可读日结事实");
    let published = report_for_month(&saved, 2030, 5, instant(date("2030-06-01"), 86_399));
    assert_eq!(
        published.published_at,
        instant(date("2030-06-01"), 10 * 3600 + 30 * 60),
        "休市日同样保留排期秒值"
    );
    let readers: Vec<AccountId> = owners
        .iter()
        .filter(|owner| {
            saved.information_states[owner].observed_at_of(published.id)
                == Some(report.disclosure_instant)
        })
        .copied()
        .collect();
    assert!(!readers.is_empty(), "到期检查的真实参与者须在日结获知材料");
    for owner in &owners {
        if let Some(observed) = saved.information_states[owner].observed_at_of(published.id) {
            assert_eq!(
                observed, report.disclosure_instant,
                "休市日不得制造 10:30 的本人观察时点"
            );
        }
    }
}

#[test]
fn three_runtime_groups_keep_the_real_roster_and_tail_group_personal_rows() {
    let mut session = GameSession::new_with_runtime_groups(
        trading_setup("2031-04-01", monthly(84_600)),
        SEED,
        RUNTIME_GROUPS,
    )
    .expect("公共构造入口应接受合法的三组拓扑");
    let roster = session.account_roster();
    assert_eq!(roster.len(), 8, "玩家 + 5 retail + 1 inst + 1 hot");
    assert_eq!(
        roster,
        (0..8).map(AccountId).collect::<Vec<_>>(),
        "名册只含真实账户，不为组宽度补位"
    );

    let initial = session.save().expect("可读初始事实");
    // 第三组按 id 序只分到 AccountId(6)/AccountId(7)：机构 6 有个人分析行，
    // Hot 7 只有注意力状态，不能凭空得到个人行。
    assert!(initial.information_states.contains_key(&AccountId(6)));
    assert!(!initial.information_states.contains_key(&AccountId(7)));
    assert!(initial.npc_attention.contains_key(&AccountId(7)));
    assert!(matches!(
        session.account_strategy_profiles().get(&AccountId(7)),
        Some(StrategyProfile::Hot(_))
    ));

    session.step().expect("3 组真实 market step 可推进");
    session
        .step()
        .expect("下一真实 decision 不可使用 padding owner");
    let after = session.save().expect("可读后续事实");
    assert_eq!(
        initial.information_states.keys().collect::<Vec<_>>(),
        after.information_states.keys().collect::<Vec<_>>(),
        "步进不得增删个人分析行"
    );
    assert_eq!(session.account_roster(), roster, "步进不得改变真实名册");
    for (owner, personal) in &after.information_states {
        assert_eq!(*owner, personal.owner());
        assert!(
            session.account(*owner).is_some(),
            "个人行属主必须是真实账户"
        );
    }
}

#[test]
fn empty_personal_groups_settle_simple_company_and_keep_the_late_monthly_window() {
    let mut session = GameSession::new_with_runtime_groups(
        no_personal_setup("2030-06-01", monthly(84_600)),
        SEED,
        RUNTIME_GROUPS,
    )
    .expect("空个人组仍可构造三组会话");
    assert!(
        session
            .save()
            .expect("可读初始事实")
            .information_states
            .is_empty(),
        "只有 Hot 时不得补默认个人态"
    );

    let report = session.end_civil_day().expect("休市日仍须完成真实日结");
    assert_eq!(report.settled_date, date("2030-06-01"));
    assert_eq!(session.tick(), 0, "休市日不得制造 market tick");
    assert_eq!(
        session.company_system().unwrap().advanced_through(),
        date("2030-06-01"),
        "没有个人重估仍须推进选中的 Simple 公司"
    );

    let saved = session.save().expect("可读日结事实");
    let endpoint = instant(date("2030-06-01"), 86_399);
    assert_eq!(saved.disclosures.published_through(), Some(endpoint));
    let published = report_for_month(&saved, 2030, 5, endpoint);
    assert_eq!(published.published_at, instant(date("2030-06-01"), 84_600));
    assert_eq!(published.source, PublicationSource::SimpleGenerated);
    assert!(saved.information_states.is_empty());
}

#[test]
fn trading_day_monthly_keeps_schedule_second_and_retains_acquired_material() {
    let start = date("2031-04-01");
    let schedule_second = 10 * 3600 + 30 * 60;
    let mut session = GameSession::new_with_runtime_groups(
        trading_setup("2031-04-01", monthly(schedule_second)),
        SEED,
        RUNTIME_GROUPS,
    )
    .expect("公共构造入口应接受合法所选模型");
    let initial = session.save().expect("可读初始事实");
    let owners: Vec<AccountId> = initial.information_states.keys().copied().collect();
    assert!(!owners.is_empty(), "只使用初始化 producer 实际生成的个人行");
    // 把本人检查排到 11:00；不断言盘中是否已公开，避免锁死 Q23 缺口。
    let updates: Vec<_> = owners
        .iter()
        .map(|owner| {
            let mut attention = initial.npc_attention[owner].clone();
            attention.information_cadence = NpcInformationCadence::Daily { checks: 1 };
            attention.next_information_check = instant(start, 11 * 3600);
            (*owner, attention)
        })
        .collect();
    session.set_runtime_information_checks(&updates);

    for _ in 0..8 {
        session.step().expect("完整真实交易日可推进");
    }
    let report = session.end_civil_day().expect("交易日完成真实日结");
    assert_eq!(report.settled_date, start);
    // 从公开库按期间取报告，不以日结事件为唯一来源：Q23 接通盘中公布后，
    // 同一月报不应在日结重复派发，`report.events` 不能成为该例前提。
    let saved = session.save().expect("可读日结事实");
    let published = report_for_month(&saved, 2031, 3, instant(start, 86_399));
    assert_eq!(
        published.published_at,
        instant(start, schedule_second),
        "公开排期秒值必须保留，不被口径端点覆盖"
    );
    assert_eq!(published.source, PublicationSource::SimpleGenerated);

    let readers: Vec<AccountId> = owners
        .iter()
        .filter(|owner| {
            saved.information_states[owner]
                .observed_at_of(published.id)
                .is_some()
        })
        .copied()
        .collect();
    assert!(!readers.is_empty(), "到期检查的真实参与者须获知当期材料");
    for owner in &owners {
        if let Some(observed) = saved.information_states[owner].observed_at_of(published.id) {
            assert!(
                observed >= published.published_at && observed <= report.disclosure_instant,
                "获知不得早于公开排期，也不得晚于本次日结披露相位"
            );
        }
    }

    session
        .step()
        .expect("下一次真实 decision step 必须保留本人材料");
    let after_next = session.save().expect("可读后续事实");
    for owner in &readers {
        assert!(
            after_next.information_states[owner]
                .observed_at_of(published.id)
                .is_some(),
            "已获知材料不得在下一 step 丢失"
        );
    }
    let page = session
        .query_public_reports(&PublicReportQuery {
            company_id: 0,
            cursor: None,
            page_size: Some(100),
        })
        .expect("查询使用已提交可见时点");
    assert!(page
        .reports
        .iter()
        .any(|summary| summary.id == published.id.value().to_string()));
}
