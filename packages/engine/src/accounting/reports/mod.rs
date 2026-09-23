//! 行业报表层（K3，任务 9–13）。
//!
//! - 任务 9–11：行业**列报分类层**（`bank`/`insurance`/`real_estate` +
//!   任务 13 的 `industrial`）——全期间读投影，科目代码单一真源。
//! - 任务 13：**完整报表生成器**（`balance_sheet`/`income`/`cash_flow`/
//!   `equity`/`notes` + `window`/`consolidated_window` 窗口底座）——五产物
//!   `ReportSet`（四张基本表 + 附注），由分录纯函数推导，带 ScopeId
//!   （单体/合并）、期间与不可变版本标记；结账/更正在 [`super::closing`]。
//!
//! 依赖方向铁律：accounting 不得 import company；行业科目代码真源放本层，
//! company 侧科目表构造器引用它。

pub mod bank;
pub mod insurance;
pub mod real_estate;

pub mod balance_sheet;
pub mod cash_flow;
pub mod equity;
pub mod income;
pub mod industrial;
pub mod notes;

pub(crate) mod consolidated_window;
mod error;
pub(crate) mod window;

pub mod validate;

pub use balance_sheet::{BalanceSheet, BsLine};
pub use cash_flow::{CashFlowStatement, IndirectLine};
pub use equity::EquityStatement;
pub use error::ReportError;
pub use income::{IncomeClass, IncomeColumns, IncomeLine, IncomeStatement};
pub use notes::{Assignment, NoteItem, NoteTarget, Notes};

use std::collections::{BTreeMap, BTreeSet};

use crate::accounting::consolidation::{ConsolidationRequest, MemberId, ScopeId};
use crate::accounting::journal::BusinessEventId;
use crate::accounting::ledger::{AccountChart, TrialBalanceSummary};
use crate::accounting::period::AccountingPeriod;

/// 行业列报口径（与科目表版本对应：v1/v2 工业、v3 银行、v4 保险、v5 地产）。
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Debug, serde::Serialize, serde::Deserialize,
)]
pub enum IndustryPresentation {
    Industrial,
    Bank,
    Insurance,
    RealEstate,
}

/// 报告种类（决定窗口形状；期间试算→报表→勾稽→封账见 closing.rs）。
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Debug, serde::Serialize, serde::Deserialize,
)]
pub enum ReportKind {
    /// 月报（窗口 = 单月）。
    Monthly,
    /// 季报快照（窗口 = 季度首月..=期末月）。
    Quarter,
    /// 半年报（窗口 = 1..=6 月；仅可落在 6 月）。
    HalfYear,
    /// 年报（窗口 = 1..=12 月；仅可落在 12 月）。
    Annual,
}

/// 窗口解析结果：（报告窗口, 上年同期窗口）。
pub type ResolvedWindows = (
    (AccountingPeriod, AccountingPeriod),
    (AccountingPeriod, AccountingPeriod),
);

impl ReportKind {
    /// 解析（报告窗口, 上年同期窗口）。
    pub fn resolve(&self, period: AccountingPeriod) -> Result<ResolvedWindows, ReportError> {
        let jan = |year: i32| {
            AccountingPeriod::from_ymd(year, 1).map_err(|_| ReportError::InvalidReportKind {
                period,
                reason: "report year outside the supported civil window",
            })
        };
        let first = match self {
            ReportKind::Monthly => period,
            ReportKind::Quarter => window::quarter_first(period),
            ReportKind::HalfYear if period.month() == 6 => jan(period.year())?,
            ReportKind::Annual if period.month() == 12 => jan(period.year())?,
            ReportKind::HalfYear | ReportKind::Annual => {
                return Err(ReportError::InvalidReportKind {
                    period,
                    reason: "half-year must land on June; annual must land on December",
                })
            }
        };
        let shift = |p: AccountingPeriod| {
            AccountingPeriod::from_ymd(p.year() - 1, p.month()).map_err(|_| {
                ReportError::InvalidReportKind {
                    period,
                    reason: "prior year precedes the supported civil window",
                }
            })
        };
        Ok(((first, period), (shift(first)?, shift(period)?)))
    }
}

/// 比较项缺历史的类型化理由（缺原因在类型层不可表示）。
#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub enum UnavailableReason {
    NoPriorYearHistory,
}

/// 比较项：可得（真值）或类型化不可得（绝不填零）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Comparative<T> {
    Available(T),
    Unavailable { reason: UnavailableReason },
}

/// 版本成因。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub enum VersionKind {
    Original,
    Correction { reason: String },
}

/// 期间版本标记（不可变；更正 = 新版本经 `supersedes` 链接前版）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct ReportVersion {
    pub sequence: u32,
    pub supersedes: Option<u32>,
    pub kind: VersionKind,
}

/// 报表来源：单体账套或固定集团合并请求。
pub enum ReportSource<'a> {
    Standalone {
        id: MemberId,
        books: &'a crate::accounting::Books,
        industry: IndustryPresentation,
    },
    Consolidated {
        request: ConsolidationRequest<'a>,
    },
}

/// 生成请求（`adjustments` = 重述映射：调整分录来源 → 目标历史期间）。
pub struct ReportRequest<'a> {
    pub period: AccountingPeriod,
    pub kind: ReportKind,
    pub source: ReportSource<'a>,
    pub version: ReportVersion,
    pub adjustments: &'a BTreeMap<BusinessEventId, AccountingPeriod>,
}

/// 五产物：四张基本表 + 附注。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct ReportSet {
    pub scope: ScopeId,
    pub period: AccountingPeriod,
    pub kind: ReportKind,
    pub window: (AccountingPeriod, AccountingPeriod),
    pub version: ReportVersion,
    pub balance_sheet: BalanceSheet,
    pub income: IncomeStatement,
    pub cash_flow: CashFlowStatement,
    pub equity: EquityStatement,
    pub notes: Notes,
}

/// 科目表版本 → 行业列报口径。
fn industry_of_chart(chart: &AccountChart) -> Result<IndustryPresentation, ReportError> {
    match chart.version() {
        1 | 2 => Ok(IndustryPresentation::Industrial),
        3 => Ok(IndustryPresentation::Bank),
        4 => Ok(IndustryPresentation::Insurance),
        5 => Ok(IndustryPresentation::RealEstate),
        version => Err(ReportError::InternalWindowInconsistent {
            detail: format!("unknown chart version {version}"),
        }),
    }
}

/// 生成五产物（纯函数：同输入 ⇒ 同 ReportSet）。
pub fn generate_report_set(request: ReportRequest<'_>) -> Result<ReportSet, ReportError> {
    let ((first, last), prior_window) = request.kind.resolve(request.period)?;
    let restatement = !request.adjustments.is_empty();
    let (scope, windows, classification) = match request.source {
        ReportSource::Standalone {
            id,
            books,
            industry,
        } => {
            let windows =
                window::standalone(books, (first, last), prior_window, request.adjustments)?;
            let classification = notes::classification(&windows.defs, &[industry])?;
            (ScopeId::Standalone(id), windows, classification)
        }
        ReportSource::Consolidated {
            request: group_request,
        } => {
            if restatement {
                return Err(ReportError::ConsolidatedRestatementUnsupported);
            }
            let mut industries: BTreeSet<IndustryPresentation> = BTreeSet::new();
            for member in &group_request.members {
                industries.insert(industry_of_chart(member.books.ledger().chart())?);
            }
            let windows =
                consolidated_window::consolidated(group_request, (first, last), prior_window)?;
            let classification =
                notes::classification(&windows.defs, &industries.into_iter().collect::<Vec<_>>())?;
            let root = windows
                .consolidation
                .as_ref()
                .map(|facts| facts.root.clone())
                .ok_or_else(|| ReportError::InternalWindowInconsistent {
                    detail: "consolidated windows missing facts".to_string(),
                })?;
            (ScopeId::Consolidated(root), windows, classification)
        }
    };
    let balance_sheet = balance_sheet::generate(&windows, &classification)?;
    let income = income::generate(&windows, &classification)?;
    let cash_flow = cash_flow::generate(&windows)?;
    let equity = equity::generate(&windows)?;
    let notes = notes::build_notes(&windows, &classification)?;
    Ok(ReportSet {
        scope,
        period: request.period,
        kind: request.kind,
        window: (first, last),
        version: request.version,
        balance_sheet,
        income,
        cash_flow,
        equity,
        notes,
    })
}

/// 试算平衡守卫（不平 = 不允许公布）。
pub fn validate_trial_balance(summary: &TrialBalanceSummary) -> Result<(), ReportError> {
    if summary.total_debits != summary.total_credits {
        return Err(ReportError::TrialBalanceUnbalanced {
            total_debits: summary.total_debits,
            total_credits: summary.total_credits,
        });
    }
    Ok(())
}
