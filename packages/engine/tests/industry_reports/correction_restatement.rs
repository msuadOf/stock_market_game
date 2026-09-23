//! 更正跨期语义回归（任务 13 复核 F1）：CAS 28 追溯重述。
//!
//! 更正调整分录的损益必须归入**目标历史期间**（重述版本 + 后续期间的
//! 期初留存收益/比较项），绝不进入后续期间的当期利润表；现金仍按实际
//! 收付期间列报（恰一次）。重述底稿（Scope → 来源 → 目标期间）随
//! ClosingEngine 持久化，经 serde 存档恢复后仍生效。
//!
//! 手算锚（元）：v1 年报净利 5,295 / 现金 95,295；v2 重述净利 5,595 /
//! 现金 95,595；2031-01 月报净利 0 / 期初留存 105,595 / 经营 CF +300；
//! FY2031 年报净利 0、上年同期（重述）5,595。

use crate::fixture::{entry, yuan};
use engine::accounting::closing::{ClosingEngine, CorrectionRequest};
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
/// 并返回更正前的 v1 年报 serde 字节（供事后逐字节比对）。
fn books_through_correction() -> (Books, ClosingEngine, MemberId, String) {
    let mut books = Books::new(engine::company::industrial::industrial_chart_v2());
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
    let v1_json = serde_json::to_string(
        closing
            .version(&scope, period("2030-12"), ReportKind::Annual, 1)
            .expect("annual v1 stored"),
    )
    .expect("v1 serializes");
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
    (books, closing, id, v1_json)
}

#[test]
fn correction_does_not_leak_into_later_periods() {
    let (mut books, mut closing, id, v1_json) = books_through_correction();
    let industry = IndustryPresentation::Industrial;
    let scope = ScopeId::Standalone(id.clone());

    // (a) 重述版本：更正归入目标历史期间的损益与权益运动。
    let v2 = closing
        .version(&scope, period("2030-12"), ReportKind::Annual, 2)
        .expect("restated annual v2")
        .clone();
    assert_eq!(v2.income.cumulative.net_income, yuan(5_595));
    assert_eq!(v2.equity.net_income, yuan(5_595));
    assert_eq!(v2.equity.opening_parent, yuan(100_000));
    assert_eq!(v2.equity.closing_parent, yuan(105_595));
    assert_eq!(
        v2.cash_flow.operating,
        yuan(5_295),
        "restated CF keeps actual periods"
    );

    // (e) 原公布 v1 逐字节不变。
    let v1 = closing
        .version(&scope, period("2030-12"), ReportKind::Annual, 1)
        .expect("original annual v1");
    assert_eq!(v1.income.cumulative.net_income, yuan(5_295));
    assert_eq!(serde_json::to_string(v1).unwrap(), v1_json);

    // (b) 2031-01 月报：更正对后续期间损益贡献为零；期初留存吸收更正；
    //     现金按实际期间恰一次。
    let jan31 = closing
        .close_month(&mut books, &id, industry, period("2031-01"))
        .expect("2031-01 close");
    let jan31_set = closing
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
        closing
            .close_month(
                &mut books,
                &id,
                industry,
                AccountingPeriod::from_ymd(2031, m).expect("month"),
            )
            .expect("2031 month close");
    }
    let (_, annual31) = closing
        .close_year(&mut books, &id, industry, 2031)
        .expect("2031 year close");
    let fy31 = closing
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

/// 重述底稿随引擎 serde 状态整体存取：save → restore → 生成的后续版本
/// 与不落盘路径逐字节一致（底稿经存档恢复后仍生效）。
#[test]
fn restatement_worksheet_survives_serde_round_trip() {
    let (books, closing, id, _v1_json) = books_through_correction();
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
