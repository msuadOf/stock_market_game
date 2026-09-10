//! 利润表生成器（K3，任务 13）：损益五分类（经营/投资/筹资/所得税费用/
//! 终止经营——CAS 30（2026）§32–38 已核验）+ 费用功能法列示（§33/34）。
//!
//! 双栏口径：**当季**（季度首月..=期末月）+ **累计**（年初至今）。行业行
//! 结构由归类表决定：银行（利息净收入/手续费）、保险（服务业绩/财务损益）
//! 的计算行由组件行推导。上年同期为类型化比较项（缺历史 `Unavailable`，
//! 绝不填零）。合并 Scope 追加少数/归母净利拆分（任务 12 输出）。

use std::collections::BTreeMap;

use crate::accounting::amount::AccountingAmount;
use crate::accounting::ledger::LedgerAccountId;

use super::notes::NoteTarget;
use super::window::StatementWindows;
use super::{Comparative, ReportError, UnavailableReason};

/// 利润表列示行。
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Debug, serde::Serialize, serde::Deserialize,
)]
pub enum IncomeLine {
    OperatingRevenue,
    OperatingCost,
    SellingExpense,
    AdministrativeExpense,
    ResearchExpense,
    ImpairmentLoss,
    InterestIncome,
    InterestExpense,
    NetInterestIncome,
    FeeAndCommissionIncome,
    InsuranceRevenue,
    InsuranceServiceExpense,
    InsuranceServiceResult,
    InsuranceFinanceExpense,
    FinanceExpense,
    IncomeTaxExpense,
}

/// 损益五分类（所得税单列；投资/终止经营暂无对应科目，结构性留空——
/// 无科目即无行，不是填零）。
#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub enum IncomeClass {
    Operating,
    Investing,
    Financing,
    Discontinued,
}

impl IncomeLine {
    /// 列示顺序。
    pub const ALL: &'static [IncomeLine] = &[
        IncomeLine::OperatingRevenue,
        IncomeLine::OperatingCost,
        IncomeLine::SellingExpense,
        IncomeLine::AdministrativeExpense,
        IncomeLine::ResearchExpense,
        IncomeLine::ImpairmentLoss,
        IncomeLine::InterestIncome,
        IncomeLine::InterestExpense,
        IncomeLine::NetInterestIncome,
        IncomeLine::FeeAndCommissionIncome,
        IncomeLine::InsuranceRevenue,
        IncomeLine::InsuranceServiceExpense,
        IncomeLine::InsuranceServiceResult,
        IncomeLine::InsuranceFinanceExpense,
        IncomeLine::FinanceExpense,
        IncomeLine::IncomeTaxExpense,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            IncomeLine::OperatingRevenue => "营业收入",
            IncomeLine::OperatingCost => "营业成本",
            IncomeLine::SellingExpense => "销售费用",
            IncomeLine::AdministrativeExpense => "管理费用",
            IncomeLine::ResearchExpense => "研发费用",
            IncomeLine::ImpairmentLoss => "减值损失",
            IncomeLine::InterestIncome => "利息收入",
            IncomeLine::InterestExpense => "利息支出",
            IncomeLine::NetInterestIncome => "利息净收入",
            IncomeLine::FeeAndCommissionIncome => "手续费及佣金净收入",
            IncomeLine::InsuranceRevenue => "保险服务收入",
            IncomeLine::InsuranceServiceExpense => "保险服务费用",
            IncomeLine::InsuranceServiceResult => "保险服务业绩",
            IncomeLine::InsuranceFinanceExpense => "保险财务损益",
            IncomeLine::FinanceExpense => "财务费用",
            IncomeLine::IncomeTaxExpense => "所得税费用",
        }
    }

    /// 收入类行（贷方正常，正值呈现贷方发生）。
    pub fn credit_positive(&self) -> bool {
        use IncomeLine::*;
        matches!(
            self,
            OperatingRevenue
                | InterestIncome
                | NetInterestIncome
                | FeeAndCommissionIncome
                | InsuranceRevenue
                | InsuranceServiceResult
        )
    }

    /// 计算行（组件推导，不参与分类小计）。
    pub fn is_computed(&self) -> bool {
        matches!(
            self,
            IncomeLine::NetInterestIncome | IncomeLine::InsuranceServiceResult
        )
    }

    pub fn class(&self) -> IncomeClass {
        match self {
            IncomeLine::FinanceExpense => IncomeClass::Financing,
            _ => IncomeClass::Operating,
        }
    }
}

/// 单栏（当季或累计）五分类列。
#[derive(Clone, Eq, PartialEq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct IncomeColumns {
    pub operating: Vec<(IncomeLine, AccountingAmount)>,
    pub operating_subtotal: AccountingAmount,
    pub investing: Vec<(IncomeLine, AccountingAmount)>,
    pub investing_subtotal: AccountingAmount,
    pub financing: Vec<(IncomeLine, AccountingAmount)>,
    pub financing_subtotal: AccountingAmount,
    pub discontinued: Vec<(IncomeLine, AccountingAmount)>,
    pub discontinued_subtotal: AccountingAmount,
    pub income_tax: AccountingAmount,
    pub net_income: AccountingAmount,
}

type Classification = BTreeMap<LedgerAccountId, NoteTarget>;

fn has_line(classification: &Classification, line: IncomeLine) -> bool {
    classification
        .values()
        .any(|target| matches!(target, NoteTarget::Income(l) if *l == line))
}

fn line_value(
    map: &BTreeMap<LedgerAccountId, AccountingAmount>,
    classification: &Classification,
    line: IncomeLine,
) -> Result<AccountingAmount, ReportError> {
    let mut total = AccountingAmount::ZERO;
    for (code, target) in classification {
        if matches!(target, NoteTarget::Income(l) if *l == line) {
            let value = map.get(code).copied().unwrap_or(AccountingAmount::ZERO);
            let signed = if line.credit_positive() {
                value.neg()?
            } else {
                value
            };
            total = total.add(signed)?;
        }
    }
    Ok(total)
}

fn columns(
    map: &BTreeMap<LedgerAccountId, AccountingAmount>,
    classification: &Classification,
) -> Result<IncomeColumns, ReportError> {
    let mut columns = IncomeColumns::default();
    let mut values: BTreeMap<IncomeLine, AccountingAmount> = BTreeMap::new();
    for line in IncomeLine::ALL {
        if *line == IncomeLine::IncomeTaxExpense {
            continue;
        }
        // 计算行随组件行出现（无组件科目即无行——不是填零）。
        let used = match line {
            IncomeLine::NetInterestIncome => {
                has_line(classification, IncomeLine::InterestIncome)
                    || has_line(classification, IncomeLine::InterestExpense)
            }
            IncomeLine::InsuranceServiceResult => {
                has_line(classification, IncomeLine::InsuranceRevenue)
                    || has_line(classification, IncomeLine::InsuranceServiceExpense)
            }
            other => has_line(classification, *other),
        };
        if !used {
            continue;
        }
        let value = if *line == IncomeLine::NetInterestIncome {
            line_value(map, classification, IncomeLine::InterestIncome)?.sub(line_value(
                map,
                classification,
                IncomeLine::InterestExpense,
            )?)?
        } else if *line == IncomeLine::InsuranceServiceResult {
            line_value(map, classification, IncomeLine::InsuranceRevenue)?.sub(line_value(
                map,
                classification,
                IncomeLine::InsuranceServiceExpense,
            )?)?
        } else {
            line_value(map, classification, *line)?
        };
        values.insert(*line, value);
    }
    for (line, value) in &values {
        let vec = match line.class() {
            IncomeClass::Operating => &mut columns.operating,
            IncomeClass::Investing => &mut columns.investing,
            IncomeClass::Financing => &mut columns.financing,
            IncomeClass::Discontinued => &mut columns.discontinued,
        };
        vec.push((*line, *value));
        if !line.is_computed() {
            let subtotal = match line.class() {
                IncomeClass::Operating => &mut columns.operating_subtotal,
                IncomeClass::Investing => &mut columns.investing_subtotal,
                IncomeClass::Financing => &mut columns.financing_subtotal,
                IncomeClass::Discontinued => &mut columns.discontinued_subtotal,
            };
            let contribution = if line.credit_positive() {
                *value
            } else {
                value.neg()?
            };
            *subtotal = subtotal.add(contribution)?;
        }
    }
    columns.income_tax = line_value(map, classification, IncomeLine::IncomeTaxExpense)?;
    columns.net_income = columns
        .operating_subtotal
        .add(columns.investing_subtotal)?
        .add(columns.financing_subtotal)?
        .add(columns.discontinued_subtotal)?
        .sub(columns.income_tax)?;
    Ok(columns)
}

/// 利润表（当季 + 累计 + 上年同期 + 合并拆分）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct IncomeStatement {
    pub quarter: IncomeColumns,
    pub cumulative: IncomeColumns,
    pub prior_year: Comparative<IncomeColumns>,
    pub minority_net_income: Option<AccountingAmount>,
    pub net_income_to_parent: Option<AccountingAmount>,
}

impl IncomeColumns {
    /// 按行取值（跨四类向量 + 所得税标量）。
    pub fn line_amount(&self, line: IncomeLine) -> Option<AccountingAmount> {
        if line == IncomeLine::IncomeTaxExpense {
            return Some(self.income_tax);
        }
        self.operating
            .iter()
            .chain(self.investing.iter())
            .chain(self.financing.iter())
            .chain(self.discontinued.iter())
            .find(|(l, _)| *l == line)
            .map(|(_, v)| *v)
    }
}

/// 生成利润表。
pub(crate) fn generate(
    windows: &StatementWindows,
    classification: &Classification,
) -> Result<IncomeStatement, ReportError> {
    let facts = windows.consolidation.as_ref();
    let quarter = columns(&windows.quarter, classification)?;
    let cumulative = columns(&windows.ytd, classification)?;
    let prior_year = match &windows.prior_year {
        None => Comparative::Unavailable {
            reason: UnavailableReason::NoPriorYearHistory,
        },
        Some(prior) => Comparative::Available(columns(prior, classification)?),
    };
    if let Some(facts) = facts {
        if cumulative.net_income != facts.consolidated_ni {
            return Err(ReportError::IncomeConsistencyMismatch {
                derived: cumulative.net_income,
                output: facts.consolidated_ni,
            });
        }
    }
    Ok(IncomeStatement {
        quarter,
        cumulative,
        prior_year,
        minority_net_income: facts.map(|facts| facts.minority_ni),
        net_income_to_parent: facts.map(|facts| facts.ni_to_parent),
    })
}
