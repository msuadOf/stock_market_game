//! 更正跨期语义回归：CAS 28 追溯重述。
//!
//! 更正调整分录的损益必须归入**目标历史期间**（重述版本 + 后续期间的
//! 期初留存收益/比较项），绝不进入后续期间的当期利润表；现金仍按实际
//! 收付期间列报（恰一次）。重述底稿（Scope → 来源 → 目标期间）随
//! ClosingEngine 持久化，经 serde 存档恢复后仍生效。
//!
//! 手算锚（元）：原年报（ReportVersion.sequence=1）净利 5,295 / 现金 95,295；重述年报（ReportVersion.sequence=2）净利 5,595 /
//! 现金 95,595；2031-01 月报净利 0 / 期初留存 105,595 / 经营 CF +300；
//! FY2031 年报净利 0、上年同期（重述）5,595。

use crate::fixture::{entry, yuan};
use engine::accounting::closing::{ClosingEngine, ClosingError, CorrectionRequest, ReportHandle};
use engine::accounting::consolidation::{MemberId, ScopeId};
use engine::accounting::reports::{Comparative, IndustryPresentation, ReportKind};
use engine::accounting::{AccountingPeriod, Books, BusinessKind, CashFlowClass, PostingSide};

fn period(iso: &str) -> AccountingPeriod {
    AccountingPeriod::from_iso(iso).expect("period")
}

fn member() -> MemberId {
    MemberId("C-LIFE".to_string())
}

/// 推进到「FY2030 已年结 + 2031-01 更正年报（遗漏收入 +300）」的状态，
/// 并返回更正前的原年报 serde 字节（供事后逐字节比对）。
struct CorrectionScenario {
    books: Books,
    closing: ClosingEngine,
    member: MemberId,
    original_report_json: String,
}

impl CorrectionScenario {
    fn through_correction() -> Self {
        let mut books = Books::new(engine::company::industrial::industrial_account_chart());
        let mut closing = ClosingEngine::new();
        let industry = IndustryPresentation::Industrial;
        let id = member();
        let scope = ScopeId::Standalone(id.clone());
        books
            .post_batch(vec![
                entry(
                    1,
                    "2029-12-31",
                    BusinessKind::OpeningBalance,
                    CashFlowClass::Financing,
                    &[
                        ("1002", PostingSide::Debit, 90_000),
                        ("1601", PostingSide::Debit, 10_000),
                        ("4001", PostingSide::Credit, 100_000),
                    ],
                ),
                entry(
                    2,
                    "2030-01-15",
                    BusinessKind::CashRevenue,
                    CashFlowClass::Operating,
                    &[
                        ("1002", PostingSide::Debit, 5_295),
                        ("6001", PostingSide::Credit, 5_295),
                    ],
                ),
            ])
            .expect("2030 postings");
        for m in 1..=11u8 {
            closing
                .close_month(
                    &mut books,
                    &id,
                    industry,
                    AccountingPeriod::from_ymd(2030, m).expect("month"),
                )
                .expect("2030 month close");
        }
        closing
            .close_year(&mut books, &id, industry, 2030)
            .expect("2030 year close");
        let original_report_json = serde_json::to_string(
            closing
                .version(&scope, period("2030-12"), ReportKind::Annual, 1)
                .expect("原年报已登记"),
        )
        .expect("原年报可序列化");
        closing
            .correct(
                &mut books,
                &id,
                industry,
                (period("2030-12"), ReportKind::Annual),
                CorrectionRequest {
                    entries: vec![entry(
                        3,
                        "2031-01-15",
                        BusinessKind::CashRevenue,
                        CashFlowClass::Operating,
                        &[
                            ("1002", PostingSide::Debit, 300),
                            ("6001", PostingSide::Credit, 300),
                        ],
                    )],
                    reason: "遗漏现金收入更正".to_string(),
                },
            )
            .expect("correction");
        Self {
            books,
            closing,
            member: id,
            original_report_json,
        }
    }

    fn close_month(&mut self, period: AccountingPeriod) -> Result<ReportHandle, ClosingError> {
        self.closing.close_month(
            &mut self.books,
            &self.member,
            IndustryPresentation::Industrial,
            period,
        )
    }

    fn close_year(&mut self, year: i32) -> Result<(ReportHandle, ReportHandle), ClosingError> {
        self.closing.close_year(
            &mut self.books,
            &self.member,
            IndustryPresentation::Industrial,
            year,
        )
    }
}

#[test]
fn correction_does_not_leak_into_later_periods() {
    let mut scenario = CorrectionScenario::through_correction();
    let scope = ScopeId::Standalone(scenario.member.clone());

    // (a) 重述版本：更正归入目标历史期间的损益与权益运动。
    let corrected_report = scenario
        .closing
        .version(&scope, period("2030-12"), ReportKind::Annual, 2)
        .expect("重述年报已登记")
        .clone();
    assert_eq!(corrected_report.income.cumulative.net_income, yuan(5_595));
    assert_eq!(corrected_report.equity.net_income, yuan(5_595));
    assert_eq!(corrected_report.equity.opening_parent, yuan(100_000));
    assert_eq!(corrected_report.equity.closing_parent, yuan(105_595));
    assert_eq!(
        corrected_report.cash_flow.operating,
        yuan(5_295),
        "restated CF keeps actual periods"
    );

    // (e) 原公布年报逐字节不变。
    let original_report = scenario
        .closing
        .version(&scope, period("2030-12"), ReportKind::Annual, 1)
        .expect("原年报仍可读取");
    assert_eq!(original_report.income.cumulative.net_income, yuan(5_295));
    assert_eq!(
        serde_json::to_string(original_report).unwrap(),
        scenario.original_report_json
    );

    // (b) 2031-01 月报：更正对后续期间损益贡献为零；期初留存吸收更正；
    //     现金按实际期间恰一次。
    let jan31 = scenario
        .close_month(period("2031-01"))
        .expect("2031-01 close");
    let jan31_set = scenario
        .closing
        .version(
            &scope,
            period("2031-01"),
            ReportKind::Monthly,
            jan31.sequence,
        )
        .expect("2031-01 stored");
    assert_eq!(
        jan31_set.income.cumulative.net_income,
        yuan(0),
        "correction must contribute zero to later-period P&L (CAS 28 restatement)"
    );
    assert_eq!(jan31_set.income.quarter.net_income, yuan(0));
    assert_eq!(jan31_set.equity.net_income, yuan(0));
    assert_eq!(
        jan31_set.equity.opening_parent,
        yuan(105_595),
        "correction adjusts opening retained earnings of later periods"
    );
    assert_eq!(
        jan31_set.cash_flow.operating,
        yuan(300),
        "cash belongs to its actual period, exactly once"
    );
    assert_eq!(jan31_set.cash_flow.closing_cash, yuan(95_595));

    // (c)/(d) FY2031 年报：累计净利 0；上年同期 = 重述后 5,595（非原报）。
    for m in 2..=11u8 {
        scenario
            .close_month(AccountingPeriod::from_ymd(2031, m).expect("month"))
            .expect("2031 month close");
    }
    let (_, annual31) = scenario.close_year(2031).expect("2031 year close");
    let fy31 = scenario
        .closing
        .version(
            &scope,
            period("2031-12"),
            ReportKind::Annual,
            annual31.sequence,
        )
        .expect("FY2031 stored")
        .clone();
    assert_eq!(fy31.income.cumulative.net_income, yuan(0));
    assert_eq!(fy31.equity.net_income, yuan(0));
    assert_eq!(fy31.equity.opening_parent, yuan(105_595));
    match &fy31.income.prior_year {
        Comparative::Available(cols) => assert_eq!(
            cols.net_income,
            yuan(5_595),
            "prior-year comparative must carry the restated figure"
        ),
        Comparative::Unavailable { .. } => panic!("prior-year comparative must be available"),
    }
    assert!(matches!(
        fy31.balance_sheet.prior_year_end,
        Comparative::Available(_)
    ));
    assert_eq!(fy31.cash_flow.operating, yuan(300));
}

#[test]
fn failed_correction_report_generation_restores_books_and_closing_state() {
    let mut scenario = CorrectionScenario::through_correction();
    let books_before = serde_json::to_vec(&scenario.books).expect("账套可序列化");
    let closing_before = serde_json::to_vec(&scenario.closing).expect("结账状态可序列化");
    let result = scenario.closing.correct(
        &mut scenario.books,
        &scenario.member,
        IndustryPresentation::Bank,
        (period("2030-12"), ReportKind::Annual),
        CorrectionRequest {
            entries: vec![entry(
                4,
                "2031-02-15",
                BusinessKind::CashRevenue,
                CashFlowClass::Operating,
                &[
                    ("1002", PostingSide::Debit, 100),
                    ("6001", PostingSide::Credit, 100),
                ],
            )],
            reason: "错配行业报表生成失败时保留此原因".to_string(),
        },
    );

    assert!(result.is_err(), "行业科目错配必须使派生报告生成失败");
    assert_eq!(
        serde_json::to_vec(&scenario.books).expect("账套可序列化"),
        books_before,
        "派生报告失败不得留下更正分录"
    );
    assert_eq!(
        serde_json::to_vec(&scenario.closing).expect("结账状态可序列化"),
        closing_before,
        "派生报告失败不得留下重述底稿或版本"
    );
}

#[test]
fn report_arithmetic_overflow_restores_correction_transaction() {
    use engine::accounting::{
        AccountingAmount, BusinessEventId, JournalEntry, JournalLine, LedgerAccountId,
    };
    use engine::calendar::CivilDate;

    let mut books = Books::new(engine::company::industrial::industrial_account_chart());
    books
        .post_batch(vec![JournalEntry {
            source: BusinessEventId::new(1),
            date: CivilDate::from_iso("2029-12-31").expect("fixture date"),
            kind: BusinessKind::OpeningBalance,
            cash_flow: CashFlowClass::Financing,
            lines: vec![
                JournalLine {
                    account: LedgerAccountId("1002".to_string()),
                    side: PostingSide::Debit,
                    amount: AccountingAmount::from_cents(i128::MAX),
                },
                JournalLine {
                    account: LedgerAccountId("4001".to_string()),
                    side: PostingSide::Credit,
                    amount: AccountingAmount::from_cents(i128::MAX),
                },
            ],
        }])
        .expect("opening balance posts at the largest representable amount");
    let mut closing = ClosingEngine::new();
    let id = member();
    for month in 1..=11 {
        closing
            .close_month(
                &mut books,
                &id,
                IndustryPresentation::Industrial,
                AccountingPeriod::from_ymd(2030, month).expect("fixture period"),
            )
            .expect("short history month closes");
    }
    closing
        .close_year(&mut books, &id, IndustryPresentation::Industrial, 2030)
        .expect("target annual report closes before correction");
    let books_before = serde_json::to_vec(&books).expect("账套可序列化");
    let closing_before = serde_json::to_vec(&closing).expect("结账状态可序列化");

    let result = closing.correct(
        &mut books,
        &id,
        IndustryPresentation::Industrial,
        (period("2030-12"), ReportKind::Annual),
        CorrectionRequest {
            entries: vec![entry(
                2,
                "2031-01-15",
                BusinessKind::CreditSale,
                CashFlowClass::NonCash,
                &[("1122", PostingSide::Debit, 1), ("6001", PostingSide::Credit, 1)],
            )],
            reason: "派生报表合计溢出更正".to_string(),
        },
    );

    assert!(matches!(
        &result,
        Err(ClosingError::CorrectionFailed { reason, cause })
            if reason == "派生报表合计溢出更正"
                && matches!(
                    cause.as_ref(),
                    ClosingError::Report(report)
                        if matches!(
                            report.as_ref(),
                            engine::accounting::reports::ReportError::Accounting(accounting)
                                if matches!(
                                    accounting.as_ref(),
                                    engine::accounting::AccountingError::AmountOverflow { op: "add", .. }
                                )
                        )
                )
    ), "派生报表的加法溢出必须拒绝更正");
    assert_eq!(serde_json::to_vec(&books).expect("账套可序列化"), books_before);
    assert_eq!(
        serde_json::to_vec(&closing).expect("结账状态可序列化"),
        closing_before
    );
}

/// 重述底稿随引擎 serde 状态整体存取：save → restore → 生成的后续版本
/// 与不落盘路径逐字节一致（底稿经存档恢复后仍生效）。
#[test]
fn restatement_worksheet_survives_serde_round_trip() {
    let CorrectionScenario {
        books,
        closing,
        member: id,
        ..
    } = CorrectionScenario::through_correction();
    let industry = IndustryPresentation::Industrial;
    let scope = ScopeId::Standalone(id.clone());
    let saved_engine = serde_json::to_string(&closing).expect("engine serializes");
    let saved_books = serde_json::to_string(&books).expect("books serialize");

    let mut live = closing;
    let mut live_books: Books = serde_json::from_str(&saved_books).expect("books restore");
    let h_live = live
        .close_month(&mut live_books, &id, industry, period("2031-01"))
        .expect("live close");

    let mut restored: ClosingEngine = serde_json::from_str(&saved_engine).expect("engine restores");
    let mut restored_books: Books = serde_json::from_str(&saved_books).expect("books restore");
    let h_restored = restored
        .close_month(&mut restored_books, &id, industry, period("2031-01"))
        .expect("restored close");

    assert_eq!(h_live, h_restored);
    let live_set = live
        .version(
            &scope,
            period("2031-01"),
            ReportKind::Monthly,
            h_live.sequence,
        )
        .expect("live stored");
    let restored_set = restored
        .version(
            &scope,
            period("2031-01"),
            ReportKind::Monthly,
            h_restored.sequence,
        )
        .expect("restored stored");
    assert_eq!(
        serde_json::to_string(live_set).unwrap(),
        serde_json::to_string(restored_set).unwrap()
    );
    assert_eq!(
        restored_set.income.cumulative.net_income,
        yuan(0),
        "worksheet must stay in force after save/restore"
    );
}
