//! 年报事实抽取（任务 18）：从本人已获知的年报提取三种估值方法所需的
//! **归母口径**事实。
//!
//! 口径纪律（K5a 行 152）：
//! - 一致报告范围——合并范围剔除少数股东：权益取 `equity_to_parent`、
//!   净利取 `net_income_to_parent`（单体缺省即累计净利），绝不把集团总量
//!   混作归母；
//! - 货币单位统一为分（`AccountingAmount` 即分；每股换算同单位，不存在
//!   元/分混算）；
//! - 借款净运动从附注短期/长期借款行读取（利息不经借款科目，见 valuation
//!   模块的 FCFE 口径推导）。

use crate::accounting::AccountingAmount;
use crate::accounting::reports::{BsLine, Comparative, IncomeLine, NoteTarget, ReportKind};
use crate::calendar::CivilInstant;
use crate::company::CompanyId;
use crate::information::{PublicationId, PublishedReport};

use super::ValuationUnavailable;

/// 上年可比收入：比较项可得（可能为 0——增长观察层显式退化处理），或
/// 缺历史（`Unavailable(reason)`，绝不填零）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PriorRevenue {
    Comparative(AccountingAmount),
    NoHistory,
}

/// 年报事实快照（归母口径；全部只读）。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AnnualFacts {
    pub report_id: PublicationId,
    pub published_at: CivilInstant,
    /// 年度累计营业收入（贷方正常，正值）。
    pub revenue: AccountingAmount,
    pub prior_revenue: PriorRevenue,
    /// 归母净利（合并拆分优先，单体 = 累计净利）。
    pub net_income_to_parent: AccountingAmount,
    /// 期末归母权益。
    pub equity_to_parent: AccountingAmount,
    /// 期初归母权益（权益变动表）。
    pub opening_equity_to_parent: AccountingAmount,
    pub operating_cf: AccountingAmount,
    pub investing_cf: AccountingAmount,
    pub financing_cf: AccountingAmount,
    /// 借款行（短期/长期借款）窗口运动折为贷方正常（新借 − 还本）。
    pub net_new_borrowings: AccountingAmount,
}

/// 抽取年报事实。守卫顺序：公司归属 → 报告种类（仅年报）→ 未来信息
/// （`published_at > as_of` ⇒ 类型化拒绝，即使直接传报表也不许前视）。
pub fn extract_annual_facts(
    report: &PublishedReport,
    expected_company: &CompanyId,
    as_of: CivilInstant,
) -> Result<AnnualFacts, ValuationUnavailable> {
    if report.company != *expected_company {
        return Err(ValuationUnavailable::CompanyMismatch {
            expected: expected_company.clone(),
            report: report.company.clone(),
        });
    }
    if report.reports.kind != ReportKind::Annual {
        return Err(ValuationUnavailable::UnsupportedReportKind {
            kind: report.reports.kind,
        });
    }
    if report.published_at > as_of {
        return Err(ValuationUnavailable::FutureDatedMaterial { report: report.id });
    }

    let income = &report.reports.income;
    let revenue = income
        .cumulative
        .line_amount(IncomeLine::OperatingRevenue)
        .unwrap_or(AccountingAmount::ZERO);
    let prior_revenue = match &income.prior_year {
        Comparative::Available(columns) => PriorRevenue::Comparative(
            columns
                .line_amount(IncomeLine::OperatingRevenue)
                .unwrap_or(AccountingAmount::ZERO),
        ),
        Comparative::Unavailable { .. } => PriorRevenue::NoHistory,
    };
    let net_income_to_parent = income
        .net_income_to_parent
        .unwrap_or(income.cumulative.net_income);
    let cash_flow = &report.reports.cash_flow;

    // 借款净运动：附注借款行 movement 求和后取负（movement 是净借方）。
    let overflow = || ValuationUnavailable::Overflow {
        step: "borrowing-line movement aggregation".into(),
    };
    let mut borrowing_movement = AccountingAmount::ZERO;
    for item in &report.reports.notes.items {
        if matches!(
            item.target,
            NoteTarget::BalanceSheet(BsLine::ShortTermBorrowings | BsLine::LongTermBorrowings)
        ) {
            borrowing_movement = borrowing_movement
                .add(item.movement)
                .map_err(|_| overflow())?;
        }
    }
    let net_new_borrowings = borrowing_movement.neg().map_err(|_| overflow())?;

    Ok(AnnualFacts {
        report_id: report.id,
        published_at: report.published_at,
        revenue,
        prior_revenue,
        net_income_to_parent,
        equity_to_parent: report.reports.balance_sheet.equity_to_parent,
        opening_equity_to_parent: report.reports.equity.opening_parent,
        operating_cf: cash_flow.operating,
        investing_cf: cash_flow.investing,
        financing_cf: cash_flow.financing,
        net_new_borrowings,
    })
}
