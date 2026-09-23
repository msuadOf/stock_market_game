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

/// 单笔销售抵销：Dr 卖方收入（转移价）/ Cr 卖方成本（转移价−未实现利润）/
/// Cr 买方存货（未实现利润；为零时不产生行）。
pub(super) fn eliminate_sale(
    group: &ValidatedGroup,
    members: &BTreeMap<MemberId, &Books>,
    sale: &IntercompanySale,
) -> Result<WorksheetEntry, ConsolidationError> {
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
    let margin = sale.invoice_amount.sub(sale.cost_amount)?;
    let product = margin
        .cents()
        .checked_mul(sale.unsold_inventory.cents())
        .ok_or_else(|| AccountingError::AmountOverflow {
            op: "unrealized_profit",
            detail: format!("{} * {}", margin.cents(), sale.unsold_inventory.cents()),
        })?;
    let unrealized = AccountingAmount::from_cents(rhe_div(product, sale.invoice_amount.cents())?);
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
