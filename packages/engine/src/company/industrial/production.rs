//! 生产（K3 工商）：领料（移动加权成本）+ 现金加工费 → 在产品（5001）→
//! 完工结转库存商品。**简化**：领料与完工同日单事件完成（不分多日在产品，
//! 不设制造费用科目——加工费直接归集 5001；`docs/company-accounting.md` §2.2
//! 采购/生产/完工结转行的 CAS 1 原文受阻，按 K3 游戏假设实现）。
//!
//! 现金/非现金分离：加工费现金流出（经营）；领料与完工是非现金结转。
//! BusinessKind 标签映射：含加工费的生产投入 = CashExpense/Operating；
//! 纯结转 = Depreciation/NonCash（通用非现金账面结转标签，见 issues.md）。

use crate::accounting::{
    AccountingAmount, BusinessEventId, BusinessKind, CashFlowClass, InventoryItemCode,
    JournalEntry, LedgerAccountId, PostingSide,
};
use crate::calendar::CivilDate;
use crate::company::industrial::error::map_inventory_issue;
use crate::company::industrial::{chart, IndustrialBooks, IndustrialError};

/// 生产结果（单一业务事件 = 1–2 张分录）。
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct ProductionOutcome {
    pub events: Vec<BusinessEventId>,
    /// 领用材料成本（移动加权平均）。
    pub material_cost: AccountingAmount,
    pub conversion_cost: AccountingAmount,
    /// 完工入账总成本 = 材料 + 加工费。
    pub total_cost: AccountingAmount,
}

impl IndustrialBooks {
    /// 生产：Dr 5001（材料+加工费）/ Cr 原材料科目、Cr 现金（加工费）；
    /// Dr 库存商品科目 / Cr 5001。整批原子（加工费现金不足 ⇒ 整批拒绝）。
    #[allow(clippy::too_many_arguments)]
    pub fn produce(
        &mut self,
        raw: InventoryItemCode,
        raw_quantity: i128,
        finished: InventoryItemCode,
        finished_quantity: i128,
        finished_account: LedgerAccountId,
        conversion_cost: AccountingAmount,
        date: CivilDate,
    ) -> Result<ProductionOutcome, IndustrialError> {
        if finished_quantity <= 0 {
            return Err(IndustrialError::NonPositiveAmount {
                what: "finished quantity",
                amount: AccountingAmount::from_cents(finished_quantity),
            });
        }
        if raw_quantity <= 0 {
            return Err(IndustrialError::NonPositiveAmount {
                what: "raw quantity",
                amount: AccountingAmount::from_cents(raw_quantity),
            });
        }
        if conversion_cost.is_negative() {
            return Err(IndustrialError::NonPositiveAmount {
                what: "conversion cost",
                amount: conversion_cost,
            });
        }
        let material_cost = self
            .inventory()
            .preview_issue(&raw, raw_quantity)
            .map_err(|error| map_inventory_issue(raw_quantity, error))?;
        let raw_account = self
            .inventory()
            .get(&raw)
            .map(|state| state.account().clone())
            .expect("preview_issue validated existence");
        let total_cost = material_cost.add(conversion_cost)?;

        let base = self.next_event_id;
        let first_event = BusinessEventId::new(base);
        let mut input_lines = vec![
            super::line(chart::acct::WIP, PostingSide::Debit, material_cost),
            super::line(&raw_account.0, PostingSide::Credit, material_cost),
        ];
        let (input_kind, input_cf) = if conversion_cost.is_positive() {
            input_lines.push(super::line(
                chart::acct::WIP,
                PostingSide::Debit,
                conversion_cost,
            ));
            input_lines.push(super::line(
                chart::acct::BANK,
                PostingSide::Credit,
                conversion_cost,
            ));
            (BusinessKind::CashExpense, CashFlowClass::Operating)
        } else {
            (BusinessKind::Depreciation, CashFlowClass::NonCash)
        };
        self.post_with_commit(
            base + 2,
            vec![
                JournalEntry {
                    source: first_event,
                    date,
                    kind: input_kind,
                    cash_flow: input_cf,
                    lines: input_lines,
                },
                JournalEntry {
                    source: BusinessEventId::new(base + 1),
                    date,
                    kind: BusinessKind::Depreciation,
                    cash_flow: CashFlowClass::NonCash,
                    lines: vec![
                        super::line(&finished_account.0, PostingSide::Debit, total_cost),
                        super::line(chart::acct::WIP, PostingSide::Credit, total_cost),
                    ],
                },
            ],
        )?;

        self.inventory_mut()
            .apply_issue(&raw, raw_quantity)
            .map_err(|error| map_inventory_issue(raw_quantity, error))?;
        self.inventory_mut()
            .receipt(finished, finished_account, finished_quantity, total_cost)?;
        Ok(ProductionOutcome {
            events: vec![first_event, BusinessEventId::new(base + 1)],
            material_cost,
            conversion_cost,
            total_cost,
        })
    }
}
