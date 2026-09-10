//! 外部商业对手方（K2）：客户/供应商/雇员/税务机关/贷款人的稳定 id 与收付记录。
//!
//! 对手方是公司域商业主体，**不是证券 NPC**：`CounterpartyId` 与交易域
//! `AccountId` 是独立命名空间（编译期不可混用）。收付记录如实登记跨模拟
//! 边界的资金流；授信约束在 [`crate::company::contracts`]（不给对手方
//! 无限信用兜底）。

use std::collections::BTreeMap;

use crate::accounting::AccountingAmount;
use crate::calendar::CivilDate;
use crate::company::error::CompanyError;

/// 外部对手方稳定 id newtype（独立于交易域 `AccountId`）。
#[derive(
    Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, serde::Deserialize,
)]
pub struct CounterpartyId(pub String);

/// 对手方角色（K2 五类外部商业主体）。
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, serde::Deserialize,
)]
pub enum CounterpartyKind {
    /// 客户（销售收款的对手方）。
    Customer,
    /// 供应商（采购付款的对手方）。
    Supplier,
    /// 雇员（薪酬收付的对手方）。
    Employee,
    /// 税务机关（税费收付的对手方）。
    TaxAuthority,
    /// 贷款人（借款/还款/利息的对手方）。
    Lender,
}

/// 外部对手方主体。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct ExternalCounterparty {
    pub id: CounterpartyId,
    pub kind: CounterpartyKind,
    pub name: String,
}

/// 跨边界资金流方向（公司视角）。
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, serde::Deserialize,
)]
pub enum FlowDirection {
    /// 公司收款（资金流入）。
    Inbound,
    /// 公司付款（资金流出）。
    Outbound,
}

/// 一笔已发生的跨边界收付记录（金额恒正，方向由收支表示）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct CounterpartyFlow {
    pub date: CivilDate,
    pub counterparty: CounterpartyId,
    pub direction: FlowDirection,
    pub amount: AccountingAmount,
    pub memo: String,
}

/// 对手方登记簿：主体 + 收付流水 + 每对手方净头寸（Σ收 − Σ付）。
#[derive(Clone, Eq, PartialEq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct CounterpartyLedger {
    counterparties: BTreeMap<CounterpartyId, ExternalCounterparty>,
    flows: Vec<CounterpartyFlow>,
}

impl CounterpartyLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记主体：空 id/名称、重复 id → 类型化拒绝。
    pub fn register(&mut self, counterparty: ExternalCounterparty) -> Result<(), CompanyError> {
        if counterparty.id.0.trim().is_empty() || counterparty.name.trim().is_empty() {
            return Err(CompanyError::CounterpartyInvalid {
                detail: format!("empty id or name for counterparty {:?}", counterparty.id),
            });
        }
        if self.counterparties.contains_key(&counterparty.id) {
            return Err(CompanyError::DuplicateCounterparty {
                counterparty: counterparty.id,
            });
        }
        let id = counterparty.id.clone();
        self.counterparties.insert(id, counterparty);
        Ok(())
    }

    pub fn get(&self, id: &CounterpartyId) -> Option<&ExternalCounterparty> {
        self.counterparties.get(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&CounterpartyId, &ExternalCounterparty)> {
        self.counterparties.iter()
    }

    /// 记一笔跨边界收付：对手方必须已登记、金额必须为正。
    pub fn record_flow(&mut self, flow: CounterpartyFlow) -> Result<(), CompanyError> {
        if !self.counterparties.contains_key(&flow.counterparty) {
            return Err(CompanyError::UnknownCounterparty {
                counterparty: flow.counterparty,
            });
        }
        if !flow.amount.is_positive() {
            return Err(CompanyError::FlowAmountNotPositive {
                counterparty: flow.counterparty,
                amount: flow.amount,
            });
        }
        self.flows.push(flow);
        Ok(())
    }

    /// 对手方净头寸（Σ收款 − Σ付款；可为负 = 公司净付出未收回，如实呈现）。
    pub fn net_position(&self, id: &CounterpartyId) -> Result<AccountingAmount, CompanyError> {
        if !self.counterparties.contains_key(id) {
            return Err(CompanyError::UnknownCounterparty {
                counterparty: id.clone(),
            });
        }
        let mut net = AccountingAmount::ZERO;
        for flow in &self.flows {
            if &flow.counterparty == id {
                net = match flow.direction {
                    FlowDirection::Inbound => net.add(flow.amount)?,
                    FlowDirection::Outbound => net.sub(flow.amount)?,
                };
            }
        }
        Ok(net)
    }

    pub fn flow_count(&self) -> usize {
        self.flows.len()
    }

    pub fn flows(&self) -> &[CounterpartyFlow] {
        &self.flows
    }
}
