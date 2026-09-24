//! P6 receipt-driven account settlement.
//!
//! This module deliberately consumes only P5-validated receipt deltas.  It does
//! not invoke fee helpers or inspect `GameConfig`: nominal fee calculation is a
//! P4 responsibility and `EnvelopeReceipt::charged` is the actual amount P6
//! must settle.

use super::{EnvelopeReceipt, ReceiptKind, StepFatal};
use crate::account::SettlementTotals;
use crate::session::account_book::AccountBook;
use crate::{Account, AccountId, Money, Side, StockCode};
use rayon::prelude::*;
use std::collections::BTreeMap;

/// Audit counts for the P6 work that was actually applied.  A non-fill receipt
/// must not produce an account settlement call.
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

/// Aggregate positive-quantity fill receipts and settle each account/stock
/// lifecycle in Buy-before-Sell order.
#[cfg(test)]
pub(super) fn apply_receipt_settlements(
    accounts: &mut AccountBook,
    receipts: &[EnvelopeReceipt],
    t1_enabled: bool,
) -> Result<SettlementApplication, StepFatal> {
    let (settled, application) = prepare_receipt_settlements(accounts, receipts, t1_enabled)?;
    accounts.extend(settled);
    Ok(application)
}

/// Build each changed account independently. Results are collected in account
/// order, and nothing is written to the caller until every lifecycle succeeds.
pub(super) fn prepare_receipt_settlements(
    accounts: &AccountBook,
    receipts: &[EnvelopeReceipt],
    t1_enabled: bool,
) -> Result<(BTreeMap<AccountId, Account>, SettlementApplication), StepFatal> {
    let mut totals_by_account_stock: BTreeMap<(AccountId, StockCode), SideTotals> = BTreeMap::new();
    let mut application = SettlementApplication::default();

    for receipt in receipts {
        if receipt.kind != ReceiptKind::Fill {
            continue;
        }
        let qty = receipt
            .qty_before
            .checked_sub(receipt.qty_after)
            .ok_or_else(|| invariant("fill receipt quantity chain regressed"))?;
        if qty == 0 {
            continue;
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

        let totals = totals_by_account_stock
            .entry((receipt.envelope.account, receipt.envelope.stock.clone()))
            .or_default();
        let side_totals = match receipt.envelope.side {
            Side::Buy => &mut totals.buy,
            Side::Sell => &mut totals.sell,
        };
        add_receipt(side_totals, gross, qty, receipt)?;
        application.applied_receipts = application
            .applied_receipts
            .checked_add(1)
            .ok_or_else(|| invariant("settlement receipt count overflow"))?;
    }

    let mut grouped: BTreeMap<AccountId, Vec<(StockCode, SideTotals)>> = BTreeMap::new();
    for ((account_id, stock), totals) in totals_by_account_stock {
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
        application.applied_groups = application
            .applied_groups
            .checked_add(groups)
            .ok_or_else(|| invariant("settlement group count overflow"))?;
        settled.insert(account_id, account);
    }
    Ok((settled, application))
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
