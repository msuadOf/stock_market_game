//! 周末发布金样：非交易日 18:00 照常公布、零撮合零市场事件零 RNG
//! 消费；开局后首次排期经 live 派发落地；提前读取被类型化拒绝。

use crate::fixture::{history_start_of, plus_days, single_company_config, INDUSTRIAL_ID, OPS_SEED};
use crate::session_fixture::{civil_setup, TICKS_PER_DAY};
use engine::accounting::reports::ReportKind;
use engine::accounting::AccountingPeriod;
use engine::calendar::{CalendarExchange, CivilInstant, TradingCalendar, Weekday};
use engine::company::{ActiveShock, CompanyId, ShockKind};
use engine::information::{
    assemble_seeded_prehistory, scheduled_instant, stable_company_offset, InformationError,
    PublicationOrigin, ScheduledReportKind,
};
use engine::session::{
    CivilDayEndReport, DayEndDisclosureCtx, DayEndDisclosures, DisclosureDispatch, GameSession,
};

/// 找到「Q1 基准日 + 该公司真实偏移」落在周六、且其前一自然日（周五）是
/// 交易日的年份（确定性搜索：偏移是 seed+公司 id 的纯函数）。
fn saturday_q1_year(offset: u8) -> i32 {
    let calendar = TradingCalendar::current_default_calendar().expect("default calendar");
    for year in 2030i32..=2096 {
        let instant = scheduled_instant(ScheduledReportKind::Q1, year, offset)
            .expect("q1 schedule legal in search range");
        if instant.date().weekday() == Weekday::Saturday {
            let friday = instant.date().prev().expect("friday exists");
            if calendar
                .is_trading_day(CalendarExchange::Sse, friday)
                .expect("friday status")
            {
                return year;
            }
        }
    }
    panic!("no Saturday Q1 publication with a trading Friday found in 2030..=2096");
}

/// 组装本测试的完整状态：前史（窗口覆盖 Q1 全季）+ 已公开集合 + 周五开局的
/// 压缩会话。as_of = 前史首日前一天（无借款账套的调用方契约）。
struct WeekendScenario {
    year: i32,
    seeded: engine::information::SeededPrehistory,
    session: GameSession,
    dispatch: DisclosureDispatch,
}

impl WeekendScenario {
    fn new(offset: u8) -> Self {
        let year = saturday_q1_year(offset);
        let saturday = scheduled_instant(ScheduledReportKind::Q1, year, offset)
            .expect("saturday q1 instant")
            .date();
        let friday = saturday.prev().expect("friday before saturday");
        let seeded = assemble_seeded_prehistory(
            single_company_config(
                history_start_of(friday)
                    .prev()
                    .expect("as_of before history"),
            ),
            friday,
        )
        .expect("prehistory assembles");
        let session =
            GameSession::new(civil_setup(friday), OPS_SEED).expect("compact session valid");
        let dispatch = DisclosureDispatch::new(seeded.last_published_instant());
        Self {
            year,
            seeded,
            session,
            dispatch,
        }
    }

    fn install_dispatch(&mut self) {
        self.dispatch.install(self.session.civil_clock_mut());
    }

    fn step_friday(&mut self) {
        for _ in 0..TICKS_PER_DAY {
            self.session.step().expect("healthy step");
        }
    }

    /// 依次完成 Session 日结、公司经营 finalize 与披露；不推进市场 tick。
    fn settle_and_dispatch(&mut self) -> (CivilDayEndReport, DayEndDisclosures) {
        let report = self.session.end_civil_day().expect("scenario day-end");
        self.seeded
            .ops
            .advance_civil_day(report.settled_date)
            .expect("scenario finalize");
        let out = self
            .dispatch
            .run_day_end(DayEndDisclosureCtx {
                report: &report,
                ops: &self.seeded.ops,
                closing: &mut self.seeded.closing,
                library: &mut self.seeded.library,
            })
            .expect("scenario disclosure dispatch");
        (report, out)
    }
}

/// 完整场景：周五开局（跑完当日会话）→ 周六（休市）日结 → 18:00 披露。
/// 断言公布本身零市场副作用（事件 seq / day / tick / 快照全部不变）。
#[test]
fn weekend_report_publishes_without_trade() {
    let company = CompanyId(INDUSTRIAL_ID.to_string());
    let offset = stable_company_offset(OPS_SEED, &company);
    let mut scenario = WeekendScenario::new(offset);
    let year = scenario.year;
    let q1_instant = scheduled_instant(ScheduledReportKind::Q1, year, offset).expect("instant");

    // 前史装配：更早排期全部 seeded；本季 Q1 公布时点晚于开局，不得提前纳入。
    assert!(
        scenario.seeded.library.report_count() > 0,
        "prior schedule points already seeded"
    );
    assert!(
        scenario
            .seeded
            .library
            .latest_report(
                &company,
                ReportKind::Quarter,
                AccountingPeriod::from_ymd(year, 3).expect("q1 period"),
                q1_instant,
            )
            .is_none(),
        "future-dated Q1 must not be pre-included"
    );

    scenario.install_dispatch();

    // —— 周五：跑完当日会话，然后日结（finalize → 披露）——
    scenario.step_friday();
    let (_friday_report, friday_out) = scenario.settle_and_dispatch();
    assert!(
        friday_out.reports_published.is_empty(),
        "Q1 instant is Saturday; nothing publishes Friday"
    );

    // —— 周六（休市日）：不调用任何 step ——
    let seq_before = scenario.session.seq();
    let (day_before, tick_before) = (scenario.session.day(), scenario.session.tick());
    let snapshot_before = scenario.session.snapshot();

    let (saturday_report, saturday_out) = scenario.settle_and_dispatch();
    assert_eq!(saturday_report.disclosure_instant.date(), q1_instant.date());

    // 零市场副作用：只有共享 civil 事件推进 seq，交易日计数、tick、市场快照不变。
    assert_eq!(
        scenario.session.seq(),
        seq_before + 1,
        "closed-day settlement emits exactly the shared civil-date event"
    );
    assert!(matches!(
        saturday_report.events.as_slice(),
        [engine::Event::CivilDateAdvanced { .. }]
    ));
    assert_eq!(
        (scenario.session.day(), scenario.session.tick()),
        (day_before, tick_before)
    );
    let mut snapshot_after = scenario.session.snapshot();
    snapshot_after.seq = snapshot_before.seq;
    assert_eq!(
        serde_json::to_string(&snapshot_after).expect("snapshot serializes"),
        serde_json::to_string(&snapshot_before).expect("snapshot serializes"),
        "the civil event changes only shared sequence metadata, not market state"
    );

    // 恰好一条 Q1 公布，时点 = 周六 18:00（civil 时钟权威相位瞬间）。
    assert_eq!(saturday_out.reports_published.len(), 1);
    let id = saturday_out.reports_published[0];
    let published = scenario
        .seeded
        .library
        .report(id, saturday_report.disclosure_instant)
        .expect("query at publication instant is legal");
    assert_eq!(published.company, company);
    assert_eq!(published.published_at, saturday_report.disclosure_instant);
    assert!(
        published.reports.window.0.year() >= q1_instant.date().year() - 2,
        "window within history"
    );
    assert!(matches!(
        &published.origin,
        PublicationOrigin::ScheduledDisclosure { fiscal_year, kind, offset_days }
            if *fiscal_year == year && *kind == ScheduledReportKind::Q1 && *offset_days == offset
    ));

    // 提前读取 = 类型化拒绝（周六 17:59 读不到）。
    let before = CivilInstant::from_hms(q1_instant.date(), 17, 59, 59).expect("17:59:59 valid");
    assert!(matches!(
        scenario.seeded.library.report(id, before).unwrap_err(),
        InformationError::EarlyRead { .. }
    ));
}

/// 临时公告金样：注入的单公司冲击在发生日的下一个 18:00（= 当日 18:00
/// 披露相位）公布，内容只含已确认事件条款；发生日之前不可读；同日重复
/// 派发不追加（恰好一次）。
#[test]
fn interim_announcement_publishes_at_next_disclosure_phase() {
    let company = CompanyId(INDUSTRIAL_ID.to_string());
    let offset = stable_company_offset(OPS_SEED, &company);
    let mut scenario = WeekendScenario::new(offset);
    let year = scenario.year;
    let saturday = scheduled_instant(ScheduledReportKind::Q1, year, offset)
        .expect("instant")
        .date();
    scenario.install_dispatch();

    scenario.step_friday();
    let (friday_report, _) = scenario.settle_and_dispatch();

    // 注入公司级冲击（apply_company_shock 的预期用法：starts_on = 下一经营日）。
    scenario
        .seeded
        .ops
        .apply_company_shock(
            &company,
            ActiveShock {
                kind: ShockKind::ContractWon,
                amplitude_bp: 1_500,
                starts_on: saturday,
                expires_on: plus_days(saturday, 5).expect("window within civil range"),
            },
        )
        .expect("company shock injection");

    let (saturday_report, out) = scenario.settle_and_dispatch();

    assert_eq!(
        out.announcements_published.len(),
        1,
        "exactly one announcement"
    );
    let announcement = scenario
        .seeded
        .library
        .announcement(
            out.announcements_published[0],
            saturday_report.disclosure_instant,
        )
        .expect("announcement readable at its phase instant");
    assert_eq!(announcement.company, company);
    assert_eq!(announcement.occurred_on, saturday);
    assert_eq!(
        announcement.published_at,
        saturday_report.disclosure_instant
    );
    assert_eq!(announcement.event.kind, ShockKind::ContractWon);
    assert_eq!(announcement.event.amplitude_bp, 1_500);
    assert_eq!(announcement.event.starts_on, saturday);
    assert_eq!(
        announcement.event.expires_on,
        plus_days(saturday, 5).unwrap()
    );
    // 未来现金流是预测：公告类型上只有事件条款，无任何「已实现」金额声明。
    let friday_night =
        CivilInstant::from_hms(friday_report.settled_date, 23, 59, 59).expect("23:59:59 valid");
    assert!(matches!(
        scenario
            .seeded
            .library
            .announcement(out.announcements_published[0], friday_night)
            .unwrap_err(),
        InformationError::EarlyRead { .. }
    ));
    // 恰好一次：同一日重复派发（宿主失误路径）不再追加公告/报告。
    let again = scenario
        .dispatch
        .run_day_end(DayEndDisclosureCtx {
            report: &saturday_report,
            ops: &scenario.seeded.ops,
            closing: &mut scenario.seeded.closing,
            library: &mut scenario.seeded.library,
        })
        .expect("replay dispatch is a no-op");
    assert!(again.announcements_published.is_empty() && again.reports_published.is_empty());
}
