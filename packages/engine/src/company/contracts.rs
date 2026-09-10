//! 经营合同类型面 + 经营预算/授信（K2）。
//!
//! 本任务只落**纯数据 + 验证**：合同（借款/应付/应收）携带本金、固定年利率
//! （整数基点）、起止日与计息基准（ACT/365F）；利息计提与入账业务在任务
//! 8–11/14。新增借款必须落在授信额度内——无授信或超授信都是类型化拒绝，
//! 不允许无限信用兜底（K2：借款必须经额度/需求约束）。

use std::collections::BTreeMap;

use crate::accounting::AccountingAmount;
use crate::calendar::CivilDate;
use crate::company::counterparty::CounterpartyId;
use crate::company::error::CompanyError;

/// 合同稳定 id newtype。
#[derive(
    Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, serde::Deserialize,
)]
pub struct ContractId(pub String);

/// 计息日计数基准（K1：合同用实际自然日 + 约定 basis；默认虚构合同 ACT/365F、
/// 固定利率，不宣称所有真实合同如此。更多基准随任务 8–11 的需要扩充枚举）。
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, serde::Deserialize,
)]
pub enum DayCountBasis {
    /// ACT/365F：实际天数 / 固定 365 分母。
    Act365F,
}

/// 合同角色（公司视角；只有借款消耗授信）。
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, serde::Deserialize,
)]
pub enum ContractRole {
    /// 借款（公司作为借款人；占用贷款人授信）。
    Borrowing,
    /// 应付（采购/服务等经营应付款；不占授信）。
    Payable,
    /// 应收（销售/服务应收款）。
    Receivable,
}

/// 经营合同（纯数据；业务过账在任务 8–11/14）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct OperatingContract {
    pub id: ContractId,
    pub role: ContractRole,
    pub counterparty: CounterpartyId,
    pub principal: AccountingAmount,
    /// 固定年利率（整数基点，≥0）。
    pub annual_rate_bp: i32,
    pub start_date: CivilDate,
    pub maturity_date: CivilDate,
    pub basis: DayCountBasis,
}

impl OperatingContract {
    /// 数据验证：正本金、非负利率、到期日晚于起息日。
    pub fn validate(&self) -> Result<(), CompanyError> {
        if !self.principal.is_positive() {
            return Err(CompanyError::NonPositivePrincipal {
                contract: self.id.clone(),
                principal: self.principal,
            });
        }
        if self.annual_rate_bp < 0 {
            return Err(CompanyError::NegativeRate {
                contract: self.id.clone(),
                rate_bp: self.annual_rate_bp,
            });
        }
        if self.maturity_date <= self.start_date {
            return Err(CompanyError::MaturityNotAfterStart {
                contract: self.id.clone(),
                start: self.start_date,
                maturity: self.maturity_date,
            });
        }
        Ok(())
    }
}

/// 合同登记簿。公开入口是 [`crate::company::Company::register_contract`]
/// （叠加对手方与授信检查）；这里只保证 id 唯一与数据形状合法。
#[derive(Clone, Eq, PartialEq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ContractBook {
    contracts: BTreeMap<ContractId, OperatingContract>,
}

impl ContractBook {
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记（先验证 + 查重；授信检查由 Company 入口执行）。
    pub(crate) fn register(&mut self, contract: OperatingContract) -> Result<(), CompanyError> {
        contract.validate()?;
        if self.contracts.contains_key(&contract.id) {
            return Err(CompanyError::DuplicateContract {
                contract: contract.id,
            });
        }
        let id = contract.id.clone();
        self.contracts.insert(id, contract);
        Ok(())
    }

    pub fn get(&self, id: &ContractId) -> Option<&OperatingContract> {
        self.contracts.get(id)
    }

    pub fn len(&self) -> usize {
        self.contracts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.contracts.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&ContractId, &OperatingContract)> {
        self.contracts.iter()
    }

    /// 对某贷款人的未偿借款本金合计（授信占用；还款核销在任务 8–11）。
    pub fn outstanding_borrowings(
        &self,
        lender: &CounterpartyId,
    ) -> Result<AccountingAmount, CompanyError> {
        let mut total = AccountingAmount::ZERO;
        for contract in self.contracts.values() {
            if contract.role == ContractRole::Borrowing && &contract.counterparty == lender {
                total = total.add(contract.principal)?;
            }
        }
        Ok(total)
    }
}

/// 单个授信额度（贷款人 → 上限）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreditLine {
    pub lender: CounterpartyId,
    pub limit: AccountingAmount,
}

/// 经营预算（K2：现金下限 + 授信额度集合；借款必须经额度约束）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct OperatingBudget {
    operating_cash_floor: AccountingAmount,
    credit_lines: BTreeMap<CounterpartyId, AccountingAmount>,
}

impl OperatingBudget {
    /// 构造：负现金下限、非正/重复授信 → 类型化拒绝。
    pub fn new(
        operating_cash_floor: AccountingAmount,
        credit_lines: Vec<CreditLine>,
    ) -> Result<Self, CompanyError> {
        if operating_cash_floor.is_negative() {
            return Err(CompanyError::BudgetInvalid {
                detail: "negative operating cash floor".to_string(),
            });
        }
        let mut lines = BTreeMap::new();
        for line in credit_lines {
            if !line.limit.is_positive() {
                return Err(CompanyError::CreditLineInvalid {
                    lender: line.lender,
                    limit: line.limit,
                    reason: "credit limit must be positive",
                });
            }
            if lines.insert(line.lender.clone(), line.limit).is_some() {
                return Err(CompanyError::DuplicateCreditLine {
                    lender: line.lender,
                });
            }
        }
        Ok(Self {
            operating_cash_floor,
            credit_lines: lines,
        })
    }

    /// 经营现金下限（资金不足生成 PaymentFailed/Overdue 业务状态属任务 8–11）。
    pub fn operating_cash_floor(&self) -> AccountingAmount {
        self.operating_cash_floor
    }

    /// 某贷款人的授信上限（无授信 = None）。
    pub fn credit_line(&self, lender: &CounterpartyId) -> Option<AccountingAmount> {
        self.credit_lines.get(lender).copied()
    }
}
