//! 公司现金分红的共同契约类型。
//!
//! `SimpleFundamentals` 与后续 `CompanySimulation` 都使用这些类型表达法定事实、
//! 方案审批与实际付款事实；模式私有的账面更新仍留在各自实现内。

use crate::accounting::{AccountingAmount, BusinessEventId};
use crate::calendar::CivilDate;

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DividendDeclaration {
    pub plan_id: String,
    pub approved_on: CivilDate,
    pub total_gross: AccountingAmount,
    pub registered_capital: AccountingAmount,
}

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DividendLegalFacts {
    pub registered_capital: AccountingAmount,
    pub source_evidence: String,
}

#[derive(Clone, Copy, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DistributableProfit {
    pub accumulated_after_loss: AccountingAmount,
    pub statutory_reserve: AccountingAmount,
    pub available_for_distribution: AccountingAmount,
    pub reserve_basis_year: Option<i32>,
}

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DividendPlanReceipt {
    pub plan_id: String,
    pub amount: AccountingAmount,
    pub statutory_reserve: AccountingAmount,
    pub already_declared: bool,
}

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DividendPaymentReceipt {
    pub plan_id: String,
    pub payment_id: String,
    pub amount: AccountingAmount,
    pub paid_on: CivilDate,
    pub already_paid: bool,
    pub simple_display_only: bool,
    pub within_six_month_deadline: bool,
}

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DividendPaymentFact {
    pub payment_id: String,
    pub paid_on: CivilDate,
    pub amount: AccountingAmount,
    pub source: BusinessEventId,
}

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DividendPlanFact {
    pub plan_id: String,
    pub approved_on: CivilDate,
    pub total_gross: AccountingAmount,
    pub registered_capital: AccountingAmount,
    pub registered_capital_source_evidence: String,
    pub declaration_source: BusinessEventId,
    pub payments: Vec<DividendPaymentFact>,
}
