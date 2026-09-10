//! 拒绝路径：科目无归属 / 重复分类 / 附注勾稽不符 / 试算不平 / 捏造比较项 /
//! 非法报告种类 / 更正目标未封账 / 调整分录落入已封期间。
//!
//! 所有拒绝都必须是类型化错误（不静默、不填零、不 clamp）。

use crate::fixture::{entry, industrial_fixture, standalone};
use engine::accounting::closing::{verify_comparative_honesty, ClosingEngine, CorrectionRequest};
use engine::accounting::consolidation::MemberId;
use engine::accounting::AccountingPeriod;
use engine::accounting::reports::notes::{merge_assignments, Assignment, NoteTarget};
use engine::accounting::reports::{
    generate_report_set, validate_trial_balance, Comparative, IncomeColumns, IncomeLine,
    IndustryPresentation, ReportError, ReportKind, ReportRequest, ReportSource, ReportVersion, VersionKind,
};
use engine::accounting::{
    AccountChart, AccountingAmount, Books, BusinessKind, CashFlowClass,
    PostingSide, TrialBalanceSummary,
};

fn yuan(v: i128) -> AccountingAmount {
    AccountingAmount::from_cents(v * 100)
}

fn original_request<'a>(source: ReportSource<'a>, kind: ReportKind, iso: &str) -> ReportRequest<'a> {
    ReportRequest {
        period: AccountingPeriod::from_iso(iso).expect("period"),
        kind,
        source,
        version: ReportVersion {
            sequence: 1,
            supersedes: None,
            kind: VersionKind::Original,
        },
        adjustments: &crate::fixture::NO_ADJUSTMENTS,
    }
}

/// 科目无归属：通用 v1 科目表 + 银行列报（1001/1601 等无银行分类）→ 类型化拒绝。
#[test]
fn unclassified_account_rejects_publication() {
    let books = Books::new(AccountChart::generic_v1());
    let err = generate_report_set(original_request(
        ReportSource::Standalone {
            id: MemberId("C-X".to_string()),
            books: &books,
            industry: IndustryPresentation::Bank,
        },
        ReportKind::Monthly,
        "2030-01",
    ))
    .expect_err("generic chart under bank presentation must be rejected");
    assert!(matches!(err, ReportError::UnclassifiedAccount { ref code } if code.0 == "1001"));
}

/// 重复分类：同一科目映射到两条不同主表行 → 类型化拒绝。
#[test]
fn duplicate_classification_rejects() {
    let base = vec![Assignment {
        code: "6001",
        target: NoteTarget::Income(IncomeLine::OperatingRevenue),
    }];
    let conflicting = vec![Assignment {
        code: "6001",
        target: NoteTarget::Income(IncomeLine::OperatingCost),
    }];
    let err = merge_assignments(base, conflicting).expect_err("conflicting assignment must be rejected");
    assert!(matches!(err, ReportError::DuplicateClassification { ref code, .. } if code.0 == "6001"));
}

/// 附注明细合计与主表行不符 → 校验拒绝公布。
#[test]
fn notes_cross_foot_mismatch_rejects() {
    let books = industrial_fixture();
    let mut set = generate_report_set(original_request(
        standalone("C-IND", &books, IndustryPresentation::Industrial),
        ReportKind::Monthly,
        "2030-06",
    ))
    .expect("generation");
    set.validate().expect("pristine set must validate");
    // 捏造附注余额（+1 分）→ 勾稽破坏。
    set.notes.items[0].closing = set.notes.items[0].closing.add(yuan(1)).expect("bump");
    let err = set.validate().expect_err("mutated notes must fail cross-foot");
    assert!(matches!(err, ReportError::NotesCrossFootMismatch { .. }));
}

/// 试算不平 → 不允许公布（构造层面不可达，直接验证守卫本身）。
#[test]
fn unbalanced_trial_balance_rejects_publication() {
    let err = validate_trial_balance(&TrialBalanceSummary {
        total_debits: yuan(100),
        total_credits: yuan(90),
    })
    .expect_err("unbalanced trial must be rejected");
    assert!(matches!(err, ReportError::TrialBalanceUnbalanced { .. }));
    validate_trial_balance(&TrialBalanceSummary {
        total_debits: yuan(100),
        total_credits: yuan(100),
    })
    .expect("balanced trial passes");
}

/// 合法 Unavailable(reason) 可公布；缺历史却呈 Available（捏造 0）→ 拒绝。
#[test]
fn fabricated_comparative_rejects_but_unavailable_publishes() {
    let books = industrial_fixture();
    let set = generate_report_set(original_request(
        standalone("C-IND", &books, IndustryPresentation::Industrial),
        ReportKind::Monthly,
        "2030-06",
    ))
    .expect("generation");
    // 2029 年无流量：income.prior_year = Unavailable —— 合法可公布。
    verify_comparative_honesty(&set, false, true).expect("typed unavailability is publishable");

    // 捏造：缺上年流量却填 Available 的空栏。
    let mut fabricated = set.clone();
    fabricated.income.prior_year = Comparative::Available(IncomeColumns::default());
    let err = verify_comparative_honesty(&fabricated, false, true)
        .expect_err("fabricated comparative must be rejected");
    assert!(matches!(err, ReportError::ComparativeFabricated { .. }));

    // 捏造：缺上年年末余额却填 Available。
    let mut fabricated_bs = set.clone();
    fabricated_bs.balance_sheet.prior_year_end = Comparative::Available(Vec::new());
    assert!(matches!(
        verify_comparative_honesty(&fabricated_bs, false, false),
        Err(ReportError::ComparativeFabricated { .. })
    ));
}

/// 非法报告种类：半年报只能落在 6 月、年报只能落在 12 月。
#[test]
fn invalid_report_kind_rejects() {
    let books = industrial_fixture();
    let err = generate_report_set(original_request(
        standalone("C-IND", &books, IndustryPresentation::Industrial),
        ReportKind::HalfYear,
        "2030-03",
    ))
    .expect_err("half-year report at March must be rejected");
    assert!(matches!(err, ReportError::InvalidReportKind { .. }));
    let err = generate_report_set(original_request(
        standalone("C-IND", &books, IndustryPresentation::Industrial),
        ReportKind::Annual,
        "2030-06",
    ))
    .expect_err("annual report at June must be rejected");
    assert!(matches!(err, ReportError::InvalidReportKind { .. }));
}

/// 更正守卫：目标期间未封账 / 目标无已公布版本 / 调整分录落入已封期间。
#[test]
fn correction_guards_reject() {
    let mut books = industrial_fixture();
    let mut closing = ClosingEngine::new();
    let id = MemberId("C-IND".to_string());
    let industry = IndustryPresentation::Industrial;
    // 只封 2030-01..06（后续月份仍开放）。
    for m in 1..=6u8 {
        books.close_period(AccountingPeriod::from_ymd(2030, m).expect("month")).expect("close");
    }
    // 先经结账引擎登记一个可更正的版本。
    let mut registered = ClosingEngine::new();
    let mut fresh = industrial_fixture();
    for m in 1..=6u8 {
        registered
            .close_month(&mut fresh, &id, industry, AccountingPeriod::from_ymd(2030, m).expect("month"))
            .expect("close");
    }

    // 目标未封账：2030-09 仍开放。
    let err = closing
        .correct(
            &mut books,
            &id,
            industry,
            (AccountingPeriod::from_ymd(2030, 9).expect("month"), ReportKind::Monthly),
            CorrectionRequest {
                entries: vec![entry(99, "2030-10-01", BusinessKind::CashExpense, CashFlowClass::Operating, &[
                    ("6602", PostingSide::Debit, 1), ("1002", PostingSide::Credit, 1),
                ])],
                reason: "test".to_string(),
            },
        )
        .expect_err("open-period target must be rejected");
    assert!(matches!(
        err,
        engine::accounting::closing::ClosingError::CorrectionTargetNotClosed { .. }
    ));

    // 目标无已公布版本（2030-06 已封账但从未经结账引擎定稿）。
    let err = closing
        .correct(
            &mut books,
            &id,
            industry,
            (AccountingPeriod::from_ymd(2030, 6).expect("month"), ReportKind::Monthly),
            CorrectionRequest {
                entries: vec![entry(98, "2030-07-01", BusinessKind::CashExpense, CashFlowClass::Operating, &[
                    ("6602", PostingSide::Debit, 1), ("1002", PostingSide::Credit, 1),
                ])],
                reason: "test".to_string(),
            },
        )
        .expect_err("unclosed-by-engine target must be rejected");
    assert!(matches!(
        err,
        engine::accounting::closing::ClosingError::NoVersionToSupersede { .. }
    ));

    // 调整分录早于目标期间（2030-05 < 2030-06）→ 更正守卫拒绝（前向约束）。
    let before = fresh.clone();
    let err = registered
        .correct(
            &mut fresh,
            &id,
            industry,
            (AccountingPeriod::from_ymd(2030, 6).expect("month"), ReportKind::Monthly),
            CorrectionRequest {
                entries: vec![entry(97, "2030-05-20", BusinessKind::CashExpense, CashFlowClass::Operating, &[
                    ("6602", PostingSide::Debit, 1), ("1002", PostingSide::Credit, 1),
                ])],
                reason: "test".to_string(),
            },
        )
        .expect_err("back-dated adjustment must be rejected");
    assert!(matches!(
        err,
        engine::accounting::closing::ClosingError::CorrectionEntriesNotForward { .. }
    ));
    assert_eq!(fresh, before, "rejected correction must leave books untouched");

    // 已封期间直接入账 → 底座 ClosedPeriod 守卫（批级中止包装；结账路径的
    // 前置保证——更正调整因此只能过账于开放期间）。
    let err = fresh
        .post_batch(vec![entry(96, "2030-05-21", BusinessKind::CashExpense, CashFlowClass::Operating, &[
            ("6602", PostingSide::Debit, 1), ("1002", PostingSide::Credit, 1),
        ])])
        .expect_err("closed-period posting must be rejected");
    assert!(matches!(
        err,
        engine::accounting::AccountingError::BatchAborted { ref cause, .. }
            if matches!(**cause, engine::accounting::AccountingError::ClosedPeriod { .. })
    ));
    assert_eq!(fresh, before, "rejected posting must leave books untouched");
}
