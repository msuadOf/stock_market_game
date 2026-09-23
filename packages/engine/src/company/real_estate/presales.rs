//! 预售处理器（K3 地产）：签约（纯子账事实，不过账）与收款
//! （Dr 现金 / Cr 2203 合同负债——**不是收入**；CAS 14 §39 已核验）。
//!
//! 简化（登记 issues）：不建模预售许可进度（签约时点不受施工进度约束）、
//! 不建模增值税（K3 地产行未列税项；任务 13 报表税务口径另议）。

use crate::accounting::{
    AccountingAmount, BusinessEventId, BusinessKind, CashFlowClass, JournalEntry, PostingSide,
};
use crate::calendar::CivilDate;
use crate::company::contracts::ContractId;
use crate::company::counterparty::{CounterpartyId, FlowDirection};
use crate::company::real_estate::projects::ProjectId;
use crate::company::real_estate::{chart, RealEstateBooks, RealEstateError};

/// 单份预售合同：套数、不含税总价（增值税未建模）、已收、是否已交付。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct PresaleContract {
    project: ProjectId,
    buyer: CounterpartyId,
    units: i128,
    price_total: AccountingAmount,
    collected: AccountingAmount,
    delivered: bool,
}

impl PresaleContract {
    pub fn project(&self) -> &ProjectId {
        &self.project
    }

    pub fn buyer(&self) -> &CounterpartyId {
        &self.buyer
    }

    pub fn units(&self) -> i128 {
        self.units
    }

    pub fn price_total(&self) -> AccountingAmount {
        self.price_total
    }

    pub fn collected(&self) -> AccountingAmount {
        self.collected
    }

    pub fn delivered(&self) -> bool {
        self.delivered
    }

    pub(super) fn collect(&mut self, amount: AccountingAmount) {
        self.collected = self.collected.add(amount).expect("validated headroom");
    }

    pub(super) fn mark_delivered(&mut self) {
        self.delivered = true;
    }
}

impl RealEstateBooks {
    /// 预售签约（纯子账事实，无分录）：校验（买方已登记、项目存在、套数
    /// 足够、价格 >0、id 唯一）→ 登记合同。套数占用 = 项目总套数 − 未交付
    /// 合同已占套数。
    pub fn sign_presale(
        &mut self,
        contract: ContractId,
        project: &ProjectId,
        buyer: &CounterpartyId,
        units: i128,
        price_total: AccountingAmount,
        _date: CivilDate,
    ) -> Result<(), RealEstateError> {
        self.ensure_counterparty(buyer)?;
        if self.project(project).is_none() {
            return Err(RealEstateError::UnknownProject {
                project: project.clone(),
            });
        }
        if units < 1 {
            return Err(RealEstateError::NonPositiveUnits { units });
        }
        if !price_total.is_positive() {
            return Err(RealEstateError::NonPositiveAmount {
                what: "presale price",
                amount: price_total,
            });
        }
        let available = self.available_units(project)?;
        if units > available {
            return Err(RealEstateError::PresaleBeyondAvailableUnits {
                project: project.clone(),
                requested: units,
                available,
            });
        }
        if self.presales().contains_key(&contract) {
            return Err(RealEstateError::DuplicatePresale {
                contract: contract.clone(),
            });
        }
        self.presales_mut().insert(
            contract,
            PresaleContract {
                project: project.clone(),
                buyer: buyer.clone(),
                units,
                price_total,
                collected: AccountingAmount::ZERO,
                delivered: false,
            },
        );
        Ok(())
    }

    /// 预售收款（可分期）：Dr 现金 / Cr 2203（经营活动）；**不确认收入**。
    /// 合同已交付后拒绝（尾款走应收路径）。
    pub fn collect_presale(
        &mut self,
        contract: &ContractId,
        amount: AccountingAmount,
        date: CivilDate,
    ) -> Result<BusinessEventId, RealEstateError> {
        let presale = self
            .presale(contract)
            .ok_or_else(|| RealEstateError::UnknownPresale {
                contract: contract.clone(),
            })?;
        if presale.delivered() {
            return Err(RealEstateError::CollectionAfterDelivery {
                contract: contract.clone(),
            });
        }
        if !amount.is_positive() {
            return Err(RealEstateError::NonPositiveAmount {
                what: "presale collection",
                amount,
            });
        }
        let remaining = presale
            .price_total()
            .sub(presale.collected())
            .expect("collected never exceeds price");
        if amount > remaining {
            return Err(RealEstateError::PresaleBeyondContract {
                contract: contract.clone(),
                requested: amount,
                remaining,
            });
        }
        let buyer = presale.buyer().clone();

        let base = self.next_event_id;
        let event = BusinessEventId::new(base);
        self.post_with_commit(
            base + 1,
            vec![JournalEntry {
                source: event,
                date,
                kind: BusinessKind::PresaleCollection,
                cash_flow: CashFlowClass::Operating,
                lines: vec![
                    super::line(chart::acct::CASH, PostingSide::Debit, amount),
                    super::line(chart::acct::CONTRACT_LIAB, PostingSide::Credit, amount),
                ],
            }],
        )?;
        self.presales_mut()
            .get_mut(contract)
            .expect("validated above")
            .collect(amount);
        self.record_flow(
            date,
            &buyer,
            FlowDirection::Inbound,
            amount,
            "presale collection",
        )?;
        Ok(event)
    }

    /// 项目可售套数 = 总套数 − 未交付合同已占套数（已交付合同的套数已在
    /// 交付时结转出结存）。
    pub(super) fn available_units(&self, project: &ProjectId) -> Result<i128, RealEstateError> {
        let state = self
            .project(project)
            .ok_or_else(|| RealEstateError::UnknownProject {
                project: project.clone(),
            })?;
        let reserved: i128 = self
            .presales()
            .values()
            .filter(|presale| !presale.delivered() && presale.project() == project)
            .map(PresaleContract::units)
            .sum();
        Ok(state.total_units() - reserved)
    }
}
