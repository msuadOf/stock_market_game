//! 交付与尾款处理器（K3 地产）：交付 = 控制权转移时点（CAS 14 §4/§13 已
//! 核验）——冲合同负债、未收部分挂应收尾款、确认收入并按移动加权结转
//! 成本；尾款回收只清应收，**不重复计收入**。
//!
//! 交付分录（单张，非现金）：Dr 2203（已收）/ Dr 1122（未收尾款）/
//! Dr 6401（结转成本）/ Cr 6001（合同总价）/ Cr 1541（开发存货）。

use crate::accounting::{
    AccountingAmount, BusinessEventId, BusinessKind, CashFlowClass, JournalEntry, OpenItemId,
    PostingSide,
};
use crate::calendar::CivilDate;
use crate::company::contracts::ContractId;
use crate::company::counterparty::{CounterpartyId, FlowDirection};
use crate::company::real_estate::{chart, RealEstateBooks, RealEstateError};

/// 交付结果。
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct DeliveryOutcome {
    pub event: BusinessEventId,
    /// 确认收入 = 合同总价（与已收金额无关）。
    pub revenue: AccountingAmount,
    /// 冲减的合同负债 = 交付前累计已收预售款。
    pub liability_cleared: AccountingAmount,
    /// 挂账应收尾款 = 总价 − 已收（为零则 `receivable` 为 None）。
    pub receivable_amount: AccountingAmount,
    /// 尾款应收开项（`AR-{event}`；未收为零时 None）。
    pub receivable: Option<OpenItemId>,
    /// 结转的开发成本（移动加权）。
    pub cost_of_sales: AccountingAmount,
}

impl RealEstateBooks {
    /// 交付（控制权转移）：校验（合同存在、未重复交付、**项目已完工**——
    /// 未达到交付条件不得确认收入）→ 原子过账 → 子账（合同交付标记 +
    /// 成本结转 + 应收开项）。
    pub fn deliver(
        &mut self,
        contract: &ContractId,
        date: CivilDate,
    ) -> Result<DeliveryOutcome, RealEstateError> {
        let presale = self
            .presale(contract)
            .ok_or_else(|| RealEstateError::UnknownPresale {
                contract: contract.clone(),
            })?;
        if presale.delivered() {
            return Err(RealEstateError::PresaleAlreadyDelivered {
                contract: contract.clone(),
            });
        }
        let project_id = presale.project().clone();
        let project_state =
            self.project(&project_id)
                .ok_or_else(|| RealEstateError::UnknownProject {
                    project: project_id.clone(),
                })?;
        if project_state.completed_on().is_none() {
            return Err(RealEstateError::DeliveryBeforeCompletion {
                project: project_id.clone(),
                contract: contract.clone(),
            });
        }
        let units = presale.units();
        let revenue = presale.price_total();
        let collected = presale.collected();
        let receivable_amount = revenue.sub(collected).expect("collected within price");
        let buyer = presale.buyer().clone();
        let cost = project_state.preview_carry_out(&project_id, units)?;

        let base = self.next_event_id;
        let event = BusinessEventId::new(base);
        let mut lines = vec![super::line(chart::acct::COGS, PostingSide::Debit, cost)];
        if collected.is_positive() {
            lines.push(super::line(
                chart::acct::CONTRACT_LIAB,
                PostingSide::Debit,
                collected,
            ));
        }
        if receivable_amount.is_positive() {
            lines.push(super::line(
                chart::acct::AR,
                PostingSide::Debit,
                receivable_amount,
            ));
        }
        lines.push(super::line(
            chart::acct::REVENUE,
            PostingSide::Credit,
            revenue,
        ));
        lines.push(super::line(
            chart::acct::DEV_INVENTORY,
            PostingSide::Credit,
            cost,
        ));
        self.post_with_commit(
            base + 1,
            vec![JournalEntry {
                source: event,
                date,
                kind: BusinessKind::RealEstateDelivery,
                cash_flow: CashFlowClass::NonCash,
                lines,
            }],
        )?;

        let receivable = if receivable_amount.is_positive() {
            let item = OpenItemId(format!("AR-{}", event.value()));
            self.receivables_mut()
                .open(item.clone(), &buyer.0, date, date, receivable_amount)?;
            Some(item)
        } else {
            None
        };
        self.presales_mut()
            .get_mut(contract)
            .expect("validated above")
            .mark_delivered();
        self.projects_mut()
            .get_mut(&project_id)
            .expect("validated above")
            .apply_carry_out(cost, units);
        Ok(DeliveryOutcome {
            event,
            revenue,
            liability_cleared: collected,
            receivable_amount,
            receivable,
            cost_of_sales: cost,
        })
    }

    /// 尾款回收（可部分）：Dr 现金 / Cr 应收（经营活动）；**不确认收入**。
    pub fn collect_final(
        &mut self,
        receivable: &OpenItemId,
        amount: AccountingAmount,
        date: CivilDate,
    ) -> Result<BusinessEventId, RealEstateError> {
        if !amount.is_positive() {
            return Err(RealEstateError::NonPositiveAmount {
                what: "final collection amount",
                amount,
            });
        }
        self.receivables().check_apply(receivable, amount)?;
        let buyer = self
            .receivables()
            .get(receivable)
            .map(|item| CounterpartyId(item.party().to_string()))
            .expect("check_apply validated existence");

        let base = self.next_event_id;
        let event = BusinessEventId::new(base);
        self.post_with_commit(
            base + 1,
            vec![JournalEntry {
                source: event,
                date,
                kind: BusinessKind::FinalPaymentCollected,
                cash_flow: CashFlowClass::Operating,
                lines: vec![
                    super::line(chart::acct::CASH, PostingSide::Debit, amount),
                    super::line(chart::acct::AR, PostingSide::Credit, amount),
                ],
            }],
        )?;
        self.receivables_mut().apply(receivable, amount)?;
        self.record_flow(
            date,
            &buyer,
            FlowDirection::Inbound,
            amount,
            "final payment collection",
        )?;
        Ok(event)
    }
}
