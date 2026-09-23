//! 前史金样（K4 / 验收句）：2000-01-01 最早开局下，1998 起财务前史 +
//! 360 个交易日预置 K 均合法且可恢复；开局未来才公布的报告不提前纳入；
//! 同 seed 排期与 PublicationId 完全一致。

use crate::fixture::{history_start_of, two_company_config, BANK_ID, INDUSTRIAL_ID};
use engine::accounting::reports::ReportKind;
use engine::accounting::AccountingPeriod;
use engine::calendar::{CalendarExchange, CivilDate, CivilInstant, TradingCalendar};
use engine::company::CompanyId;
use engine::information::{
    assemble_seeded_prehistory, scheduled_instant, stable_company_offset, PublicationOrigin,
    ScheduledReportKind,
};

const GAME_START: &str = "2000-01-01";

fn q1(year: i32) -> AccountingPeriod {
    AccountingPeriod::from_ymd(year, 3).expect("q1 period")
}

fn annual(year: i32) -> AccountingPeriod {
    AccountingPeriod::from_ymd(year, 12).expect("annual period")
}

#[test]
fn earliest_start_prehistory_is_legal_and_restorable() {
    let start = CivilDate::from_iso(GAME_START).expect("earliest runtime start");
    let as_of = history_start_of(start)
        .prev()
        .expect("as_of before history");

    // 前史首日 = 1998-01-01：不早于初始化专用下界（1998 起财务前史合法）。
    assert_eq!(history_start_of(start).to_iso(), "1998-01-01");

    // 360 个交易日预置 K： earliest 开局下日历前史查询不耗尽。
    TradingCalendar::default_v1()
        .expect("default calendar")
        .trading_days_before(CalendarExchange::Sse, start, 360)
        .expect("360 trading days of preset candles are legal at the earliest start");

    let seeded = assemble_seeded_prehistory(two_company_config(11, as_of), start)
        .expect("prehistory assembles at the earliest runtime start");

    // 已公开集合：工商 + 银行各 7 期（1998 Q1/H1/Q3/年报 + 1999 Q1/H1/Q3）。
    assert_eq!(
        seeded.library.report_count(),
        14,
        "7 scheduled publications per company"
    );
    assert_eq!(
        seeded.library.announcement_count(),
        0,
        "no interim announcements in prehistory"
    );

    let start_instant = CivilInstant::new(start, 0).expect("start-of-day instant");
    for id in seeded.library.publication_ids() {
        let report = seeded
            .library
            .report(id, start_instant)
            .expect("every seeded report is readable at game start");
        assert!(
            report.published_at < start_instant,
            "seeded set must only contain pre-start publications"
        );
        assert!(matches!(
            report.origin,
            PublicationOrigin::SeededPrehistory { .. }
        ));
    }

    // 开局未来才公布的报告不得提前纳入：1999 年报（2000-03 排期）、2000 Q1。
    for company_id in [INDUSTRIAL_ID, BANK_ID] {
        let company = CompanyId(company_id.to_string());
        let offset = stable_company_offset(11, &company);
        let annual_1999 = scheduled_instant(ScheduledReportKind::Annual, 1999, offset)
            .expect("annual 1999 schedule");
        assert!(
            annual_1999 > start_instant,
            "fixture sanity: annual 1999 publishes after start"
        );
        assert!(
            seeded
                .library
                .latest_report(&company, ReportKind::Annual, annual(1999), start_instant)
                .is_none(),
            "annual 1999 must not be pre-included (publishes {annual_1999:?})"
        );
        assert!(
            seeded
                .library
                .latest_report(&company, ReportKind::Quarter, q1(2000), start_instant)
                .is_none(),
            "Q1 2000 must not be pre-included"
        );
        // 1998/1999 的已排期各期全部在场。
        for (year, kind, report_kind, period) in [
            (1998, ScheduledReportKind::Q1, ReportKind::Quarter, q1(1998)),
            (
                1998,
                ScheduledReportKind::HalfYear,
                ReportKind::HalfYear,
                AccountingPeriod::from_ymd(1998, 6).expect("h1"),
            ),
            (
                1998,
                ScheduledReportKind::Q3,
                ReportKind::Quarter,
                AccountingPeriod::from_ymd(1998, 9).expect("q3"),
            ),
            (
                1998,
                ScheduledReportKind::Annual,
                ReportKind::Annual,
                annual(1998),
            ),
            (1999, ScheduledReportKind::Q1, ReportKind::Quarter, q1(1999)),
            (
                1999,
                ScheduledReportKind::HalfYear,
                ReportKind::HalfYear,
                AccountingPeriod::from_ymd(1999, 6).expect("h1"),
            ),
            (
                1999,
                ScheduledReportKind::Q3,
                ReportKind::Quarter,
                AccountingPeriod::from_ymd(1999, 9).expect("q3"),
            ),
        ] {
            let published = seeded
                .library
                .latest_report(&company, report_kind, period, start_instant)
                .unwrap_or_else(|| panic!("{company_id:?} {kind:?} {year} must be seeded"));
            assert_eq!(published.company, company);
            assert_eq!(
                published.published_at,
                scheduled_instant(kind, year, offset).unwrap()
            );
            assert_eq!(published.reports.period, period);
            // 比较项诚实性（任务 13 契约）：前史开局凭证落在 1997-12-31，
            // 故 1998 年报（上年窗口 = 全年 1997）比较项 Available（年初余额）；
            // 1998 中期报告的上年同季窗口早于开局凭证 ⇒ 合法 Unavailable。
            if year == 1998 {
                match kind {
                    ScheduledReportKind::Annual => assert!(matches!(
                        published.reports.income.prior_year,
                        engine::accounting::reports::Comparative::Available(_)
                    )),
                    _ => assert!(matches!(
                        published.reports.income.prior_year,
                        engine::accounting::reports::Comparative::Unavailable { .. }
                    )),
                }
            }
        }
    }
}

#[test]
fn same_seed_prehistory_ids_and_bytes_identical() {
    let start = CivilDate::from_iso(GAME_START).expect("earliest runtime start");
    let as_of = history_start_of(start)
        .prev()
        .expect("as_of before history");

    let first =
        assemble_seeded_prehistory(two_company_config(11, as_of), start).expect("first assembly");
    let second =
        assemble_seeded_prehistory(two_company_config(11, as_of), start).expect("second assembly");

    // 同 seed：PublicationId 序列 + 全库 serde 字节逐位一致。
    assert_eq!(
        first.library.publication_ids(),
        second.library.publication_ids(),
        "same seed must produce identical publication id sequence"
    );
    assert_eq!(
        serde_json::to_string(&first.library).expect("library serializes"),
        serde_json::to_string(&second.library).expect("library serializes"),
        "same seed must produce byte-identical libraries"
    );
    // 排期一致（时间表逐期对照）。
    for id in first.library.publication_ids() {
        let at = first
            .library
            .report(id, CivilInstant::new(start, 0).unwrap())
            .expect("readable");
        let other = second
            .library
            .report(id, CivilInstant::new(start, 0).unwrap())
            .expect("readable");
        assert_eq!(at.published_at, other.published_at);
        assert_eq!(at.company, other.company);
    }
}

#[test]
fn seeded_library_round_trips_through_serde() {
    let start = CivilDate::from_iso(GAME_START).expect("earliest runtime start");
    let as_of = history_start_of(start)
        .prev()
        .expect("as_of before history");
    let seeded =
        assemble_seeded_prehistory(two_company_config(11, as_of), start).expect("assembly");

    // serde 往返：恢复边界全量校验后逐字节等价（可恢复性验收句）。
    let bytes = serde_json::to_string(&seeded.library).expect("library serializes");
    let restored: engine::information::PublicLibrary =
        serde_json::from_str(&bytes).expect("library restores");
    assert_eq!(
        serde_json::to_string(&restored).expect("restored serializes"),
        bytes,
        "round-trip must be byte-stable"
    );
    // 恢复后的库仍可按 id/company/period 查询（含提前读取守卫）。
    let start_instant = CivilInstant::new(start, 0).expect("start instant");
    for id in seeded.library.publication_ids() {
        let original = seeded.library.report(id, start_instant).expect("readable");
        let back = restored
            .report(id, start_instant)
            .expect("restored readable");
        assert_eq!(
            serde_json::to_string(original).unwrap(),
            serde_json::to_string(back).unwrap()
        );
    }
}
