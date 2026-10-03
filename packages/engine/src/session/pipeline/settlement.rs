//! Settlement 消费 ReceiptAggregation 校验后的收据增量。
//!
//! nominal 费用由 P4 计算；此处只累计 EnvelopeReceipt::charged 的实际实收金额。

use super::{EnvelopeReceipt, ReceiptKind, StepFatal};
use crate::account::SettlementTotals;
use crate::session::account_book::AccountBook;
use crate::{Account, AccountId, Money, Side, StockCode};
use rayon::prelude::*;
use std::collections::BTreeMap;

/// 实际 Settlement 工作的审计计数；非 Fill 收据不触发账户结算。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct SettlementApplication {
    pub(super) applied_groups: usize,
    pub(super) applied_receipts: usize,
}

#[derive(Clone, Copy, Debug, Default)]
struct SideTotals {
    buy: SettlementTotals,
    sell: SettlementTotals,
}

/// 聚合正数量 Fill，按 account/stock 的 Buy-before-Sell 生命周期结算。
#[cfg(test)]
pub(super) fn apply_receipt_settlements(
    accounts: &mut AccountBook,
    receipts: &[EnvelopeReceipt],
    t1_enabled: bool,
) -> Result<SettlementApplication, StepFatal> {
    let (settled, application) =
        ReceiptSettlementPlan::from_receipts(receipts)?.prepare_accounts(accounts, t1_enabled)?;
    accounts.extend(settled);
    Ok(application)
}

/// 本批实际 charged 收据的临时结算计划；权威 AccountBook 由 caller 持有。
#[derive(Debug, Default)]
pub(super) struct ReceiptSettlementPlan {
    totals_by_account_stock: BTreeMap<(AccountId, StockCode), SideTotals>,
    application: SettlementApplication,
}

impl ReceiptSettlementPlan {
    pub(super) fn from_receipts(receipts: &[EnvelopeReceipt]) -> Result<Self, StepFatal> {
        let mut plan = Self::default();
        for receipt in receipts {
            plan.record_receipt(receipt)?;
        }
        Ok(plan)
    }

    fn record_receipt(&mut self, receipt: &EnvelopeReceipt) -> Result<(), StepFatal> {
        if receipt.kind != ReceiptKind::Fill {
            return Ok(());
        }
        let qty = receipt
            .qty_before
            .checked_sub(receipt.qty_after)
            .ok_or_else(|| invariant("fill receipt quantity chain regressed"))?;
        if qty == 0 {
            return Ok(());
        }
        let gross = receipt
            .value_after
            .sub(receipt.value_before)
            .map_err(|error| invariant(&error.to_string()))?;
        if gross <= Money::ZERO {
            return Err(invariant(
                "positive-quantity fill receipt has non-positive gross",
            ));
        }
        if receipt.envelope.side == Side::Buy && receipt.charged.stamp_tax != Money::ZERO {
            return Err(invariant("buyer receipt has non-zero stamp tax"));
        }

        let totals = self
            .totals_by_account_stock
            .entry((receipt.envelope.account, receipt.envelope.stock.clone()))
            .or_default();
        let side_totals = match receipt.envelope.side {
            Side::Buy => &mut totals.buy,
            Side::Sell => &mut totals.sell,
        };
        add_receipt(side_totals, gross, qty, receipt)?;
        self.application.applied_receipts = self
            .application
            .applied_receipts
            .checked_add(1)
            .ok_or_else(|| invariant("settlement receipt count overflow"))?;
        Ok(())
    }

    /// 按账户并行准备 shadow，按原账户顺序取首错；全部成功才交出 patch。
    pub(super) fn prepare_accounts(
        mut self,
        accounts: &AccountBook,
        t1_enabled: bool,
    ) -> Result<(BTreeMap<AccountId, Account>, SettlementApplication), StepFatal> {
        let mut grouped: BTreeMap<AccountId, Vec<(StockCode, SideTotals)>> = BTreeMap::new();
        for ((account_id, stock), totals) in self.totals_by_account_stock {
            grouped.entry(account_id).or_default().push((stock, totals));
        }
        let prepared: Vec<Result<_, StepFatal>> = grouped
            .into_par_iter()
            .map(|(account_id, stocks)| {
                let mut account = accounts
                    .get(&account_id)
                    .ok_or_else(|| {
                        invariant(&format!("settlement account {} is missing", account_id.0))
                    })?
                    .clone_for_shadow()
                    .map_err(|error| invariant(&error.to_string()))?;
                let mut applied_groups = 0_usize;
                for (stock, totals) in stocks {
                    if totals.buy.qty > 0 {
                        account
                            .apply_settlement(Side::Buy, stock.clone(), totals.buy, t1_enabled)
                            .map_err(|error| invariant(&error.to_string()))?;
                        applied_groups = applied_groups
                            .checked_add(1)
                            .ok_or_else(|| invariant("settlement group count overflow"))?;
                    }
                    if totals.sell.qty > 0 {
                        account
                            .apply_settlement(Side::Sell, stock, totals.sell, t1_enabled)
                            .map_err(|error| invariant(&error.to_string()))?;
                        applied_groups = applied_groups
                            .checked_add(1)
                            .ok_or_else(|| invariant("settlement group count overflow"))?;
                    }
                }
                Ok((account_id, account, applied_groups))
            })
            .collect();
        let mut settled = BTreeMap::new();
        for result in prepared {
            let (account_id, account, groups) = result?;
            self.application.applied_groups =
                self.application
                    .applied_groups
                    .checked_add(groups)
                    .ok_or_else(|| invariant("settlement group count overflow"))?;
            settled.insert(account_id, account);
        }
        Ok((settled, self.application))
    }
}

fn add_receipt(
    totals: &mut SettlementTotals,
    gross: Money,
    qty: u32,
    receipt: &EnvelopeReceipt,
) -> Result<(), StepFatal> {
    totals.gross = totals
        .gross
        .add(gross)
        .map_err(|error| invariant(&error.to_string()))?;
    totals.commission = totals
        .commission
        .add(receipt.charged.commission)
        .map_err(|error| invariant(&error.to_string()))?;
    totals.stamp_tax = totals
        .stamp_tax
        .add(receipt.charged.stamp_tax)
        .map_err(|error| invariant(&error.to_string()))?;
    totals.transfer_fee = totals
        .transfer_fee
        .add(receipt.charged.transfer_fee)
        .map_err(|error| invariant(&error.to_string()))?;
    totals.qty = totals
        .qty
        .checked_add(qty)
        .ok_or_else(|| invariant("settlement quantity overflow"))?;
    Ok(())
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::settlement".to_owned(),
    }
}
