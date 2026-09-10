//! 生命周期金样：逐日入账→逐月封月（含季报/半年报快照不阻断后续入账）→
//! 年结→次年更正年报；原公开版本逐字节不变；现金不双计。
//!
//! 更正语义：调整分录过账于当前开放期间（2031-01），经重述映射作用于
//! 2030-12 的资产负债表/利润表窗口；现金流量表保持实际收付期间口径
//! （间接法以显式「重述现金调整」行配平，不做 plug）。
//!
//! 手算锚（元）：v1 年报净利 5,295 / 现金 99,975；v2 重述净利 5,595 /
//! 现金 100,275；v2 经营 CF 5,000（实际期间）；2031-01 月报经营 CF +300。

use crate::fixture::{entry, standalone};
use engine::accounting::closing::{ClosingEngine, CorrectionRequest};
use engine::accounting::consolidation::{MemberId, ScopeId};
use engine::accounting::AccountingPeriod;
use engine::accounting::reports::{
    generate_report_set, BsLine, Comparative, IndustryPresentation, ReportKind, ReportRequest, ReportVersion, VersionKind,
};
use engine::accounting::{AccountingAmount, Books, BusinessKind, CashFlowClass, PostingSide};

fn yuan(v: i128) -> AccountingAmount {
    AccountingAmount::from_cents(v * 100)
}

fn period(iso: &str) -> AccountingPeriod {
    AccountingPeriod::from_iso(iso).expect("period")
}

fn member() -> MemberId {
    MemberId("C-LIFE".to_string())
}

fn original_request<'a>(iso: &str, kind: ReportKind, books: &'a Books) -> ReportRequest<'a> {
    ReportRequest {
        period: period(iso),
        kind,
        source: standalone("C-LIFE", books, IndustryPresentation::Industrial),
        version: ReportVersion {
            sequence: 1,
            supersedes: None,
            kind: VersionKind::Original,
        },
        adjustments: &crate::fixture::NO_ADJUSTMENTS,
    }
}

#[test]
fn monthly_finalize_year_close_and_correction_lifecycle() {
    let mut books = Books::new(engine::company::industrial::industrial_chart_v2());
    let mut closing = ClosingEngine::new();
    let industry = IndustryPresentation::Industrial;
    let id = member();
    let scope = ScopeId::Standalone(id.clone());

    // 开局（2029-12；NonCash 平衡凭证）。
    books
        .post_batch(vec![entry(1, "2029-12-31", BusinessKind::OpeningBalance, CashFlowClass::Financing, &[
            ("1002", PostingSide::Debit, 90_000),
            ("1601", PostingSide::Debit, 10_000),
            ("4001", PostingSide::Credit, 100_000),
        ])])
        .expect("opening posts");

    // —— 1 月：逐日入账两笔，月末定稿（每日定稿的月度收敛）——
    books
        .post_batch(vec![entry(2, "2030-01-15", BusinessKind::CashRevenue, CashFlowClass::Operating, &[
            ("1002", PostingSide::Debit, 2_000), ("6001", PostingSide::Credit, 2_000),
        ])])
        .expect("january day posting");
    books
        .post_batch(vec![entry(21, "2030-01-28", BusinessKind::CashExpense, CashFlowClass::Operating, &[
            ("6602", PostingSide::Debit, 300), ("1002", PostingSide::Credit, 300),
        ])])
        .expect("january day posting 2");
    let jan = closing
        .close_month(&mut books, &id, industry, period("2030-01"))
        .expect("january close");
    assert_eq!(jan.sequence, 1);

    // —— 2–3 月：同年后续月入账并定稿 ——
    for (src, date, kind, class, lines) in [
        (3u64, "2030-02-10", BusinessKind::CreditSale, CashFlowClass::NonCash,
         vec![("1122", PostingSide::Debit, 1_500i128), ("6001", PostingSide::Credit, 1_500)]),
        (4, "2030-02-20", BusinessKind::ReceivableCollection, CashFlowClass::Operating,
         vec![("1002", PostingSide::Debit, 1_000), ("1122", PostingSide::Credit, 1_000)]),
        (5, "2030-03-15", BusinessKind::Depreciation, CashFlowClass::NonCash,
         vec![("6602", PostingSide::Debit, 120), ("1602", PostingSide::Credit, 120)]),
    ] {
        books
            .post_batch(vec![entry(src, date, kind, class, &lines)])
            .expect("posting");
    }
    closing.close_month(&mut books, &id, industry, period("2030-02")).expect("feb close");
    closing.close_month(&mut books, &id, industry, period("2030-03")).expect("mar close");

    // —— Q1 季报快照：不封账、不阻断 ——
    let q1 = closing
        .snapshot_interim(&books, &id, industry, period("2030-03"), ReportKind::Quarter)
        .expect("q1 snapshot");
    let q1_snapshot = closing
        .version(&scope, period("2030-03"), ReportKind::Quarter, q1.sequence)
        .expect("q1 stored")
        .clone();

    // —— 4–11 月：快照后继续自由入账（含空月照常封月）——
    for (src, date, kind, class, lines) in [
        (6u64, "2030-04-10", BusinessKind::CashExpense, CashFlowClass::Operating,
         vec![("6401", PostingSide::Debit, 700i128), ("1002", PostingSide::Credit, 700)]),
        (7, "2030-05-10", BusinessKind::LoanDisbursement, CashFlowClass::Financing,
         vec![("1002", PostingSide::Debit, 5_000), ("2001", PostingSide::Credit, 5_000)]),
        (8, "2030-06-10", BusinessKind::InterestAccrual, CashFlowClass::NonCash,
         vec![("6603", PostingSide::Debit, 25), ("2231", PostingSide::Credit, 25)]),
        (9, "2030-06-20", BusinessKind::TaxAccrual, CashFlowClass::NonCash,
         vec![("6801", PostingSide::Debit, 60), ("222104", PostingSide::Credit, 60)]),
        (10, "2030-09-10", BusinessKind::CashRevenue, CashFlowClass::Operating,
         vec![("1002", PostingSide::Debit, 3_000), ("6001", PostingSide::Credit, 3_000)]),
        (11, "2030-11-10", BusinessKind::InterestPayment, CashFlowClass::Financing,
         vec![("2231", PostingSide::Debit, 25), ("1002", PostingSide::Credit, 25)]),
    ] {
        books
            .post_batch(vec![entry(src, date, kind, class, &lines)])
            .expect("later-month posting");
    }
    for m in 4..=11u8 {
        closing
            .close_month(&mut books, &id, industry, AccountingPeriod::from_ymd(2030, m).expect("month"))
            .expect("month close");
    }
    // 半年报快照（6 月窗口，此时 4–6 月均已定稿）。
    closing
        .snapshot_interim(&books, &id, industry, period("2030-06"), ReportKind::HalfYear)
        .expect("halfyear snapshot");

    // —— 年结：12 月封月 + 月报版本 + 年报版本 ——
    let (monthly_dec, annual) = closing.close_year(&mut books, &id, industry, 2030).expect("year close");
    assert_eq!((monthly_dec.kind, annual.kind), (ReportKind::Monthly, ReportKind::Annual));
    let annual_original = closing
        .version(&scope, period("2030-12"), ReportKind::Annual, annual.sequence)
        .expect("annual v1 stored")
        .clone();

    // —— 次年更正年报：调整分录（现金 300）过账于开放期间 2031-01 ——
    let corrected = closing
        .correct(
            &mut books,
            &id,
            industry,
            (period("2030-12"), ReportKind::Annual),
            CorrectionRequest {
                entries: vec![entry(12, "2031-01-15", BusinessKind::CashRevenue, CashFlowClass::Operating, &[
                    ("1002", PostingSide::Debit, 300), ("6001", PostingSide::Credit, 300),
                ])],
                reason: "遗漏现金收入更正".to_string(),
            },
        )
        .expect("correction");
    assert_eq!(corrected.sequence, 2);

    // —— 原公开版本逐字节不变 ——
    let annual_v1_after = closing
        .version(&scope, period("2030-12"), ReportKind::Annual, 1)
        .expect("annual v1 still stored");
    assert_eq!(&annual_original, annual_v1_after, "original published version must be byte-identical");
    assert_eq!(
        serde_json::to_string(&annual_original).unwrap(),
        serde_json::to_string(annual_v1_after).unwrap()
    );

    // —— 重述年报：利润表/资产负债表吸收更正；现金流量表保持实际期间 ——
    let v2 = closing
        .version(&scope, period("2030-12"), ReportKind::Annual, 2)
        .expect("annual v2 stored");
    assert_eq!(v2.version.supersedes, Some(1));
    assert!(matches!(v2.version.kind, VersionKind::Correction { .. }));
    assert_eq!(v2.income.cumulative.net_income, yuan(5_595));
    let cash_line = v2
        .balance_sheet
        .asset_lines
        .iter()
        .find(|(l, _)| *l == BsLine::CashFunds)
        .map(|(_, v)| *v)
        .expect("cash line");
    assert_eq!(cash_line, yuan(100_275));
    assert_eq!(v2.balance_sheet.total_assets, yuan(110_655));
    assert_eq!(v2.cash_flow.operating, yuan(5_000), "restated CF keeps actual periods");
    assert_eq!(v2.cash_flow.closing_cash, yuan(99_975), "CF closing on actual-period basis");
    // 间接法配平（含显式重述现金调整行）：5,595 + 4,680 − 4,975 − 300 = 5,000。
    let sum: i128 = v2.cash_flow.indirect.iter().map(|l| l.amount.cents()).sum();
    assert_eq!(sum, 5_000 * 100);

    // —— 现金不双计：总账现金恰等于实际运动合计（含更正一次）——
    assert_eq!(books.ledger().cash_total().expect("cash"), yuan(100_275));

    // —— 更正后的 2031-01 月报：经营 CF 恰好包含更正现金一次；上年同期可得 ——
    let fy31 = generate_report_set(original_request("2031-01", ReportKind::Monthly, &books))
        .expect("2031-01 report");
    assert_eq!(fy31.cash_flow.operating, yuan(300));
    assert!(matches!(fy31.income.prior_year, Comparative::Available(_)));

    // —— 期间版本可由窗口重建：1 月月报/Q1 快照 与推进后的账套逐字节一致 ——
    let jan_rebuilt = generate_report_set(original_request("2030-01", ReportKind::Monthly, &books))
        .expect("jan rebuild");
    let jan_stored = closing
        .version(&scope, period("2030-01"), ReportKind::Monthly, jan.sequence)
        .expect("january stored")
        .clone();
    assert_eq!(jan_stored, jan_rebuilt);
    let q1_rebuilt = generate_report_set(original_request("2030-03", ReportKind::Quarter, &books))
        .expect("q1 rebuild");
    assert_eq!(q1_snapshot, q1_rebuilt, "quarter snapshot must regenerate byte-equal from later journal");
}
