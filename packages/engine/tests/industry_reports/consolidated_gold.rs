//! 合并 Scope 金样：工业母公司 + 80% 工业子公司（内部赊销 + 未实现利润抵销）。
//!
//! 手算基准（元，2030-03 月报）：
//! - 合并净利 1,300（对外收入 2,800 − 集团成本 1,500）；少数损益 60 / 归母 1,240。
//! - 合并权益 121,300 = 实收资本（母）100,000 + 归母留存 17,240 + 少数 4,060。
//! - 现金 120,400；经营 CF +400（母 −400 + 子 +800）。

use crate::fixture::{group_parent_books, group_request, group_sub_books, GROUP_ROOT};
use engine::accounting::consolidation::{MemberId, ScopeId};
use engine::accounting::reports::{
    generate_report_set, BsLine, Comparative, IncomeLine, ReportKind, ReportRequest, ReportSource,
    ReportVersion, UnavailableReason, VersionKind,
};
use engine::accounting::{AccountingAmount, AccountingPeriod};
use std::collections::BTreeMap;

fn yuan(v: i128) -> AccountingAmount {
    AccountingAmount::from_cents(v * 100)
}

fn bs_amount(set: &engine::accounting::reports::ReportSet, line: BsLine) -> AccountingAmount {
    set.balance_sheet
        .asset_lines
        .iter()
        .chain(set.balance_sheet.liability_lines.iter())
        .chain(set.balance_sheet.equity_lines.iter())
        .find(|(l, _)| *l == line)
        .map(|(_, v)| *v)
        .unwrap_or_else(|| panic!("line {line:?} missing"))
}

#[test]
fn consolidated_march_gold() {
    let parent = group_parent_books();
    let sub = group_sub_books();
    let set = generate_report_set(ReportRequest {
        period: AccountingPeriod::from_iso("2030-03").expect("period"),
        kind: ReportKind::Monthly,
        source: ReportSource::Consolidated {
            request: group_request(&parent, &sub),
        },
        version: ReportVersion {
            sequence: 1,
            supersedes: None,
            kind: VersionKind::Original,
        },
        adjustments: &BTreeMap::new(),
    })
    .expect("consolidated report must generate");
    set.validate().expect("consolidated set must cross-foot");

    assert_eq!(
        set.scope,
        ScopeId::Consolidated(MemberId(GROUP_ROOT.to_string()))
    );

    // —— 资产负债表：内部应收/应付全额抵销，存货扣未实现利润 200 ——
    let bs = &set.balance_sheet;
    assert_eq!(bs_amount(&set, BsLine::CashFunds), yuan(120_400));
    assert_eq!(bs_amount(&set, BsLine::Inventory), yuan(900));
    assert_eq!(bs_amount(&set, BsLine::Receivables), AccountingAmount::ZERO);
    assert_eq!(bs_amount(&set, BsLine::AccountsPayable), AccountingAmount::ZERO);
    assert_eq!(bs.total_assets, yuan(121_300));
    assert_eq!(bs.total_liabilities, AccountingAmount::ZERO);
    assert_eq!(bs_amount(&set, BsLine::PaidInCapital), yuan(100_000));
    assert_eq!(bs_amount(&set, BsLine::RetainedEarnings), yuan(17_240));
    assert_eq!(bs_amount(&set, BsLine::MinorityEquity), yuan(4_060));
    assert_eq!(bs.total_equity, yuan(121_300));
    assert_eq!(bs.equity_to_parent, yuan(117_240));
    assert_eq!(bs.liabilities_and_equity, bs.total_assets);

    // 上年年末比较项：合并拆分口径（少数 = 子公司期初权益 × 20%）。
    let Comparative::Available(prior) = &bs.prior_year_end else {
        panic!("prior year end must be available")
    };
    let prior_cash = prior
        .iter()
        .find(|(l, _)| *l == BsLine::CashFunds)
        .map(|(_, v)| *v)
        .expect("prior cash");
    assert_eq!(prior_cash, yuan(120_000));
    let prior_minority = prior
        .iter()
        .find(|(l, _)| *l == BsLine::MinorityEquity)
        .map(|(_, v)| *v)
        .expect("prior minority");
    assert_eq!(prior_minority, yuan(4_000));

    // —— 利润表：合并口径收入 2,800（母 3,000 − 抵销 1,000）/ 成本 1,500 ——
    let cum = &set.income.cumulative;
    let revenue = cum
        .operating
        .iter()
        .find(|(l, _)| *l == IncomeLine::OperatingRevenue)
        .map(|(_, v)| *v)
        .expect("revenue");
    assert_eq!(revenue, yuan(2_800));
    let cost = cum
        .operating
        .iter()
        .find(|(l, _)| *l == IncomeLine::OperatingCost)
        .map(|(_, v)| *v)
        .expect("cost");
    assert_eq!(cost, yuan(1_500));
    assert_eq!(cum.net_income, yuan(1_300));
    assert_eq!(set.income.minority_net_income, Some(yuan(60)));
    assert_eq!(set.income.net_income_to_parent, Some(yuan(1_240)));

    // 上年同期：无历史流量 ⇒ 类型化不可得。
    assert!(matches!(
        set.income.prior_year,
        Comparative::Unavailable {
            reason: UnavailableReason::NoPriorYearHistory
        }
    ));

    // —— 现金流量表：Σ成员（抵销不触现金）——
    let cf = &set.cash_flow;
    assert_eq!(cf.operating, yuan(400));
    assert_eq!(cf.investing, AccountingAmount::ZERO);
    assert_eq!(cf.financing, AccountingAmount::ZERO);
    assert_eq!(cf.opening_cash, yuan(120_000));
    assert_eq!(cf.closing_cash, yuan(120_400));
    // 间接法：NI 1,300 − 存货增加 900 = 400（应收/应付抵销后运动为 0，不产生行）。
    let sum: i128 = cf.indirect.iter().map(|l| l.amount.cents()).sum();
    assert_eq!(sum, 400 * 100);

    // —— 所有者权益变动表：合并拆分 ——
    let eq = &set.equity;
    assert_eq!(eq.opening_parent, yuan(116_000));
    assert_eq!(eq.net_income, yuan(1_240));
    assert_eq!(eq.opening_minority, Some(yuan(4_000)));
    assert_eq!(eq.minority_net_income, Some(yuan(60)));
    assert_eq!(eq.closing_parent, yuan(117_240));
    assert_eq!(eq.closing_minority, Some(yuan(4_060)));

    // —— 附注：子公司权益科目进入合并拆分披露（不参与主表行勾稽）——
    assert!(set
        .notes
        .consolidation_split_items
        .iter()
        .any(|item| item.code == "4001"));
}
