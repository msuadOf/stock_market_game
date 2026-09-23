//! Host-facing public company-report DTOs.
//!
//! These types intentionally project immutable published reports only. Company books,
//! operating state, NPC information, beliefs, and other private authority never cross
//! this boundary.

use crate::accounting::reports::{Comparative, UnavailableReason};
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

#[derive(Clone, Debug, serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct PublicReportPage {
    pub reports: Vec<PublicReportSummary>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct PublicReportSummary {
    /// Opaque decimal publication ID. It is the only pagination order key.
    pub id: String,
    pub company_id: String,
    pub period: String,
    pub kind: PublicReportKind,
    pub version_sequence: String,
    pub supersedes: Option<String>,
    pub approved_date: String,
    pub approved_second_of_day: u32,
    pub published_date: String,
    pub published_second_of_day: u32,
    pub accounting: PublicReportAccountingSummary,
}

#[derive(Clone, Copy, Debug, serde::Serialize, ts_rs::TS)]
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
    /// All accounting values are exact decimal yuan strings, never JavaScript numbers.
    pub total_assets: String,
    pub total_liabilities: String,
    pub total_equity: String,
    pub closing_cash: String,
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

impl From<&PublishedReport> for PublicReportSummary {
    fn from(report: &PublishedReport) -> Self {
        let reports = &report.reports;
        let accounting = PublicReportAccountingSummary {
            total_assets: reports.balance_sheet.total_assets.to_yuan_string(),
            total_liabilities: reports.balance_sheet.total_liabilities.to_yuan_string(),
            total_equity: reports.balance_sheet.total_equity.to_yuan_string(),
            closing_cash: reports.balance_sheet.closing_cash.to_yuan_string(),
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
        }
    }
}
