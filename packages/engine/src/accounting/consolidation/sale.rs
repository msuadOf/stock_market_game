//! 内部销售抵销（K3，任务 12）：转移价/成本/未售存货申报校验 + 未实现
//! 利润计算 + 工作底稿分录生成。
//!
//! 未实现利润 = rhe((转移价−成本)×未售存货 / 转移价)（比例分摊假设：
//! 买方存货按转移价计价、批次毛利均匀——登记于 issues 的游戏简化）。
//! 上游（子公司卖方）未实现利润经成员调整后损益自然按少数股东比例分担
//! （见 minority.rs）；下游只冲减母公司损益。

use std::collections::BTreeMap;

use crate::accounting::amount::AccountingAmount;
use crate::accounting::error::AccountingError;
use crate::accounting::inventory::rhe_div;
use crate::accounting::journal::PostingSide;
use crate::accounting::ledger::{AccountElement, LedgerAccountId};
use crate::accounting::Books;

use super::eliminate::{ensure_member, member_def};
use super::error::ConsolidationError;
use super::group::{MemberId, ValidatedGroup};
use super::worksheet::{IntercompanySale, WorksheetEntry, WorksheetLine, WorksheetReason};
use PostingSide::{Credit, Debit};

/// 已通过成员、科目及金额核验的借用申报；不能脱离原申报构造。
pub(super) struct ValidatedIntercompanySale<'a> {
    sale: &'a IntercompanySale,
}

impl IntercompanySale {
    pub(super) fn validate_for<'a>(
        &'a self,
        group: &ValidatedGroup,
        members: &BTreeMap<MemberId, &Books>,
    ) -> Result<ValidatedIntercompanySale<'a>, ConsolidationError> {
        let sale = self;
        ensure_member(group, &sale.seller)?;
        ensure_member(group, &sale.buyer)?;
        if sale.seller == sale.buyer {
            return Err(ConsolidationError::IntercompanySelfReference {
                member: sale.seller.clone(),
            });
        }
        require_element(
            members,
            &sale.seller,
            &sale.revenue_account,
            AccountElement::Revenue,
        )?;
        require_element(
            members,
            &sale.seller,
            &sale.cost_account,
            AccountElement::Expense,
        )?;
        let inventory_def = member_def(members, &sale.buyer, &sale.inventory_account)?;
        if inventory_def.is_cash {
            return Err(ConsolidationError::IntercompanyTouchesCash {
                member: sale.buyer.clone(),
                account: sale.inventory_account.clone(),
            });
        }
        if !matches!(inventory_def.element, AccountElement::Asset) {
            return Err(ConsolidationError::SaleAccountElement {
                member: sale.buyer.clone(),
                account: sale.inventory_account.clone(),
                expected: AccountElement::Asset,
                actual: inventory_def.element,
            });
        }
        if !sale.invoice_amount.is_positive() {
            return Err(ConsolidationError::SaleInvoiceNotPositive {
                seller: sale.seller.clone(),
                buyer: sale.buyer.clone(),
                invoice: sale.invoice_amount,
            });
        }
        if sale.cost_amount.is_negative() || sale.cost_amount > sale.invoice_amount {
            return Err(ConsolidationError::SaleCostBeyondInvoice {
                seller: sale.seller.clone(),
                buyer: sale.buyer.clone(),
                invoice: sale.invoice_amount,
                cost: sale.cost_amount,
            });
        }
        if sale.unsold_inventory.is_negative() || sale.unsold_inventory > sale.invoice_amount {
            return Err(ConsolidationError::SaleUnsoldBeyondInvoice {
                seller: sale.seller.clone(),
                buyer: sale.buyer.clone(),
                invoice: sale.invoice_amount,
                unsold: sale.unsold_inventory,
            });
        }
        Ok(ValidatedIntercompanySale { sale })
    }
}

impl ValidatedIntercompanySale<'_> {
    fn unrealized_profit(&self) -> Result<AccountingAmount, ConsolidationError> {
        let sale = self.sale;
        let margin = sale.invoice_amount.sub(sale.cost_amount)?;
        let product = margin
            .cents()
            .checked_mul(sale.unsold_inventory.cents())
            .ok_or_else(|| AccountingError::AmountOverflow {
                op: "unrealized_profit",
                detail: format!("{} * {}", margin.cents(), sale.unsold_inventory.cents()),
            })?;
        let unrealized =
            AccountingAmount::from_cents(rhe_div(product, sale.invoice_amount.cents())?);
        Ok(unrealized)
    }

    pub(super) fn to_worksheet_entry(&self) -> Result<WorksheetEntry, ConsolidationError> {
        let sale = self.sale;
        let unrealized = self.unrealized_profit()?;
        let mut lines = vec![
            WorksheetLine {
                member: sale.seller.clone(),
                account: sale.revenue_account.clone(),
                side: Debit,
                amount: sale.invoice_amount,
            },
            WorksheetLine {
                member: sale.seller.clone(),
                account: sale.cost_account.clone(),
                side: Credit,
                amount: sale.invoice_amount.sub(unrealized)?,
            },
        ];
        if !unrealized.is_zero() {
            lines.push(WorksheetLine {
                member: sale.buyer.clone(),
                account: sale.inventory_account.clone(),
                side: Credit,
                amount: unrealized,
            });
        }
        Ok(WorksheetEntry {
            reason: WorksheetReason::IntercompanySale,
            lines,
        })
    }
}

/// 申报科目要素必须等于期望要素且非现金。
fn require_element(
    members: &BTreeMap<MemberId, &Books>,
    id: &MemberId,
    account: &LedgerAccountId,
    expected: AccountElement,
) -> Result<(), ConsolidationError> {
    let def = member_def(members, id, account)?;
    if def.is_cash || def.element != expected {
        return Err(ConsolidationError::SaleAccountElement {
            member: id.clone(),
            account: account.clone(),
            expected,
            actual: def.element,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::group::SubsidiaryOwnership;
    use super::*;
    use crate::accounting::ledger::AccountChart;

    #[test]
    fn validated_sale_keeps_account_error_first_and_half_even_zero_lines() {
        let root = MemberId("root".into());
        let sub = MemberId("sub".into());
        let group = ValidatedGroup {
            root: root.clone(),
            subsidiaries: vec![SubsidiaryOwnership {
                id: sub.clone(),
                issued_shares: 100,
                parent_held_shares: 80,
                parent_ownership_bp: 8000,
                minority_bp: 2000,
            }],
        };
        let books = Books::new(AccountChart::generic_v1());
        let members = BTreeMap::from([(root.clone(), &books), (sub.clone(), &books)]);
        let mut sale = IntercompanySale {
            seller: root,
            buyer: sub,
            revenue_account: LedgerAccountId("1001".into()),
            cost_account: LedgerAccountId("6401".into()),
            inventory_account: LedgerAccountId("1601".into()),
            invoice_amount: AccountingAmount::ZERO,
            cost_amount: AccountingAmount::from_cents(4),
            unsold_inventory: AccountingAmount::from_cents(1),
        };
        assert!(matches!(
            sale.validate_for(&group, &members),
            Err(ConsolidationError::SaleAccountElement { .. })
        ));
        sale.revenue_account = LedgerAccountId("6001".into());
        sale.invoice_amount = AccountingAmount::from_cents(8);
        let zero = sale
            .validate_for(&group, &members)
            .unwrap()
            .to_worksheet_entry()
            .unwrap();
        assert_eq!(zero.lines.len(), 2);
        assert_eq!(zero.lines[1].amount.cents(), 8);
        sale.unsold_inventory = AccountingAmount::from_cents(3);
        let rounded = sale
            .validate_for(&group, &members)
            .unwrap()
            .to_worksheet_entry()
            .unwrap();
        assert_eq!(rounded.lines.len(), 3);
        assert_eq!(rounded.lines[2].amount.cents(), 2);
    }
}
