//! 工业金样：6 月月报 / Q2 季报快照 / 年报 五产物逐行断言 + 重建性质。
//!
//! 手算基准见 fixture.rs 头注。所有金额以元表述、断言用分。

use crate::fixture::{industrial_fixture, standalone};
use engine::accounting::AccountingPeriod;
use engine::accounting::reports::{
    generate_report_set, BsLine, Comparative, IncomeLine, IndustryPresentation, ReportKind,
    ReportRequest, ReportSource, ReportVersion, UnavailableReason, VersionKind,
};
use engine::accounting::{AccountingAmount, BusinessEventId};
use std::collections::BTreeMap;

fn period(iso: &str) -> AccountingPeriod {
    AccountingPeriod::from_iso(iso).expect("fixture period")
}

fn yuan(v: i128) -> AccountingAmount {
    AccountingAmount::from_cents(v * 100)
}

/// 在行集合中取指定行（缺失即 panic——分类必须完整）。
fn amount(lines: &[(BsLine, AccountingAmount)], line: BsLine) -> AccountingAmount {
    lines
        .iter()
        .find(|(l, _)| *l == line)
        .map(|(_, v)| *v)
        .unwrap_or_else(|| panic!("line {line:?} missing"))
}

fn income_amount(lines: &[(IncomeLine, AccountingAmount)], line: IncomeLine) -> AccountingAmount {
    lines
        .iter()
        .find(|(l, _)| *l == line)
        .map(|(_, v)| *v)
        .unwrap_or_else(|| panic!("income line {line:?} missing"))
}

fn original_request<'a>(
    period_iso: &str,
    kind: ReportKind,
    source: ReportSource<'a>,
) -> ReportRequest<'a> {
    ReportRequest {
        period: period(period_iso),
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

/// 6 月月报：当季/累计双栏、期初期末、比较项、CF 直接/间接勾稽全锁。
#[test]
fn industrial_june_monthly_gold() {
    let books = industrial_fixture();
    let set = generate_report_set(original_request(
        "2030-06",
        ReportKind::Monthly,
        standalone("C-IND", &books, IndustryPresentation::Industrial),
    ))
    .expect("report generation must succeed");
    set.validate().expect("generated set must cross-foot");

    // —— 资产负债表（期末）——
    let bs = &set.balance_sheet;
    assert_eq!(amount(&bs.asset_lines, BsLine::CashFunds), yuan(97_300));
    assert_eq!(amount(&bs.asset_lines, BsLine::Receivables), yuan(500));
    assert_eq!(amount(&bs.asset_lines, BsLine::FixedAssets), yuan(9_880));
    assert_eq!(bs.total_assets, yuan(107_680));
    assert_eq!(amount(&bs.liability_lines, BsLine::ShortTermBorrowings), yuan(5_000));
    assert_eq!(amount(&bs.liability_lines, BsLine::TaxesPayable), yuan(60));
    assert_eq!(amount(&bs.liability_lines, BsLine::InterestPayable), yuan(25));
    assert_eq!(bs.total_liabilities, yuan(5_085));
    assert_eq!(amount(&bs.equity_lines, BsLine::PaidInCapital), yuan(100_000));
    assert_eq!(amount(&bs.equity_lines, BsLine::RetainedEarnings), yuan(2_595));
    assert_eq!(bs.total_equity, yuan(102_595));
    assert_eq!(bs.equity_to_parent, bs.total_equity);
    assert_eq!(bs.liabilities_and_equity, bs.total_assets);
    assert_eq!(bs.closing_cash, yuan(97_300));

    // 上年年末比较项：开局分录存在 ⇒ Available（真实余额，非捏造）。
    let Comparative::Available(prior) = &bs.prior_year_end else {
        panic!("prior year end must be available (opening entries exist)")
    };
    assert_eq!(amount(prior, BsLine::CashFunds), yuan(90_000));
    assert_eq!(amount(prior, BsLine::FixedAssets), yuan(10_000));
    assert_eq!(amount(prior, BsLine::PaidInCapital), yuan(100_000));

    // —— 利润表：累计（1–6 月）——
    let cum = &set.income.cumulative;
    assert_eq!(income_amount(&cum.operating, IncomeLine::OperatingRevenue), yuan(3_500));
    assert_eq!(income_amount(&cum.operating, IncomeLine::OperatingCost), yuan(700));
    assert_eq!(income_amount(&cum.operating, IncomeLine::AdministrativeExpense), yuan(120));
    assert_eq!(cum.operating_subtotal, yuan(2_680));
    assert_eq!(income_amount(&cum.financing, IncomeLine::FinanceExpense), yuan(25));
    assert_eq!(cum.financing_subtotal, yuan(-25));
    assert_eq!(cum.income_tax, yuan(60));
    assert_eq!(cum.net_income, yuan(2_595));

    // —— 利润表：当季（4–6 月）——
    let q = &set.income.quarter;
    assert_eq!(income_amount(&q.operating, IncomeLine::OperatingRevenue), AccountingAmount::ZERO);
    assert_eq!(income_amount(&q.operating, IncomeLine::OperatingCost), yuan(700));
    assert_eq!(q.operating_subtotal, yuan(-700));
    assert_eq!(q.net_income, yuan(-785));

    // 上年同期：2029 年 1–6 月无分录 ⇒ 类型化 Unavailable（绝不填零）。
    match &set.income.prior_year {
        Comparative::Unavailable { reason } => {
            assert_eq!(*reason, UnavailableReason::NoPriorYearHistory);
        }
        Comparative::Available(_) => panic!("prior-year income must be unavailable"),
    }

    // —— 现金流量表（6 月窗口；6 月全部非现金 ⇒ 三类皆 0）——
    let cf = &set.cash_flow;
    assert_eq!(cf.operating, AccountingAmount::ZERO);
    assert_eq!(cf.investing, AccountingAmount::ZERO);
    assert_eq!(cf.financing, AccountingAmount::ZERO);
    // 现金分录全部落在 1–5 月 ⇒ 6 月期初 = 期末 = 97,300。
    assert_eq!(cf.opening_cash, yuan(97_300));
    assert_eq!(cf.closing_cash, yuan(97_300));

    // —— 间接法：净利 ± 非现金/营运资金项目 = 经营现金（逐行手算）——
    // 6 月窗口：NI −85；应付利息 +25；应交所得税 +60 ⇒ 0。
    assert_eq!(cf.indirect[0].label, "净利润");
    assert_eq!(cf.indirect[0].amount, yuan(-85));
    let sum: i128 = cf.indirect.iter().map(|l| l.amount.cents()).sum();
    assert_eq!(sum, 0, "indirect must reconcile to operating (0)");

    // —— 所有者权益变动表：期初 102,680 − 85 = 期末 102,595 ——
    let eq = &set.equity;
    assert_eq!(eq.opening_parent, yuan(102_680));
    assert_eq!(eq.net_income, yuan(-85));
    assert_eq!(eq.other_comprehensive, AccountingAmount::ZERO);
    assert_eq!(eq.distributions, AccountingAmount::ZERO, "guardrail #4: 分配恒 0");
    assert_eq!(eq.capital_contributions, AccountingAmount::ZERO);
    assert_eq!(eq.closing_parent, yuan(102_595));
    assert!(eq.opening_minority.is_none() && eq.closing_minority.is_none());

    // —— 附注：有发生额科目全部归类（13 个）——
    assert_eq!(set.notes.items.len(), 13);
}

/// 年报（Annual, 2030-12）：全年窗口五产物。
#[test]
fn industrial_annual_gold() {
    let books = industrial_fixture();
    let set = generate_report_set(original_request(
        "2030-12",
        ReportKind::Annual,
        standalone("C-IND", &books, IndustryPresentation::Industrial),
    ))
    .expect("annual report must generate");
    set.validate().expect("annual set must cross-foot");

    let bs = &set.balance_sheet;
    assert_eq!(amount(&bs.asset_lines, BsLine::CashFunds), yuan(100_275));
    assert_eq!(bs.total_assets, yuan(110_655));
    assert_eq!(amount(&bs.liability_lines, BsLine::InterestPayable), AccountingAmount::ZERO);
    assert_eq!(bs.total_liabilities, yuan(5_060));
    assert_eq!(amount(&bs.equity_lines, BsLine::RetainedEarnings), yuan(5_595));

    let cum = &set.income.cumulative;
    assert_eq!(income_amount(&cum.operating, IncomeLine::OperatingRevenue), yuan(6_500));
    assert_eq!(cum.net_income, yuan(5_595));

    let cf = &set.cash_flow;
    assert_eq!(cf.operating, yuan(5_300));
    assert_eq!(cf.investing, AccountingAmount::ZERO);
    assert_eq!(cf.financing, yuan(4_975));
    assert_eq!(cf.opening_cash, yuan(90_000));
    assert_eq!(cf.closing_cash, yuan(100_275));
    // 间接法：5,595 + (应收 −500 + 折旧 +120 + 短借 +5,000 + 应交税 +60) − 筹资 4,975 = 5,300。
    assert_eq!(cf.indirect[0].amount, yuan(5_595));
    let sum: i128 = cf.indirect.iter().map(|l| l.amount.cents()).sum();
    assert_eq!(sum, 5_300 * 100);

    // 年报当季列 = Q4（10–12 月）：收入 3,000、费用 0。
    assert_eq!(set.income.quarter.net_income, yuan(3_000));
}

/// Q2 季报快照：窗口 4–6 月，期初 = 3 月末余额。
#[test]
fn industrial_q2_snapshot_gold() {
    let books = industrial_fixture();
    let set = generate_report_set(original_request(
        "2030-06",
        ReportKind::Quarter,
        standalone("C-IND", &books, IndustryPresentation::Industrial),
    ))
    .expect("quarter snapshot must generate");
    set.validate().expect("quarter set must cross-foot");

    assert_eq!(set.window, (period("2030-04"), period("2030-06")));
    let cf = &set.cash_flow;
    assert_eq!(cf.operating, yuan(-700));
    assert_eq!(cf.financing, yuan(5_000));
    assert_eq!(cf.opening_cash, yuan(93_000));
    assert_eq!(cf.closing_cash, yuan(97_300));
    assert_eq!(set.income.cumulative.net_income, yuan(2_595));
    assert_eq!(set.income.quarter.net_income, yuan(-785));
}

/// 重建性质：同一账套生成两次 ⇒ 序列化字节相同（无快照独立随机化）。
#[test]
fn reconstruction_is_byte_equal() {
    let books = industrial_fixture();
    let a = generate_report_set(original_request(
        "2030-06",
        ReportKind::Monthly,
        standalone("C-IND", &books, IndustryPresentation::Industrial),
    ))
    .expect("first generation");
    let b = generate_report_set(original_request(
        "2030-06",
        ReportKind::Monthly,
        standalone("C-IND", &books, IndustryPresentation::Industrial),
    ))
    .expect("second generation");
    assert_eq!(a, b, "same journal must derive equal ReportSet");
    let sa = serde_json::to_string(&a).expect("ReportSet serializes");
    let sb = serde_json::to_string(&b).expect("ReportSet serializes");
    assert_eq!(sa, sb, "serialized bytes must be equal");

    // 调整分录映射为空时，包含未映射分录的账套不受影响（纯函数输入不变）。
    let with_empty: BTreeMap<BusinessEventId, AccountingPeriod> = BTreeMap::new();
    let c = generate_report_set(ReportRequest {
        period: period("2030-06"),
        kind: ReportKind::Monthly,
        source: standalone("C-IND", &books, IndustryPresentation::Industrial),
        version: ReportVersion {
            sequence: 1,
            supersedes: None,
            kind: VersionKind::Original,
        },
        adjustments: &with_empty,
    })
    .expect("generation with explicit empty adjustments");
    assert_eq!(a, c);
}
