//! Pure P6 receipt projection for retail experience and its companion event stream.

use super::{EnvelopeReceipt, FeeComponents, ReceiptKind, ReceiptLocalKey};
use crate::{
    AccountId, ExperienceError, Money, OrderId, Position, RetailExperienceState, Side, StockCode,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, serde::Serialize)]
pub(super) struct RetailReceiptIdentity {
    pub(super) index: u64,
    pub(super) local_key: ReceiptLocalKey,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(super) struct RetailOrderIdentity {
    pub(super) account: AccountId,
    pub(super) stock: StockCode,
    pub(super) side_rank: u8,
    pub(super) order: OrderId,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Serialize)]
pub(in crate::session) struct RetailProjectionSeen {
    pub(super) receipts: BTreeSet<RetailReceiptIdentity>,
}

#[cfg(test)]
impl RetailProjectionSeen {
    pub(in crate::session) fn insert_test_identity(
        &mut self,
        index: u64,
        local_key: ReceiptLocalKey,
    ) {
        self.receipts
            .insert(RetailReceiptIdentity { index, local_key });
    }
}

pub(super) struct RetailProjectionInput<'a> {
    pub(super) retail_experience: &'a BTreeMap<AccountId, RetailExperienceState>,
    /// P6 derives this from authoritative `AccountKind::Retail`; experience
    /// storage must never be used to infer an account's kind.
    pub(super) retail_accounts: &'a BTreeSet<AccountId>,
    pub(super) positions_before: &'a BTreeMap<AccountId, BTreeMap<StockCode, Position>>,
    pub(super) positions_after: &'a BTreeMap<AccountId, BTreeMap<StockCode, Position>>,
    pub(super) seen: &'a RetailProjectionSeen,
    pub(super) market_minute: u64,
    pub(super) receipts: &'a [EnvelopeReceipt],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum RetailReceiptEvent {
    Filled {
        account: AccountId,
        stock: StockCode,
        side: Side,
        order: OrderId,
        qty: u32,
        gross: Money,
        /// P6 reports the actual charged components only; nominal is intentionally absent.
        charged: FeeComponents,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RetailProjectionOutput {
    pub(super) retail_experience: BTreeMap<AccountId, RetailExperienceState>,
    pub(super) seen: RetailProjectionSeen,
    pub(super) events: Vec<RetailReceiptEvent>,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub(super) enum RetailProjectionError {
    #[error("retail receipt {index} has a regressing quantity chain")]
    QuantityRegression { index: u64 },
    #[error("retail receipt {index} has non-positive gross")]
    NonPositiveGross { index: u64 },
    #[error("retail order {order:?} has a non-positive average price")]
    NonPositiveAveragePrice { order: OrderId },
    #[error("retail receipt identity {index} has conflicting payloads")]
    ConflictingIdentity { index: u64 },
    #[error("retail receipt aggregation overflow")]
    Overflow,
    #[error("retail receipt position transition is inconsistent for account {account:?}")]
    PositionTransition { account: AccountId },
    #[error(
        "retail receipt final position disagrees with settled account for account {account:?}"
    )]
    FinalPosition { account: AccountId },
    #[error("retail experience state is missing for account {account:?}")]
    MissingRetailExperience { account: AccountId },
    #[error(transparent)]
    Experience(#[from] ExperienceError),
}

#[derive(Clone)]
struct Aggregate {
    side: Side,
    qty: u32,
    gross: Money,
    charged: FeeComponents,
}

/// Projects only P5-validated, normalized receipts.  It never mutates caller
/// state: all maps and seen keys are cloned and returned together on success.
pub(super) fn project_retail_receipts(
    input: RetailProjectionInput<'_>,
) -> Result<RetailProjectionOutput, RetailProjectionError> {
    let mut seen = input.seen.clone();
    let mut aggregates = BTreeMap::new();
    for receipt in canonical_unseen_receipts(input.receipts, input.seen)? {
        seen.receipts.insert(receipt_identity(receipt));
        match receipt.kind {
            ReceiptKind::Release | ReceiptKind::Reject | ReceiptKind::Rollover => continue,
            ReceiptKind::Fill => {}
        }
        if !input.retail_accounts.contains(&receipt.envelope.account) {
            continue;
        }
        let qty = receipt.qty_before.checked_sub(receipt.qty_after).ok_or(
            RetailProjectionError::QuantityRegression {
                index: receipt.index,
            },
        )?;
        let gross = receipt.value_after.sub(receipt.value_before).map_err(|_| {
            RetailProjectionError::NonPositiveGross {
                index: receipt.index,
            }
        })?;
        if qty == 0 || gross <= Money::ZERO {
            return Err(RetailProjectionError::NonPositiveGross {
                index: receipt.index,
            });
        }
        let order = order_identity(receipt);
        let aggregate = aggregates.entry(order).or_insert_with(|| Aggregate {
            side: receipt.envelope.side,
            qty: 0,
            gross: Money::ZERO,
            charged: FeeComponents::ZERO,
        });
        aggregate.qty = aggregate
            .qty
            .checked_add(qty)
            .ok_or(RetailProjectionError::Overflow)?;
        aggregate.gross = aggregate
            .gross
            .add(gross)
            .map_err(|_| RetailProjectionError::Overflow)?;
        aggregate.charged = add_fee(aggregate.charged, receipt.charged)?;
    }

    let mut retail_experience = input.retail_experience.clone();
    let mut running_qty = BTreeMap::new();
    let mut events = Vec::new();
    for (order, aggregate) in aggregates {
        let before_positions = input.positions_before.get(&order.account);
        let before_qty = *running_qty
            .entry((order.account, order.stock.clone()))
            .or_insert_with(|| {
                before_positions
                    .and_then(|positions| positions.get(&order.stock))
                    .map_or(0, |position| position.qty)
            });
        let after_qty = match aggregate.side {
            Side::Buy => before_qty.checked_add(aggregate.qty),
            Side::Sell => before_qty.checked_sub(aggregate.qty),
        }
        .ok_or(RetailProjectionError::PositionTransition {
            account: order.account,
        })?;
        let average = Money::from_cents(
            aggregate
                .gross
                .cents()
                .checked_div(i64::from(aggregate.qty))
                .ok_or(RetailProjectionError::Overflow)?,
        );
        if average <= Money::ZERO {
            return Err(RetailProjectionError::NonPositiveAveragePrice { order: order.order });
        }
        let cost_before = before_positions
            .and_then(|positions| positions.get(&order.stock))
            .and_then(Position::cost_price)
            .filter(|cost| *cost > Money::ZERO);
        retail_state(&mut retail_experience, order.account)?.record_fill_with_order(
            &order.stock,
            aggregate.side,
            average,
            before_qty,
            after_qty,
            cost_before,
            input.market_minute,
            Some(order.order.0),
        )?;
        running_qty.insert((order.account, order.stock.clone()), after_qty);
        events.push(RetailReceiptEvent::Filled {
            account: order.account,
            stock: order.stock,
            side: aggregate.side,
            order: order.order,
            qty: aggregate.qty,
            gross: aggregate.gross,
            charged: aggregate.charged,
        });
    }

    for ((account, stock), qty) in &running_qty {
        let settled_qty = input
            .positions_after
            .get(account)
            .and_then(|positions| positions.get(stock))
            .map_or(0, |position| position.qty);
        if *qty != settled_qty {
            return Err(RetailProjectionError::FinalPosition { account: *account });
        }
    }
    for (account, state) in &mut retail_experience {
        if !input.retail_accounts.contains(account) {
            continue;
        }
        let held = input
            .positions_after
            .get(account)
            .map(|positions| positions.keys().cloned().collect())
            .unwrap_or_default();
        state.prune_watchlist(&held);
    }
    Ok(RetailProjectionOutput {
        retail_experience,
        seen,
        events,
    })
}

/// Obtains the authoritative retail state for an aggregate already classified
/// as retail.  Keep this fallible even though the normal classification pass
/// makes the absence impossible: an ownership mismatch must be reported as a
/// typed P6 failure rather than panicking or silently dropping the fill.
pub(super) fn retail_state(
    retail_experience: &mut BTreeMap<AccountId, RetailExperienceState>,
    account: AccountId,
) -> Result<&mut RetailExperienceState, RetailProjectionError> {
    retail_experience
        .get_mut(&account)
        .ok_or(RetailProjectionError::MissingRetailExperience { account })
}

/// Produces the deterministic P5 receipt subset that this P6 transaction has
/// not previously committed. Conflicting duplicate identities fail closed.
pub(super) fn canonical_unseen_receipts<'a>(
    receipts: &'a [EnvelopeReceipt],
    seen: &RetailProjectionSeen,
) -> Result<Vec<&'a EnvelopeReceipt>, RetailProjectionError> {
    let mut canonical: BTreeMap<RetailReceiptIdentity, &EnvelopeReceipt> = BTreeMap::new();
    for receipt in receipts {
        let identity = receipt_identity(receipt);
        if let Some(existing) = canonical.insert(identity.clone(), receipt) {
            if !same_payload(existing, receipt) {
                return Err(RetailProjectionError::ConflictingIdentity {
                    index: receipt.index,
                });
            }
        }
    }
    Ok(canonical
        .into_iter()
        .filter_map(|(identity, receipt)| (!seen.receipts.contains(&identity)).then_some(receipt))
        .collect())
}

fn receipt_identity(receipt: &EnvelopeReceipt) -> RetailReceiptIdentity {
    RetailReceiptIdentity {
        index: receipt.index,
        local_key: receipt.local_key.clone(),
    }
}

fn order_identity(receipt: &EnvelopeReceipt) -> RetailOrderIdentity {
    RetailOrderIdentity {
        account: receipt.envelope.account,
        stock: receipt.envelope.stock.clone(),
        side_rank: match receipt.envelope.side {
            Side::Buy => 0,
            Side::Sell => 1,
        },
        order: receipt.envelope.order,
    }
}

fn add_fee(
    left: FeeComponents,
    right: FeeComponents,
) -> Result<FeeComponents, RetailProjectionError> {
    Ok(FeeComponents {
        commission: left
            .commission
            .add(right.commission)
            .map_err(|_| RetailProjectionError::Overflow)?,
        stamp_tax: left
            .stamp_tax
            .add(right.stamp_tax)
            .map_err(|_| RetailProjectionError::Overflow)?,
        transfer_fee: left
            .transfer_fee
            .add(right.transfer_fee)
            .map_err(|_| RetailProjectionError::Overflow)?,
    })
}

fn same_payload(left: &EnvelopeReceipt, right: &EnvelopeReceipt) -> bool {
    left.envelope == right.envelope
        && left.kind == right.kind
        && left.qty_before == right.qty_before
        && left.qty_after == right.qty_after
        && left.value_before == right.value_before
        && left.value_after == right.value_after
        && left.delta == right.delta
        && left.nominal == right.nominal
        && left.charged == right.charged
        && left.charged_before == right.charged_before
        && left.charged_after == right.charged_after
        && left.deliver_qty == right.deliver_qty
        && left.deliver_cash == right.deliver_cash
}
