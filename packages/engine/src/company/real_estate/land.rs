//! 购地处理器（K3 地产）：项目建立 + 土地成本入开发存货。
//!
//! 分录：Dr 1541 开发存货 / Cr 1002 银行存款（经营活动——土地是开发商品
//! 的原材料存货，非固定资产投资；CAS 31「投资与筹资之外的活动为经营」的
//! 归类选择，登记 docs）。新项目受单公司项目数上限约束（K2 需求约束）。

use crate::accounting::{
    AccountingAmount, BusinessEventId, BusinessKind, CashFlowClass, JournalEntry, PostingSide,
};
use crate::calendar::CivilDate;
use crate::company::counterparty::{CounterpartyId, FlowDirection};
use crate::company::real_estate::projects::{ProjectId, ProjectState};
use crate::company::real_estate::{chart, RealEstateBooks, RealEstateError};

impl RealEstateBooks {
    /// 购地（项目建立）：校验（对手方已登记、数量 ≥1、成本 >0、id 唯一、
    /// 项目数上限）→ 原子过账 → 项目子账建立。
    pub fn acquire_land(
        &mut self,
        project: ProjectId,
        seller: &CounterpartyId,
        units: i128,
        land_cost: AccountingAmount,
        date: CivilDate,
    ) -> Result<BusinessEventId, RealEstateError> {
        self.ensure_counterparty(seller)?;
        if units < 1 {
            return Err(RealEstateError::NonPositiveUnits { units });
        }
        if !land_cost.is_positive() {
            return Err(RealEstateError::NonPositiveAmount {
                what: "land cost",
                amount: land_cost,
            });
        }
        if self.projects().contains_key(&project) {
            return Err(RealEstateError::DuplicateProject {
                project: project.clone(),
            });
        }
        if self.projects().len() >= self.max_projects {
            return Err(RealEstateError::ProjectCountLimit {
                limit: self.max_projects,
            });
        }

        let base = self.next_event_id;
        let event = BusinessEventId::new(base);
        self.post_with_commit(
            base + 1,
            vec![JournalEntry {
                source: event,
                date,
                kind: BusinessKind::LandAcquisition,
                cash_flow: CashFlowClass::Operating,
                lines: vec![
                    super::line(chart::acct::DEV_INVENTORY, PostingSide::Debit, land_cost),
                    super::line(chart::acct::CASH, PostingSide::Credit, land_cost),
                ],
            }],
        )?;
        self.projects_mut()
            .insert(project.clone(), ProjectState::new(land_cost, units));
        self.record_flow(
            date,
            seller,
            FlowDirection::Outbound,
            land_cost,
            "land acquisition",
        )?;
        Ok(event)
    }
}
