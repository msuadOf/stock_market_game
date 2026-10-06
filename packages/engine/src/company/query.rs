//! 面向 Host 的公司公开报表 DTO。
//!
//! 这些类型仅投影已经披露的不可变报表；公司账簿、经营状态、NPC 信息与信念
//! 等私有权威状态不得跨过此边界。

use crate::accounting::consolidation::ScopeId;
use crate::accounting::reports::{
    BsLine, Comparative, IncomeColumns, IncomeLine, NoteItem, NoteTarget, ReportRoe, ReportSet,
    UnavailableReason, VersionKind,
};
use crate::accounting::AccountingAmount;
use crate::information::period_end_date;
use crate::information::PublishedReport;

pub const DEFAULT_PUBLIC_REPORT_PAGE_SIZE: u16 = 20;
pub const MAX_PUBLIC_REPORT_PAGE_SIZE: u16 = 100;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct PublicReportQuery {
    pub company_id: String,
    pub cursor: Option<String>,
    pub page_size: Option<u16>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct PublicReportAvailabilityQuery {
    pub company_id: String,
    pub period_end: String,
    pub kind: PublicReportKind,
    pub scope: PublicReportScope,
}

#[derive(Clone, Debug, serde::Serialize, ts_rs::TS)]
#[serde(tag = "status")]
#[ts(export)]
pub enum PublicReportAvailability {
    Available { report: PublicReportSummary },
    Unavailable { reason: PublicReportUnavailableReason },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub enum PublicReportUnavailableReason {
    BeforeOpening,
    NotYetSettled,
    PeriodNotRepresented,
    NotYetPublished,
    NotScheduled,
    ScopeNotRepresented,
}

#[derive(Clone, Debug, serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct PublicReportPage {
    pub reports: Vec<PublicReportSummary>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct PublicReportSummary {
    /// PublicationId 使用不透明十进制字符串，且是分页排序的唯一 key。
    pub id: String,
    pub company_id: String,
    pub source: crate::information::PublicationSource,
    pub period: String,
    pub kind: PublicReportKind,
    pub version_sequence: String,
    pub supersedes: Option<String>,
    pub approved_date: String,
    pub approved_second_of_day: u32,
    pub published_date: String,
    pub published_second_of_day: u32,
    pub accounting: PublicReportAccountingSummary,
    pub financials: PublicReportFinancials,
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub enum PublicReportKind {
    Monthly,
    Quarter,
    HalfYear,
    Annual,
}

#[derive(Clone, Debug, serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct PublicReportAccountingSummary {
    /// 所有会计金额均为精确的十进制元字符串，不使用 JavaScript number。
    pub total_assets: String,
    pub total_liabilities: String,
    pub total_equity: String,
    pub closing_cash: String,
    pub report_period_net_income: String,
    pub quarter_net_income: String,
    pub net_income: String,
    pub income_tax: String,
    pub operating_cash_flow: String,
    pub investing_cash_flow: String,
    pub financing_cash_flow: String,
    pub net_cash_change: String,
    pub prior_year_net_income: PublicComparativeAmount,
}

#[derive(Clone, Debug, serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub enum PublicComparativeAmount {
    Available { amount: String },
    Unavailable { reason: PublicUnavailableReason },
}

#[derive(Clone, Copy, Debug, serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub enum PublicUnavailableReason {
    NoPriorYearHistory,
}

#[derive(Clone, Debug, serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct PublicReportFinancials {
    pub scope: PublicReportScope,
    pub window_start: String,
    pub window_end: String,
    pub version_kind: PublicReportVersionKind,
    pub version_supersedes: Option<String>,
    pub balance_sheet: PublicReportBalanceSheet,
    pub income: PublicReportIncome,
    pub cash_flow: PublicReportCashFlow,
    pub equity: PublicReportEquity,
    pub notes: PublicReportNotes,
    pub roe: ReportRoe,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub enum PublicReportScope {
    Standalone { entity_id: String },
    Consolidated { root_entity_id: String },
}

#[derive(Clone, Debug, serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub enum PublicReportVersionKind {
    Original,
    Correction { reason: String },
}

#[derive(Clone, Debug, serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct PublicReportLine {
    pub subject: String,
    pub amount: String,
}

#[derive(Clone, Debug, serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub enum PublicReportComparative<Value> {
    Available(Value),
    Unavailable { reason: PublicUnavailableReason },
}

#[derive(Clone, Debug, serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct PublicReportBalanceSheet {
    pub asset_lines: Vec<PublicReportLine>,
    pub total_assets: String,
    pub liability_lines: Vec<PublicReportLine>,
    pub total_liabilities: String,
    pub equity_lines: Vec<PublicReportLine>,
    pub total_equity: String,
    pub equity_to_parent: String,
    pub liabilities_and_equity: String,
    pub closing_cash: String,
    pub prior_year_end: PublicReportComparative<PublicReportBalanceComparison>,
}

#[derive(Clone, Debug, serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct PublicReportBalanceComparison {
    pub lines: Vec<PublicReportLine>,
    pub total_assets: String,
    pub total_liabilities: String,
    pub total_equity: String,
    pub equity_to_parent: String,
    pub liabilities_and_equity: String,
}

#[derive(Clone, Debug, serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct PublicReportIncomeColumns {
    pub operating: Vec<PublicReportLine>,
    pub operating_subtotal: String,
    pub investing: Vec<PublicReportLine>,
    pub investing_subtotal: String,
    pub financing: Vec<PublicReportLine>,
    pub financing_subtotal: String,
    pub discontinued: Vec<PublicReportLine>,
    pub discontinued_subtotal: String,
    pub income_tax: String,
    pub net_income: String,
}

#[derive(Clone, Debug, serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct PublicReportIncome {
    pub report_period: PublicReportIncomeColumns,
    pub quarter: PublicReportIncomeColumns,
    pub cumulative: PublicReportIncomeColumns,
    pub prior_year: PublicReportComparative<PublicReportIncomeColumns>,
    pub minority_net_income: Option<String>,
    pub net_income_to_parent: Option<String>,
    pub report_period_net_income_to_parent: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct PublicReportCashFlow {
    pub operating: String,
    pub investing: String,
    pub financing: String,
    pub net_change: String,
    pub opening_cash: String,
    pub closing_cash: String,
    pub indirect: Vec<PublicReportLine>,
}

#[derive(Clone, Debug, serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct PublicReportEquity {
    pub opening_parent: String,
    pub net_income: String,
    pub other_comprehensive: String,
    pub capital_contributions: String,
    pub distributions: String,
    pub closing_parent: String,
    pub opening_minority: Option<String>,
    pub minority_net_income: Option<String>,
    pub closing_minority: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub enum PublicReportNoteTarget {
    BalanceSheet(String),
    Income(String),
}

#[derive(Clone, Debug, serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct PublicReportNoteItem {
    pub code: String,
    pub name: String,
    pub target: PublicReportNoteTarget,
    pub opening: String,
    pub movement: String,
    pub ytd_movement: String,
    pub closing: String,
}

#[derive(Clone, Debug, serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct PublicReportNotes {
    pub items: Vec<PublicReportNoteItem>,
    pub consolidation_split_items: Vec<PublicReportNoteItem>,
}

fn public_lines<Line>(
    lines: &[(Line, AccountingAmount)],
    label: fn(&Line) -> &'static str,
) -> Vec<PublicReportLine> {
    lines
        .iter()
        .map(|(line, amount)| PublicReportLine {
            subject: label(line).to_string(),
            amount: amount.to_yuan_string(),
        })
        .collect()
}

fn public_comparative<Value, PublicValue>(
    value: &Comparative<Value>,
    project: impl Fn(&Value) -> PublicValue,
) -> PublicReportComparative<PublicValue> {
    match value {
        Comparative::Available(value) => PublicReportComparative::Available(project(value)),
        Comparative::Unavailable { reason } => PublicReportComparative::Unavailable {
            reason: match reason {
                UnavailableReason::NoPriorYearHistory => {
                    PublicUnavailableReason::NoPriorYearHistory
                }
            },
        },
    }
}

impl From<&IncomeColumns> for PublicReportIncomeColumns {
    fn from(columns: &IncomeColumns) -> Self {
        Self {
            operating: public_lines(&columns.operating, IncomeLine::label),
            operating_subtotal: columns.operating_subtotal.to_yuan_string(),
            investing: public_lines(&columns.investing, IncomeLine::label),
            investing_subtotal: columns.investing_subtotal.to_yuan_string(),
            financing: public_lines(&columns.financing, IncomeLine::label),
            financing_subtotal: columns.financing_subtotal.to_yuan_string(),
            discontinued: public_lines(&columns.discontinued, IncomeLine::label),
            discontinued_subtotal: columns.discontinued_subtotal.to_yuan_string(),
            income_tax: columns.income_tax.to_yuan_string(),
            net_income: columns.net_income.to_yuan_string(),
        }
    }
}

impl From<&NoteItem> for PublicReportNoteItem {
    fn from(item: &NoteItem) -> Self {
        Self {
            code: item.code.clone(),
            name: item.name.clone(),
            target: match &item.target {
                NoteTarget::BalanceSheet(line) => {
                    PublicReportNoteTarget::BalanceSheet(line.label().to_string())
                }
                NoteTarget::Income(line) => {
                    PublicReportNoteTarget::Income(line.label().to_string())
                }
            },
            opening: item.opening.to_yuan_string(),
            movement: item.movement.to_yuan_string(),
            ytd_movement: item.ytd_movement.to_yuan_string(),
            closing: item.closing.to_yuan_string(),
        }
    }
}

impl From<&[(BsLine, AccountingAmount)]> for PublicReportBalanceComparison {
    fn from(lines: &[(BsLine, AccountingAmount)]) -> Self {
        let mut seen = Vec::new();
        let mut total_assets = AccountingAmount::ZERO;
        let mut total_liabilities = AccountingAmount::ZERO;
        let mut total_equity = AccountingAmount::ZERO;
        let mut equity_to_parent = AccountingAmount::ZERO;
        for (line, amount) in lines {
            assert!(
                !seen.contains(line),
                "published prior-year balance sheet contains duplicate line {line:?}"
            );
            seen.push(*line);
            if line.is_asset() {
                total_assets = total_assets
                    .add(*amount)
                    .expect("published prior-year asset aggregate must fit accounting amount");
            } else if line.is_equity() {
                total_equity = total_equity
                    .add(*amount)
                    .expect("published prior-year equity aggregate must fit accounting amount");
                if *line != BsLine::MinorityEquity {
                    equity_to_parent = equity_to_parent.add(*amount).expect(
                        "published prior-year parent equity aggregate must fit accounting amount",
                    );
                }
            } else {
                total_liabilities = total_liabilities
                    .add(*amount)
                    .expect("published prior-year liability aggregate must fit accounting amount");
            }
        }
        Self {
            lines: public_lines(lines, BsLine::label),
            total_assets: total_assets.to_yuan_string(),
            total_liabilities: total_liabilities.to_yuan_string(),
            total_equity: total_equity.to_yuan_string(),
            equity_to_parent: equity_to_parent.to_yuan_string(),
            liabilities_and_equity: total_liabilities.add(total_equity)
                .expect("published prior-year liabilities and equity aggregate must fit accounting amount")
                .to_yuan_string(),
        }
    }
}

impl From<&ReportSet> for PublicReportFinancials {
    fn from(reports: &ReportSet) -> Self {
        let balance = &reports.balance_sheet;
        let cash = &reports.cash_flow;
        let equity = &reports.equity;
        Self {
            scope: match &reports.scope {
                ScopeId::Standalone(member) => PublicReportScope::Standalone {
                    entity_id: member.0.clone(),
                },
                ScopeId::Consolidated(root) => PublicReportScope::Consolidated {
                    root_entity_id: root.0.clone(),
                },
            },
            window_start: format!(
                "{:04}-{:02}-01",
                reports.window.0.year(),
                reports.window.0.month()
            ),
            window_end: period_end_date(reports.window.1)
                .expect("a published report always has a valid window end")
                .to_iso(),
            version_kind: match &reports.version.kind {
                VersionKind::Original => PublicReportVersionKind::Original,
                VersionKind::Correction { reason } => PublicReportVersionKind::Correction {
                    reason: reason.clone(),
                },
            },
            version_supersedes: reports
                .version
                .supersedes
                .map(|sequence| sequence.to_string()),
            balance_sheet: PublicReportBalanceSheet {
                asset_lines: public_lines(&balance.asset_lines, BsLine::label),
                total_assets: balance.total_assets.to_yuan_string(),
                liability_lines: public_lines(&balance.liability_lines, BsLine::label),
                total_liabilities: balance.total_liabilities.to_yuan_string(),
                equity_lines: public_lines(&balance.equity_lines, BsLine::label),
                total_equity: balance.total_equity.to_yuan_string(),
                equity_to_parent: balance.equity_to_parent.to_yuan_string(),
                liabilities_and_equity: balance.liabilities_and_equity.to_yuan_string(),
                closing_cash: balance.closing_cash.to_yuan_string(),
                prior_year_end: public_comparative(&balance.prior_year_end, |lines| {
                    PublicReportBalanceComparison::from(lines.as_slice())
                }),
            },
            income: PublicReportIncome {
                report_period: PublicReportIncomeColumns::from(&reports.income.report_period),
                quarter: PublicReportIncomeColumns::from(&reports.income.quarter),
                cumulative: PublicReportIncomeColumns::from(&reports.income.cumulative),
                prior_year: public_comparative(&reports.income.prior_year, |columns| {
                    PublicReportIncomeColumns::from(columns)
                }),
                minority_net_income: reports
                    .income
                    .minority_net_income
                    .map(|amount| amount.to_yuan_string()),
                net_income_to_parent: reports
                    .income
                    .net_income_to_parent
                    .map(|amount| amount.to_yuan_string()),
                report_period_net_income_to_parent: reports
                    .income
                    .report_period_net_income_to_parent
                    .map(|amount| amount.to_yuan_string()),
            },
            cash_flow: PublicReportCashFlow {
                operating: cash.operating.to_yuan_string(),
                investing: cash.investing.to_yuan_string(),
                financing: cash.financing.to_yuan_string(),
                net_change: cash.net_change.to_yuan_string(),
                opening_cash: cash.opening_cash.to_yuan_string(),
                closing_cash: cash.closing_cash.to_yuan_string(),
                indirect: cash
                    .indirect
                    .iter()
                    .map(|line| PublicReportLine {
                        subject: line.label.clone(),
                        amount: line.amount.to_yuan_string(),
                    })
                    .collect(),
            },
            equity: PublicReportEquity {
                opening_parent: equity.opening_parent.to_yuan_string(),
                net_income: equity.net_income.to_yuan_string(),
                other_comprehensive: equity.other_comprehensive.to_yuan_string(),
                capital_contributions: equity.capital_contributions.to_yuan_string(),
                distributions: equity.distributions.to_yuan_string(),
                closing_parent: equity.closing_parent.to_yuan_string(),
                opening_minority: equity
                    .opening_minority
                    .map(|amount| amount.to_yuan_string()),
                minority_net_income: equity
                    .minority_net_income
                    .map(|amount| amount.to_yuan_string()),
                closing_minority: equity
                    .closing_minority
                    .map(|amount| amount.to_yuan_string()),
            },
            notes: PublicReportNotes {
                items: reports
                    .notes
                    .items
                    .iter()
                    .map(PublicReportNoteItem::from)
                    .collect(),
                consolidation_split_items: reports
                    .notes
                    .consolidation_split_items
                    .iter()
                    .map(PublicReportNoteItem::from)
                    .collect(),
            },
            roe: reports.roe.clone(),
        }
    }
}

impl From<&PublishedReport> for PublicReportSummary {
    fn from(report: &PublishedReport) -> Self {
        let reports = &report.reports;
        let accounting = PublicReportAccountingSummary {
            total_assets: reports.balance_sheet.total_assets.to_yuan_string(),
            total_liabilities: reports.balance_sheet.total_liabilities.to_yuan_string(),
            total_equity: reports.balance_sheet.total_equity.to_yuan_string(),
            closing_cash: reports.balance_sheet.closing_cash.to_yuan_string(),
            report_period_net_income: reports.income.report_period.net_income.to_yuan_string(),
            quarter_net_income: reports.income.quarter.net_income.to_yuan_string(),
            net_income: reports.income.cumulative.net_income.to_yuan_string(),
            income_tax: reports.income.cumulative.income_tax.to_yuan_string(),
            operating_cash_flow: reports.cash_flow.operating.to_yuan_string(),
            investing_cash_flow: reports.cash_flow.investing.to_yuan_string(),
            financing_cash_flow: reports.cash_flow.financing.to_yuan_string(),
            net_cash_change: reports.cash_flow.net_change.to_yuan_string(),
            prior_year_net_income: match &reports.income.prior_year {
                Comparative::Available(columns) => PublicComparativeAmount::Available {
                    amount: columns.net_income.to_yuan_string(),
                },
                Comparative::Unavailable { reason } => PublicComparativeAmount::Unavailable {
                    reason: match reason {
                        UnavailableReason::NoPriorYearHistory => {
                            PublicUnavailableReason::NoPriorYearHistory
                        }
                    },
                },
            },
        };
        Self {
            id: report.id.value().to_string(),
            company_id: report.company.0.clone(),
            source: report.source,
            period: period_end_date(reports.period)
                .expect("a published report always has a valid period end")
                .to_iso(),
            kind: match reports.kind {
                crate::accounting::reports::ReportKind::Monthly => PublicReportKind::Monthly,
                crate::accounting::reports::ReportKind::Quarter => PublicReportKind::Quarter,
                crate::accounting::reports::ReportKind::HalfYear => PublicReportKind::HalfYear,
                crate::accounting::reports::ReportKind::Annual => PublicReportKind::Annual,
            },
            version_sequence: reports.version.sequence.to_string(),
            supersedes: report.supersedes.map(|id| id.value().to_string()),
            approved_date: report.approved_at.date().to_iso(),
            approved_second_of_day: report.approved_at.second_of_day(),
            published_date: report.published_at.date().to_iso(),
            published_second_of_day: report.published_at.second_of_day(),
            accounting,
            financials: PublicReportFinancials::from(reports),
        }
    }
}
