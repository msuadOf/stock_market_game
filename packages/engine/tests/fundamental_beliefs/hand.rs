//! 手工 ReportSet/PublishedReport 构造器（核心抽取层的直测面——不经结账
//! 机器；字段最小可读，勾稽由真实链路测试覆盖）。

use crate::{COMPANY, d};
use engine::accounting::AccountingAmount;
use engine::accounting::AccountingPeriod;
use engine::accounting::consolidation::{MemberId, ScopeId};
use engine::accounting::reports::{
    BalanceSheet, BsLine, CashFlowStatement, Comparative, EquityStatement, IncomeColumns,
    IncomeLine, IncomeStatement, Notes, ReportKind, ReportSet, ReportVersion, UnavailableReason,
    VersionKind,
};
use engine::calendar::CivilInstant;
use engine::company::CompanyId;
use engine::information::{
    AccountingPolicyRef, PublicationId, PublicationOrigin, PublishedReport, ScheduledReportKind,
};

/// 手工 ReportSet 规格（核心抽取层直测面；分单位）。
pub(crate) struct HandReportSpec {
    pub kind: ReportKind,
    pub consolidated: bool,
    pub equity_to_parent_cents: i128,
    pub total_equity_cents: i128,
    pub opening_parent_cents: i128,
    pub ni_total_cents: i128,
    pub ni_to_parent_cents: Option<i128>,
    pub revenue_cents: i128,
    /// None ⇒ 上年同期比较项 Unavailable（缺历史）。
    pub prior_revenue_cents: Option<i128>,
    pub operating_cf_cents: i128,
    pub investing_cf_cents: i128,
    pub financing_cf_cents: i128,
    pub published_on: &'static str,
}

impl Default for HandReportSpec {
    fn default() -> Self {
        Self {
            kind: ReportKind::Annual,
            consolidated: false,
            equity_to_parent_cents: 0,
            total_equity_cents: 0,
            opening_parent_cents: 0,
            ni_total_cents: 0,
            ni_to_parent_cents: None,
            revenue_cents: 0,
            prior_revenue_cents: None,
            operating_cf_cents: 0,
            investing_cf_cents: 0,
            financing_cf_cents: 0,
            published_on: "2031-03-20",
        }
    }
}

fn income_columns(revenue_cents: i128, net_income_cents: i128) -> IncomeColumns {
    IncomeColumns {
        operating: vec![(
            IncomeLine::OperatingRevenue,
            AccountingAmount::from_cents(revenue_cents),
        )],
        operating_subtotal: AccountingAmount::from_cents(revenue_cents),
        net_income: AccountingAmount::from_cents(net_income_cents),
        ..IncomeColumns::default()
    }
}

/// 构造最小可读 PublishedReport（抽取层只读字段，不经过结账勾稽机器）。
pub(crate) fn hand_report(spec: HandReportSpec) -> PublishedReport {
    let minority_ni = spec
        .ni_to_parent_cents
        .map(|parent| spec.ni_total_cents - parent);
    let member = MemberId(COMPANY.to_string());
    let scope = if spec.consolidated {
        ScopeId::Consolidated(member)
    } else {
        ScopeId::Standalone(member)
    };
    let published_at = CivilInstant::from_hms(d(spec.published_on), 18, 0, 0).expect("phase");
    let net_change = spec.operating_cf_cents + spec.investing_cf_cents + spec.financing_cf_cents;
    PublishedReport {
        id: PublicationId::new(901),
        company: CompanyId(COMPANY.to_string()),
        policy: AccountingPolicyRef { chart_version: 2 },
        approved_at: CivilInstant::from_hms(d(spec.published_on), 8, 0, 0).expect("approval"),
        published_at,
        origin: PublicationOrigin::SeededPrehistory {
            fiscal_year: 2030,
            kind: ScheduledReportKind::Annual,
            offset_days: 0,
        },
        supersedes: None,
        reports: ReportSet {
            scope,
            period: AccountingPeriod::from_ymd(2030, 12).expect("annual period"),
            kind: spec.kind,
            window: (
                AccountingPeriod::from_ymd(2030, 1).expect("jan"),
                AccountingPeriod::from_ymd(2030, 12).expect("dec"),
            ),
            version: ReportVersion {
                sequence: 1,
                supersedes: None,
                kind: VersionKind::Original,
            },
            balance_sheet: BalanceSheet {
                asset_lines: Vec::new(),
                total_assets: AccountingAmount::ZERO,
                liability_lines: Vec::new(),
                total_liabilities: AccountingAmount::ZERO,
                equity_lines: vec![
                    (
                        BsLine::PaidInCapital,
                        AccountingAmount::from_cents(spec.equity_to_parent_cents),
                    ),
                    (BsLine::RetainedEarnings, AccountingAmount::ZERO),
                ],
                total_equity: AccountingAmount::from_cents(spec.total_equity_cents),
                equity_to_parent: AccountingAmount::from_cents(spec.equity_to_parent_cents),
                liabilities_and_equity: AccountingAmount::from_cents(spec.total_equity_cents),
                closing_cash: AccountingAmount::ZERO,
                prior_year_end: Comparative::Unavailable {
                    reason: UnavailableReason::NoPriorYearHistory,
                },
            },
            income: IncomeStatement {
                quarter: IncomeColumns::default(),
                cumulative: income_columns(spec.revenue_cents, spec.ni_total_cents),
                prior_year: match spec.prior_revenue_cents {
                    Some(prior) => Comparative::Available(income_columns(prior, 0)),
                    None => Comparative::Unavailable {
                        reason: UnavailableReason::NoPriorYearHistory,
                    },
                },
                minority_net_income: minority_ni.map(AccountingAmount::from_cents),
                net_income_to_parent: spec.ni_to_parent_cents.map(AccountingAmount::from_cents),
            },
            cash_flow: CashFlowStatement {
                operating: AccountingAmount::from_cents(spec.operating_cf_cents),
                investing: AccountingAmount::from_cents(spec.investing_cf_cents),
                financing: AccountingAmount::from_cents(spec.financing_cf_cents),
                net_change: AccountingAmount::from_cents(net_change),
                opening_cash: AccountingAmount::ZERO,
                closing_cash: AccountingAmount::from_cents(net_change),
                indirect: Vec::new(),
            },
            equity: EquityStatement {
                opening_parent: AccountingAmount::from_cents(spec.opening_parent_cents),
                net_income: AccountingAmount::from_cents(
                    spec.ni_to_parent_cents.unwrap_or(spec.ni_total_cents),
                ),
                other_comprehensive: AccountingAmount::ZERO,
                capital_contributions: AccountingAmount::ZERO,
                distributions: AccountingAmount::ZERO,
                closing_parent: AccountingAmount::from_cents(spec.equity_to_parent_cents),
                opening_minority: None,
                minority_net_income: minority_ni.map(AccountingAmount::from_cents),
                closing_minority: None,
            },
            notes: Notes {
                items: Vec::new(),
                consolidation_split_items: Vec::new(),
            },
        },
    }
}
