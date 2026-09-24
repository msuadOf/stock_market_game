//! Pure P6 receipt projection for retail experience and its companion event stream.

use super::{EnvelopeReceipt, FeeComponents, ReceiptKind, ReceiptLocalKey};
use crate::{
    AccountId, ExperienceError, Money, OrderId, Position, RetailExperienceState, Side, StockCode,
};
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

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

/// A persistent radix index. Copying a tick candidate shares old receipt nodes;
/// inserting a new identity copies only the path for its global index.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::session) struct RetailProjectionSeen {
    root: Option<Arc<SeenNode>>,
    len: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum SeenNode {
    Branch([Option<Arc<SeenNode>>; 16]),
    Identity(ReceiptLocalKey),
}

struct SeenIdentities<'a>(&'a RetailProjectionSeen);

#[derive(serde::Serialize)]
struct SeenIdentityRef<'a> {
    index: u64,
    local_key: &'a ReceiptLocalKey,
}

impl serde::Serialize for SeenIdentities<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut sequence = serializer.serialize_seq(Some(self.0.len))?;
        if let Some(root) = &self.0.root {
            visit_seen_identities(root, 0, 0, &mut |index, local_key| {
                sequence.serialize_element(&SeenIdentityRef { index, local_key })
            })?;
        }
        sequence.end()
    }
}

impl serde::Serialize for RetailProjectionSeen {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("RetailProjectionSeen", 1)?;
        state.serialize_field("receipts", &SeenIdentities(self))?;
        state.end()
    }
}

impl RetailProjectionSeen {
    #[cfg(test)]
    pub(super) const fn len(&self) -> usize {
        self.len
    }

    #[cfg(test)]
    pub(super) const fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn get(&self, index: u64) -> Option<&ReceiptLocalKey> {
        let mut node = self.root.as_deref()?;
        for depth in 0..16 {
            let SeenNode::Branch(children) = node else {
                return None;
            };
            node = children[nibble(index, depth)].as_deref()?;
        }
        match node {
            SeenNode::Identity(local_key) => Some(local_key),
            SeenNode::Branch(_) => None,
        }
    }

    fn insert(
        &mut self,
        index: u64,
        local_key: ReceiptLocalKey,
    ) -> Result<(), RetailProjectionError> {
        if insert_seen_node(&mut self.root, index, 0, &local_key)? {
            self.len = self
                .len
                .checked_add(1)
                .ok_or(RetailProjectionError::Overflow)?;
        }
        Ok(())
    }

    fn identities(&self) -> Vec<RetailReceiptIdentity> {
        let mut identities = Vec::with_capacity(self.len);
        if let Some(root) = &self.root {
            let result: Result<(), std::convert::Infallible> =
                visit_seen_identities(root, 0, 0, &mut |index, local_key| {
                    identities.push(RetailReceiptIdentity {
                        index,
                        local_key: local_key.clone(),
                    });
                    Ok(())
                });
            match result {
                Ok(()) => {}
                Err(never) => match never {},
            }
        }
        identities
    }

    /// Canonical, lossless persistence projection. The global receipt index is
    /// part of the replay identity; the local key alone is intentionally not a
    /// cross-tick cursor.
    pub(in crate::session) fn authoritative_identities(&self) -> Vec<(u64, ReceiptLocalKey)> {
        self.identities()
            .into_iter()
            .map(|identity| (identity.index, identity.local_key))
            .collect()
    }

    /// Rebuilds replay protection from a save only after proving that it covers
    /// exactly the global receipt prefix `[0, next_receipt_base)`. Rejecting
    /// gaps and duplicate indices prevents a damaged save from silently
    /// replaying an already-settled receipt or skipping a later one.
    pub(in crate::session) fn from_authoritative_identities(
        identities: impl IntoIterator<Item = (u64, ReceiptLocalKey)>,
        next_receipt_base: u64,
    ) -> Result<Self, super::StepFatal> {
        let mut seen = Self::default();
        let mut expected_index = 0_u64;
        for (index, local_key) in identities {
            if index != expected_index {
                return Err(persistence_identity_error(format!(
                    "retail receipt index {index} does not continue the expected prefix at {expected_index}"
                )));
            }
            seen.insert(index, local_key)
                .map_err(|error| persistence_identity_error(error.to_string()))?;
            expected_index = expected_index.checked_add(1).ok_or_else(|| {
                persistence_identity_error("retail receipt index overflow".to_owned())
            })?;
        }
        if expected_index != next_receipt_base {
            return Err(persistence_identity_error(format!(
                "retail receipt prefix ends at {expected_index}, but next_receipt_base is {next_receipt_base}"
            )));
        }
        Ok(seen)
    }
}

#[cfg(test)]
impl RetailProjectionSeen {
    pub(in crate::session) fn insert_test_identity(
        &mut self,
        index: u64,
        local_key: ReceiptLocalKey,
    ) {
        self.insert(index, local_key)
            .expect("test receipt identity must be internally consistent");
    }
}

fn nibble(index: u64, depth: usize) -> usize {
    ((index >> ((15 - depth) * 4)) & 0x0f) as usize
}

fn insert_seen_node(
    slot: &mut Option<Arc<SeenNode>>,
    index: u64,
    depth: usize,
    local_key: &ReceiptLocalKey,
) -> Result<bool, RetailProjectionError> {
    if depth == 16 {
        return match slot {
            Some(node) if matches!(node.as_ref(), SeenNode::Identity(previous) if previous == local_key) => {
                Ok(false)
            }
            Some(_) => Err(RetailProjectionError::ConflictingIdentity { index }),
            None => {
                *slot = Some(Arc::new(SeenNode::Identity(local_key.clone())));
                Ok(true)
            }
        };
    }
    let node =
        slot.get_or_insert_with(|| Arc::new(SeenNode::Branch(std::array::from_fn(|_| None))));
    let SeenNode::Branch(children) = Arc::make_mut(node) else {
        return Err(RetailProjectionError::ConflictingIdentity { index });
    };
    insert_seen_node(
        &mut children[nibble(index, depth)],
        index,
        depth + 1,
        local_key,
    )
}

fn visit_seen_identities<E>(
    node: &SeenNode,
    depth: usize,
    prefix: u64,
    visit: &mut impl FnMut(u64, &ReceiptLocalKey) -> Result<(), E>,
) -> Result<(), E> {
    match node {
        SeenNode::Identity(local_key) => {
            debug_assert_eq!(depth, 16);
            visit(prefix, local_key)?;
        }
        SeenNode::Branch(children) => {
            for (digit, child) in children.iter().enumerate() {
                if let Some(child) = child {
                    visit_seen_identities(child, depth + 1, (prefix << 4) | digit as u64, visit)?;
                }
            }
        }
    }
    Ok(())
}

fn persistence_identity_error(description: String) -> super::StepFatal {
    super::StepFatal::InvariantViolation {
        description,
        location: "pipeline::RetailProjectionSeen::from_authoritative_identities".to_owned(),
    }
}

pub(super) struct RetailProjectionInput<'a> {
    pub(super) retail_experience: &'a crate::session::retail_experience_book::RetailExperienceBook,
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
    /// Only retail accounts with new fills need a copy.
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

struct AccountProjection {
    account: AccountId,
    state: Option<RetailExperienceState>,
    events: Vec<RetailReceiptEvent>,
    final_position_error: Option<RetailProjectionError>,
}

/// Projects only P5-validated, normalized receipts.  It never mutates caller
/// state: only selected retail accounts and seen keys are copied on success.
pub(super) fn project_retail_receipts(
    input: RetailProjectionInput<'_>,
) -> Result<RetailProjectionOutput, RetailProjectionError> {
    let mut seen = input.seen.clone();
    let mut aggregates = BTreeMap::new();
    for receipt in canonical_unseen_receipts(input.receipts, input.seen)? {
        seen.insert(receipt.index, receipt.local_key.clone())?;
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

    let mut by_account: BTreeMap<AccountId, Vec<(RetailOrderIdentity, Aggregate)>> =
        BTreeMap::new();
    for (order, aggregate) in aggregates {
        by_account
            .entry(order.account)
            .or_default()
            .push((order, aggregate));
    }
    let jobs: Vec<_> = input
        .retail_accounts
        .iter()
        .map(|account| (*account, by_account.remove(account).unwrap_or_default()))
        .collect();
    let prepared: Vec<Result<AccountProjection, RetailProjectionError>> = jobs
        .into_par_iter()
        .map(|(account, orders)| project_retail_account(&input, account, orders))
        .collect();

    // Complete every account's fill processing before checking final positions.
    // Indexed collection keeps the first error in account order.
    let prepared: Vec<AccountProjection> = prepared.into_iter().collect::<Result<_, _>>()?;
    let mut retail_experience = BTreeMap::new();
    let mut events = Vec::new();
    for projected in prepared {
        if let Some(error) = projected.final_position_error {
            return Err(error);
        }
        if let Some(state) = projected.state {
            retail_experience.insert(projected.account, state);
        }
        events.extend(projected.events);
    }
    Ok(RetailProjectionOutput {
        retail_experience,
        seen,
        events,
    })
}

fn project_retail_account(
    input: &RetailProjectionInput<'_>,
    account: AccountId,
    orders: Vec<(RetailOrderIdentity, Aggregate)>,
) -> Result<AccountProjection, RetailProjectionError> {
    let mut state = input.retail_experience.get(&account).cloned();
    let before_positions = input.positions_before.get(&account);
    let mut running_qty = BTreeMap::new();
    let mut events = Vec::with_capacity(orders.len());
    for (order, aggregate) in orders {
        let before_qty = *running_qty.entry(order.stock.clone()).or_insert_with(|| {
            before_positions
                .and_then(|positions| positions.get(&order.stock))
                .map_or(0, |position| position.qty)
        });
        let after_qty = match aggregate.side {
            Side::Buy => before_qty.checked_add(aggregate.qty),
            Side::Sell => before_qty.checked_sub(aggregate.qty),
        }
        .ok_or(RetailProjectionError::PositionTransition { account })?;
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
        state
            .as_mut()
            .ok_or(RetailProjectionError::MissingRetailExperience { account })?
            .record_fill_with_order(
                &order.stock,
                aggregate.side,
                average,
                before_qty,
                after_qty,
                cost_before,
                input.market_minute,
                Some(order.order.0),
            )?;
        running_qty.insert(order.stock.clone(), after_qty);
        events.push(RetailReceiptEvent::Filled {
            account,
            stock: order.stock,
            side: aggregate.side,
            order: order.order,
            qty: aggregate.qty,
            gross: aggregate.gross,
            charged: aggregate.charged,
        });
    }

    let final_position_error = running_qty.iter().find_map(|(stock, qty)| {
        let settled_qty = input
            .positions_after
            .get(&account)
            .and_then(|positions| positions.get(stock))
            .map_or(0, |position| position.qty);
        (*qty != settled_qty).then_some(RetailProjectionError::FinalPosition { account })
    });
    if let Some(state) = &mut state {
        let held = input
            .positions_after
            .get(&account)
            .map(|positions| positions.keys().cloned().collect())
            .unwrap_or_default();
        state.prune_watchlist(&held);
    }
    Ok(AccountProjection {
        account,
        state,
        events,
        final_position_error,
    })
}

/// Produces the deterministic P5 receipt subset that this P6 transaction has
/// not previously committed. A global receipt index names exactly one local
/// identity: conflicting reuse fails closed both within this batch and against
/// restored replay protection.
pub(super) fn canonical_unseen_receipts<'a>(
    receipts: &'a [EnvelopeReceipt],
    seen: &RetailProjectionSeen,
) -> Result<Vec<&'a EnvelopeReceipt>, RetailProjectionError> {
    let mut canonical: BTreeMap<u64, (RetailReceiptIdentity, &EnvelopeReceipt)> = BTreeMap::new();
    for receipt in receipts {
        let identity = receipt_identity(receipt);
        if let Some((existing_identity, existing_receipt)) = canonical.get(&receipt.index) {
            if existing_identity != &identity || !same_payload(existing_receipt, receipt) {
                return Err(RetailProjectionError::ConflictingIdentity {
                    index: receipt.index,
                });
            }
            continue;
        }
        canonical.insert(receipt.index, (identity, receipt));
    }

    let mut unseen = Vec::with_capacity(canonical.len());
    for (index, (identity, receipt)) in canonical {
        match seen.get(index) {
            None => unseen.push(receipt),
            Some(previous) if previous == &identity.local_key => {}
            Some(_) => return Err(RetailProjectionError::ConflictingIdentity { index }),
        }
    }
    Ok(unseen)
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
