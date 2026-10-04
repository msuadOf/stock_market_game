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

#[test]
fn mixed_industry_receivables_keep_their_statement_meaning() {
    use crate::fixture::{books_with, entry};
    use engine::accounting::consolidation::{ConsolidationRequest, GroupMember, MemberSpec};
    use engine::accounting::{BusinessKind, CashFlowClass, PostingSide};
    let make_books = |chart, liability: &str| {
        books_with(
            chart,
            vec![
                entry(
                    1,
                    "2029-12-31",
                    BusinessKind::OpeningBalance,
                    CashFlowClass::Financing,
                    &[
                        ("1002", PostingSide::Debit, 100),
                        ("4001", PostingSide::Credit, 100),
                    ],
                ),
                entry(
                    2,
                    "2030-03-01",
                    BusinessKind::CreditSale,
                    CashFlowClass::NonCash,
                    &[
                        ("1122", PostingSide::Debit, 20),
                        (liability, PostingSide::Credit, 20),
                    ],
                ),
            ],
        )
    };
    let parent = make_books(engine::company::industrial::industrial_chart_v2(), "6001");
    let sub = make_books(engine::company::insurance::insurance_chart_v4(), "2501");
    let member = |id: &str, parent: Option<&str>, held, books| GroupMember {
        spec: MemberSpec {
            id: MemberId(id.into()),
            group_parent: parent.map(|id| MemberId(id.into())),
            issued_shares: 100,
            parent_held_shares: held,
        },
        books,
    };
    let set = generate_report_set(ReportRequest {
        period: AccountingPeriod::from_iso("2030-03").unwrap(),
        kind: ReportKind::Monthly,
        source: ReportSource::Consolidated {
            request: ConsolidationRequest {
                root: MemberId("root".into()),
                members: vec![
                    member("root", None, 0, &parent),
                    member("sub", Some("root"), 80, &sub),
                ],
                intercompany_balances: vec![],
                intercompany_sales: vec![],
            },
        },
        version: ReportVersion {
            sequence: 1,
            supersedes: None,
            kind: VersionKind::Original,
        },
        adjustments: &BTreeMap::new(),
    })
    .expect("mixed industry report must generate");
    set.validate().unwrap();
    assert_eq!(bs_amount(&set, BsLine::Receivables), yuan(20));
    assert_eq!(bs_amount(&set, BsLine::InsuranceReceivables), yuan(20));
    assert_eq!(set.balance_sheet.total_assets, yuan(240));
    assert_eq!(set.cash_flow.closing_cash, yuan(200));
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
fn consolidated_missing_parent_income_is_rejected_at_report_validation() {
    let parent = group_parent_books();
    let sub = group_sub_books();
    let mut report = generate_report_set(ReportRequest {
        period: AccountingPeriod::from_iso("2030-03").unwrap(),
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
    .unwrap();
    report.income.net_income_to_parent = None;
    let error = report
        .validate()
        .expect_err("Consolidated report must carry parent income");
    assert!(
        matches!(&error, engine::accounting::reports::ReportError::MissingParentIncome { scope, period } if scope == &ScopeId::Consolidated(MemberId(GROUP_ROOT.into())) && *period == AccountingPeriod::from_iso("2030-03").unwrap())
    );
    assert!(error.to_string().contains("net_income_to_parent"));
    let mut closing = engine::accounting::closing::ClosingEngine::new();
    assert!(closing.record(report).is_err());
}

#[test]
fn standalone_parent_income_none_remains_valid() {
    let books = crate::fixture::industrial_fixture();
    let report = generate_report_set(ReportRequest {
        period: AccountingPeriod::from_iso("2030-06").unwrap(),
        kind: ReportKind::Monthly,
        source: crate::fixture::standalone(
            "standalone",
            &books,
            engine::accounting::reports::IndustryPresentation::Industrial,
        ),
        version: ReportVersion {
            sequence: 1,
            supersedes: None,
            kind: VersionKind::Original,
        },
        adjustments: &BTreeMap::new(),
    })
    .unwrap();
    assert_eq!(report.income.net_income_to_parent, None);
    report.validate().unwrap();
}

#[test]
fn closing_deserialize_checks_parent_income_before_key_rebuild() {
    let parent = group_parent_books();
    let sub = group_sub_books();
    let report = generate_report_set(ReportRequest {
        period: AccountingPeriod::from_iso("2030-03").unwrap(),
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
    .unwrap();
    let mut closing = engine::accounting::closing::ClosingEngine::new();
    closing.record(report).unwrap();
    let original = serde_json::to_value(&closing).unwrap();
    for duplicate in [false, true] {
        let mut corrupted = original.clone();
        corrupted["versions"][0][3][0]["income"]["net_income_to_parent"] = serde_json::Value::Null;
        if duplicate {
            corrupted["versions"]
                .as_array_mut()
                .unwrap()
                .push(original["versions"][0].clone());
        }
        let error = serde_json::from_value::<engine::accounting::closing::ClosingEngine>(corrupted)
            .expect_err("closing restore must check each raw report before rebuilding keys");
        assert!(error.to_string().contains("income.net_income_to_parent"));
    }
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
    assert_eq!(
        bs_amount(&set, BsLine::AccountsPayable),
        AccountingAmount::ZERO
    );
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
