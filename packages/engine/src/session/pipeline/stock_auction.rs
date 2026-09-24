use super::{
    transition::{BuyFillInput, FillTransition, SellFillInput},
    Envelope, EnvelopeKey, EnvelopeOrigin, EnvelopeReceipt, FeeComponents, JournalRank,
    ReceiptDelta, ReceiptKind, ReceiptLocalKey, ReceiptSource, ReceiptTransition, ResVec,
    StepFatal,
};
use crate::{AccountId, GameConfig, Money, MoneyError, OrderId, Side, StockCode, StockExchange};
use std::collections::{BTreeMap, BTreeSet};

#[path = "b2_auction_day_end.rs"]
pub(super) mod b2_auction_day_end;
#[cfg(test)]
#[path = "b2_auction_day_end_tests.rs"]
mod b2_auction_day_end_tests;

/// Explicit auction phase semantics. Opening cancellation is allowed only while
/// `elapsed_ticks < cancelable_ticks`; the closing call auction never accepts it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AuctionPhase {
    Opening {
        elapsed_ticks: u64,
        cancelable_ticks: u64,
    },
    Closing,
}

impl AuctionPhase {
    const fn allows_cancel(self) -> bool {
        matches!(
            self,
            Self::Opening {
                elapsed_ticks,
                cancelable_ticks,
            } if elapsed_ticks < cancelable_ticks
        )
    }
}

/// An auction queue entry backed by the same envelope that P5 will audit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct AuctionOrder {
    pub(super) envelope: Envelope,
    pub(super) arrival_seq: u64,
}

#[derive(Clone, Debug)]
pub(super) enum AuctionOperation {
    Place(AuctionOrder),
    Cancel {
        sealed_index: u64,
        account: AccountId,
        order_id: OrderId,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AuctionCancelRejection {
    NotCancelable,
    OrderNotFound,
    NotOrderOwner,
    SameTickEnvelope,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum AuctionOperationFact {
    Placed {
        account: AccountId,
        order_id: OrderId,
    },
    Canceled {
        account: AccountId,
        order_id: OrderId,
        remaining_qty: u32,
    },
    Rejected {
        account: AccountId,
        order_id: OrderId,
        reason: AuctionCancelRejection,
    },
}

#[derive(Debug)]
pub(super) struct AuctionOperationOutput {
    pub(super) fact: AuctionOperationFact,
    pub(super) receipt: Option<EnvelopeReceipt>,
    pub(super) terminal_key: Option<EnvelopeKey>,
}

/// Stock-local, deterministic auction operation state. P3 validation remains
/// outside this component; accepted limit orders and cancellation operations
/// are applied here in their already-sealed order.
#[derive(Clone, Debug)]
pub(super) struct StockAuctionState {
    stock: StockCode,
    orders: Vec<AuctionOrder>,
}

impl StockAuctionState {
    pub(super) const fn new(stock: StockCode) -> Self {
        Self {
            stock,
            orders: Vec::new(),
        }
    }

    pub(super) fn orders(&self) -> &[AuctionOrder] {
        &self.orders
    }

    pub(super) fn apply_operation(
        &mut self,
        phase: AuctionPhase,
        operation: AuctionOperation,
    ) -> Result<AuctionOperationOutput, StepFatal> {
        match operation {
            AuctionOperation::Place(order) => self.place(order),
            AuctionOperation::Cancel {
                sealed_index,
                account,
                order_id,
            } => self.cancel(phase, sealed_index, account, order_id),
        }
    }

    fn place(&mut self, order: AuctionOrder) -> Result<AuctionOperationOutput, StepFatal> {
        validate_order(&self.stock, &order)?;
        let key = order.envelope.key().clone();
        if self
            .orders
            .iter()
            .any(|queued| queued.envelope.key().order == key.order)
        {
            return Err(state_invariant("duplicate auction order id"));
        }
        self.orders.push(order);
        Ok(AuctionOperationOutput {
            fact: AuctionOperationFact::Placed {
                account: key.account,
                order_id: key.order,
            },
            receipt: None,
            terminal_key: None,
        })
    }

    fn cancel(
        &mut self,
        phase: AuctionPhase,
        sealed_index: u64,
        account: AccountId,
        order_id: OrderId,
    ) -> Result<AuctionOperationOutput, StepFatal> {
        if !phase.allows_cancel() {
            return Ok(cancel_rejected(
                account,
                order_id,
                AuctionCancelRejection::NotCancelable,
            ));
        }
        let Some(index) = self
            .orders
            .iter()
            .position(|order| order.envelope.key().order == order_id)
        else {
            return Ok(cancel_rejected(
                account,
                order_id,
                AuctionCancelRejection::OrderNotFound,
            ));
        };
        if self.orders[index].envelope.key().account != account {
            return Ok(cancel_rejected(
                account,
                order_id,
                AuctionCancelRejection::NotOrderOwner,
            ));
        }
        if self.orders[index].envelope.origin() == EnvelopeOrigin::P3Created {
            return Ok(cancel_rejected(
                account,
                order_id,
                AuctionCancelRejection::SameTickEnvelope,
            ));
        }

        let order = self.orders.remove(index);
        let key = order.envelope.key().clone();
        let audit = order.envelope.audit();
        let receipt = release_receipt(&order, ReceiptSource::SealedIntent(sealed_index), 0)?;
        Ok(AuctionOperationOutput {
            fact: AuctionOperationFact::Canceled {
                account,
                order_id,
                remaining_qty: audit.remaining_qty,
            },
            receipt: Some(receipt),
            terminal_key: Some(key),
        })
    }
}

fn cancel_rejected(
    account: AccountId,
    order_id: OrderId,
    reason: AuctionCancelRejection,
) -> AuctionOperationOutput {
    AuctionOperationOutput {
        fact: AuctionOperationFact::Rejected {
            account,
            order_id,
            reason,
        },
        receipt: None,
        terminal_key: None,
    }
}

fn validate_order(stock: &StockCode, order: &AuctionOrder) -> Result<(), StepFatal> {
    order.envelope.validate()?;
    let key = order.envelope.key();
    let audit = order.envelope.audit();
    if &key.stock != stock {
        return Err(state_invariant("auction order belongs to another stock"));
    }
    if key.order.0 != order.arrival_seq {
        return Err(state_invariant(
            "auction arrival sequence must equal the original order id",
        ));
    }
    if audit.remaining_qty == 0 || audit.limit <= Money::ZERO {
        return Err(state_invariant(
            "auction order must have positive remaining quantity and limit",
        ));
    }
    match key.side {
        Side::Buy if order.envelope.live().shares != 0 => {
            Err(state_invariant("buy auction envelope contains shares"))
        }
        Side::Sell
            if order.envelope.live().cash != Money::ZERO
                || order.envelope.live().shares != audit.remaining_qty =>
        {
            Err(state_invariant(
                "sell auction envelope must contain every remaining share and no cash",
            ))
        }
        Side::Buy | Side::Sell => Ok(()),
    }
}

#[derive(Clone, Debug)]
pub(super) struct AuctionCompletionInput {
    pub(super) state: StockAuctionState,
    pub(super) phase: AuctionPhase,
    pub(super) previous_close: Money,
    pub(super) exchange: StockExchange,
    pub(super) price_tick: Money,
    pub(super) config: GameConfig,
    /// Live continuous-book envelopes that the same closing boundary will
    /// terminate. They participate in the stock-local DayEnd source numbering
    /// even though they never enter the closing-auction matching queue.
    pub(super) day_end_envelopes: Vec<EnvelopeKey>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct AuctionMatch {
    pub(super) buy: EnvelopeKey,
    pub(super) sell: EnvelopeKey,
    pub(super) qty: u32,
    pub(super) price: Money,
}

#[derive(Debug)]
pub(super) struct AuctionCompletionOutput {
    pub(super) clearing: Option<ClearingSelection>,
    pub(super) matched_volume: u64,
    pub(super) receipts: Vec<EnvelopeReceipt>,
    pub(super) terminal_keys: Vec<EnvelopeKey>,
    pub(super) continuous_orders: Vec<AuctionOrder>,
    pub(super) matches: Vec<AuctionMatch>,
    pub(super) day_end_source_indices: BTreeMap<EnvelopeKey, u32>,
}

/// Completes one stock's call auction without touching a market or account.
/// Opening remainders retain their envelope, order id and arrival sequence for
/// the continuous book. Closing remainders are instead terminated by DayEnd.
pub(super) fn complete_stock_auction(
    input: AuctionCompletionInput,
) -> Result<AuctionCompletionOutput, StepFatal> {
    let AuctionCompletionInput {
        mut state,
        phase,
        previous_close,
        exchange,
        price_tick,
        config,
        day_end_envelopes,
    } = input;
    let source_indices = auction_source_indices(&state.orders)?;
    let clearing_orders: Vec<_> = state
        .orders
        .iter()
        .map(|order| ClearingOrder {
            side: order.envelope.key().side,
            limit: order.envelope.audit().limit,
            qty: u64::from(order.envelope.audit().remaining_qty),
        })
        .collect();
    let clearing = select_clearing(&clearing_orders, previous_close, exchange, price_tick)?;
    let mut receipts = Vec::new();
    let mut matches = Vec::new();
    let mut ordinals = vec![0_u64; state.orders.len()];
    let mut matched_volume = 0_u64;

    if let Some(selection) = clearing {
        let mut buys: Vec<usize> = state
            .orders
            .iter()
            .enumerate()
            .filter(|(_, order)| {
                order.envelope.key().side == Side::Buy
                    && order.envelope.audit().limit >= selection.price
            })
            .map(|(index, _)| index)
            .collect();
        let mut sells: Vec<usize> = state
            .orders
            .iter()
            .enumerate()
            .filter(|(_, order)| {
                order.envelope.key().side == Side::Sell
                    && order.envelope.audit().limit <= selection.price
            })
            .map(|(index, _)| index)
            .collect();
        buys.sort_by_key(|index| {
            (
                std::cmp::Reverse(state.orders[*index].envelope.audit().limit),
                state.orders[*index].arrival_seq,
            )
        });
        sells.sort_by_key(|index| {
            (
                state.orders[*index].envelope.audit().limit,
                state.orders[*index].arrival_seq,
            )
        });

        let mut buy_cursor = 0;
        let mut sell_cursor = 0;
        while buy_cursor < buys.len() && sell_cursor < sells.len() {
            let buy_index = buys[buy_cursor];
            let sell_index = sells[sell_cursor];
            let qty = state.orders[buy_index]
                .envelope
                .audit()
                .remaining_qty
                .min(state.orders[sell_index].envelope.audit().remaining_qty);
            if qty == 0 {
                return Err(state_invariant(
                    "auction matching encountered an exhausted queue entry",
                ));
            }
            let buy_key = state.orders[buy_index].envelope.key().clone();
            let sell_key = state.orders[sell_index].envelope.key().clone();
            let buy_source = ReceiptSource::Auction(source_index(&source_indices, &buy_key)?);
            let sell_source = ReceiptSource::Auction(source_index(&source_indices, &sell_key)?);
            receipts.push(fill_receipt(
                &mut state.orders[buy_index],
                qty,
                selection.price,
                buy_source,
                ordinals[buy_index],
                &config,
            )?);
            ordinals[buy_index] = next_ordinal(ordinals[buy_index])?;
            receipts.push(fill_receipt(
                &mut state.orders[sell_index],
                qty,
                selection.price,
                sell_source,
                ordinals[sell_index],
                &config,
            )?);
            ordinals[sell_index] = next_ordinal(ordinals[sell_index])?;
            matched_volume = matched_volume
                .checked_add(u64::from(qty))
                .ok_or_else(|| state_invariant("auction matched volume overflow"))?;
            matches.push(AuctionMatch {
                buy: buy_key,
                sell: sell_key,
                qty,
                price: selection.price,
            });

            if state.orders[buy_index].envelope.audit().remaining_qty == 0 {
                buy_cursor += 1;
            }
            if state.orders[sell_index].envelope.audit().remaining_qty == 0 {
                sell_cursor += 1;
            }
        }
        if matched_volume != selection.volume {
            return Err(state_invariant(
                "auction match legs disagree with selected clearing volume",
            ));
        }
    }

    let day_end_indices = if phase == AuctionPhase::Closing {
        Some(day_end_source_indices(&state.orders, day_end_envelopes)?)
    } else {
        if !day_end_envelopes.is_empty() {
            return Err(state_invariant(
                "opening auction received closing DayEnd envelopes",
            ));
        }
        None
    };
    let mut terminal_keys = Vec::new();
    let mut continuous_orders = Vec::new();
    for (index, mut order) in state.orders.into_iter().enumerate() {
        let key = order.envelope.key().clone();
        if order.envelope.audit().remaining_qty == 0 {
            terminal_keys.push(key);
            continue;
        }
        let auction_source_index = source_index(&source_indices, &key)?;
        let rollover = rollover_receipt(
            &order,
            ReceiptSource::Auction(auction_source_index),
            ordinals[index],
        )?;
        apply_last_delta(&mut order, &rollover)?;
        receipts.push(rollover);
        match phase {
            AuctionPhase::Opening { .. } => continuous_orders.push(order),
            AuctionPhase::Closing => {
                let day_end_source_index = source_index(
                    day_end_indices
                        .as_ref()
                        .ok_or_else(|| state_invariant("closing DayEnd source map is absent"))?,
                    &key,
                )?;
                let day_end =
                    release_receipt(&order, ReceiptSource::DayEnd(day_end_source_index), 0)?;
                apply_last_delta(&mut order, &day_end)?;
                receipts.push(day_end);
                terminal_keys.push(key);
            }
        }
    }

    Ok(AuctionCompletionOutput {
        clearing,
        matched_volume,
        receipts,
        terminal_keys,
        continuous_orders,
        matches,
        day_end_source_indices: day_end_indices.unwrap_or_default(),
    })
}

fn auction_source_indices(
    orders: &[AuctionOrder],
) -> Result<BTreeMap<EnvelopeKey, u32>, StepFatal> {
    let keys: BTreeSet<_> = orders
        .iter()
        .map(|order| order.envelope.key().clone())
        .collect();
    if keys.len() != orders.len() {
        return Err(state_invariant("duplicate auction envelope key"));
    }
    keys.into_iter()
        .enumerate()
        .map(|(index, key)| {
            let index = u32::try_from(index)
                .map_err(|_| state_invariant("auction source index overflow"))?;
            Ok((key, index))
        })
        .collect()
}

fn day_end_source_indices(
    orders: &[AuctionOrder],
    continuous_envelopes: Vec<EnvelopeKey>,
) -> Result<BTreeMap<EnvelopeKey, u32>, StepFatal> {
    let mut keys: BTreeSet<_> = orders
        .iter()
        .filter(|order| order.envelope.audit().remaining_qty > 0)
        .map(|order| order.envelope.key().clone())
        .collect();
    for key in continuous_envelopes {
        if !keys.insert(key) {
            return Err(state_invariant(
                "DayEnd envelope appears in both auction and continuous state",
            ));
        }
    }
    keys.into_iter()
        .enumerate()
        .map(|(index, key)| {
            let index = u32::try_from(index)
                .map_err(|_| state_invariant("DayEnd source index overflow"))?;
            Ok((key, index))
        })
        .collect()
}

pub(super) fn auction_indicative(
    state: &StockAuctionState,
    previous_close: Money,
    exchange: StockExchange,
    price_tick: Money,
) -> Result<(Option<ClearingSelection>, u64), StepFatal> {
    let orders = state
        .orders
        .iter()
        .map(|order| ClearingOrder {
            side: order.envelope.key().side,
            limit: order.envelope.audit().limit,
            qty: u64::from(order.envelope.audit().remaining_qty),
        })
        .collect::<Vec<_>>();
    let clearing = select_clearing(&orders, previous_close, exchange, price_tick)?;
    let imbalance = match clearing {
        Some(selection) => selection.imbalance,
        None => checked_total_imbalance(&orders)?,
    };
    Ok((clearing, imbalance))
}

pub(super) fn day_end_release_receipt(
    envelope: &Envelope,
    source_index: u32,
) -> Result<EnvelopeReceipt, StepFatal> {
    terminal_receipt(
        envelope,
        ReceiptSource::DayEnd(source_index),
        0,
        ReceiptKind::Release,
    )
}

fn source_index(indices: &BTreeMap<EnvelopeKey, u32>, key: &EnvelopeKey) -> Result<u32, StepFatal> {
    indices
        .get(key)
        .copied()
        .ok_or_else(|| state_invariant("auction envelope has no source index"))
}

fn fill_receipt(
    order: &mut AuctionOrder,
    qty: u32,
    price: Money,
    source: ReceiptSource,
    ordinal: u64,
    config: &GameConfig,
) -> Result<EnvelopeReceipt, StepFatal> {
    let before = order.envelope.audit();
    let remaining_qty_after = before
        .remaining_qty
        .checked_sub(qty)
        .ok_or_else(|| state_invariant("auction fill exceeds remaining quantity"))?;
    let gross = price.mul_shares(qty).map_err(money_invariant)?;
    let side = order.envelope.key().side;
    let transition = match side {
        Side::Buy => FillTransition::buy(BuyFillInput {
            config,
            limit: before.limit,
            fill_qty: qty,
            remaining_qty_after,
            filled_value_before: before.filled_value,
            gross_delta: gross,
            live_before: order.envelope.live(),
        })?,
        Side::Sell => FillTransition::sell(SellFillInput {
            config,
            fill_qty: qty,
            remaining_qty_after,
            filled_value_before: before.filled_value,
            gross_delta: gross,
            nominal_before: before.nominal,
            charged_before: before.charged,
        })?,
    };
    let value_after = before.filled_value.add(gross).map_err(money_invariant)?;
    let nominal_after = fee_add(before.nominal, transition.nominal)?;
    if side == Side::Buy
        && (nominal_after != transition.nominal_after || before.charged != before.nominal)
    {
        return Err(state_invariant(
            "buyer fee audit disagrees with cumulative filled value",
        ));
    }
    let audit_after = super::EnvelopeAudit {
        limit: before.limit,
        remaining_qty: remaining_qty_after,
        filled_qty: before
            .filled_qty
            .checked_add(qty)
            .ok_or_else(|| state_invariant("auction filled quantity overflow"))?,
        filled_value: value_after,
        nominal: nominal_after,
        charged: transition.charged_after,
    };
    let key = order.envelope.key().clone();
    let receipt = EnvelopeReceipt {
        index: 0,
        local_key: local_key(source, key.clone(), ordinal)?,
        envelope: key,
        kind: ReceiptKind::Fill,
        qty_before: before.remaining_qty,
        qty_after: remaining_qty_after,
        value_before: before.filled_value,
        value_after,
        delta: transition.delta,
        nominal: transition.nominal,
        charged: transition.charged,
        charged_before: before.charged,
        charged_after: transition.charged_after,
        deliver_qty: transition.deliver_qty,
        deliver_cash: transition.deliver_cash,
    };
    order.envelope.apply(receipt.delta, audit_after)?;
    Ok(receipt)
}

fn rollover_receipt(
    order: &AuctionOrder,
    source: ReceiptSource,
    ordinal: u64,
) -> Result<EnvelopeReceipt, StepFatal> {
    let audit = order.envelope.audit();
    let key = order.envelope.key().clone();
    Ok(EnvelopeReceipt {
        index: 0,
        local_key: local_key(source, key.clone(), ordinal)?,
        envelope: key,
        kind: ReceiptKind::Rollover,
        qty_before: audit.remaining_qty,
        qty_after: audit.remaining_qty,
        value_before: audit.filled_value,
        value_after: audit.filled_value,
        delta: ReceiptDelta::sealed(ResVec::ZERO, ResVec::ZERO, order.envelope.live()),
        nominal: FeeComponents::ZERO,
        charged: FeeComponents::ZERO,
        charged_before: audit.charged,
        charged_after: audit.charged,
        deliver_qty: 0,
        deliver_cash: Money::ZERO,
    })
}

fn release_receipt(
    order: &AuctionOrder,
    source: ReceiptSource,
    ordinal: u64,
) -> Result<EnvelopeReceipt, StepFatal> {
    terminal_receipt(&order.envelope, source, ordinal, ReceiptKind::Release)
}

pub(super) fn reject_receipt(
    order: &AuctionOrder,
    sealed_index: u64,
) -> Result<EnvelopeReceipt, StepFatal> {
    terminal_receipt(
        &order.envelope,
        ReceiptSource::SealedIntent(sealed_index),
        0,
        ReceiptKind::Reject,
    )
}

fn terminal_receipt(
    envelope: &Envelope,
    source: ReceiptSource,
    ordinal: u64,
    kind: ReceiptKind,
) -> Result<EnvelopeReceipt, StepFatal> {
    let audit = envelope.audit();
    let key = envelope.key().clone();
    Ok(EnvelopeReceipt {
        index: 0,
        local_key: local_key(source, key.clone(), ordinal)?,
        envelope: key,
        kind,
        qty_before: audit.remaining_qty,
        qty_after: audit.remaining_qty,
        value_before: audit.filled_value,
        value_after: audit.filled_value,
        delta: ReceiptDelta::sealed(ResVec::ZERO, envelope.live(), ResVec::ZERO),
        nominal: FeeComponents::ZERO,
        charged: FeeComponents::ZERO,
        charged_before: audit.charged,
        charged_after: audit.charged,
        deliver_qty: 0,
        deliver_cash: Money::ZERO,
    })
}

fn apply_last_delta(order: &mut AuctionOrder, receipt: &EnvelopeReceipt) -> Result<(), StepFatal> {
    order.envelope.apply(receipt.delta, order.envelope.audit())
}

fn local_key(
    source: ReceiptSource,
    envelope: EnvelopeKey,
    ordinal: u64,
) -> Result<ReceiptLocalKey, StepFatal> {
    ReceiptLocalKey::new(
        JournalRank::SealedBatch,
        source,
        ReceiptTransition { envelope, ordinal },
    )
}

fn next_ordinal(ordinal: u64) -> Result<u64, StepFatal> {
    ordinal
        .checked_add(1)
        .ok_or_else(|| state_invariant("auction receipt ordinal overflow"))
}

fn fee_add(left: FeeComponents, right: FeeComponents) -> Result<FeeComponents, StepFatal> {
    Ok(FeeComponents {
        commission: left
            .commission
            .add(right.commission)
            .map_err(money_invariant)?,
        stamp_tax: left
            .stamp_tax
            .add(right.stamp_tax)
            .map_err(money_invariant)?,
        transfer_fee: left
            .transfer_fee
            .add(right.transfer_fee)
            .map_err(money_invariant)?,
    })
}

fn money_invariant(error: MoneyError) -> StepFatal {
    state_invariant(&error.to_string())
}

fn state_invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::stock_auction".to_owned(),
    }
}

/// Stock-local input needed to select an A-share call-auction clearing price.
///
/// Quantity is widened to `u64` at the boundary so the selector can check its
/// aggregation limits without depending on an order-book storage type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ClearingOrder {
    pub(super) side: Side,
    pub(super) limit: Money,
    pub(super) qty: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ClearingSelection {
    pub(super) price: Money,
    pub(super) volume: u64,
    pub(super) imbalance: u64,
}

/// Selects one clearing result without mutating an order queue or account.
///
pub(super) fn select_clearing(
    orders: &[ClearingOrder],
    previous_close: Money,
    exchange: StockExchange,
    price_tick: Money,
) -> Result<Option<ClearingSelection>, StepFatal> {
    let candidates: BTreeSet<Money> = orders.iter().map(|order| order.limit).collect();
    let mut stats = Vec::with_capacity(candidates.len());
    for price in candidates {
        if let Some(candidate) = candidate_stats(orders, price)? {
            stats.push(candidate);
        }
    }
    let Some(max_volume) = stats.iter().map(|candidate| candidate.result.volume).max() else {
        return Ok(None);
    };
    stats.retain(|candidate| candidate.result.volume == max_volume);

    match exchange {
        StockExchange::Shanghai => {
            let min_imbalance = stats
                .iter()
                .map(|candidate| candidate.result.imbalance)
                .min()
                .ok_or_else(|| invariant("Shanghai candidates disappeared after volume filter"))?;
            stats.retain(|candidate| candidate.result.imbalance == min_imbalance);
            let low = stats
                .iter()
                .map(|candidate| candidate.result.price)
                .min()
                .ok_or_else(|| invariant("Shanghai candidates have no lowest price"))?;
            let high = stats
                .iter()
                .map(|candidate| candidate.result.price)
                .max()
                .ok_or_else(|| invariant("Shanghai candidates have no highest price"))?;
            let midpoint = rounded_midpoint(low, high, price_tick)?;
            Ok(candidate_stats(orders, midpoint)?.map(|candidate| candidate.result))
        }
        StockExchange::Shenzhen => {
            let min_difference = stats
                .iter()
                .map(|candidate| candidate.aggressive_difference)
                .min()
                .ok_or_else(|| invariant("Shenzhen candidates disappeared after volume filter"))?;
            Ok(stats
                .into_iter()
                .filter(|candidate| candidate.aggressive_difference == min_difference)
                .min_by_key(|candidate| {
                    (
                        i128::from(candidate.result.price.cents())
                            .abs_diff(i128::from(previous_close.cents())),
                        candidate.result.price,
                    )
                })
                .map(|candidate| candidate.result))
        }
    }
}

/// Computes the no-cross indicative imbalance with checked quantity sums.
///
pub(super) fn checked_total_imbalance(orders: &[ClearingOrder]) -> Result<u64, StepFatal> {
    let buy = checked_sum(
        orders
            .iter()
            .filter(|order| order.side == Side::Buy)
            .map(|order| order.qty),
        "buy",
        "pipeline::stock_auction::checked_total_imbalance",
        "auction indicative imbalance quantity overflow",
    )?;
    let sell = checked_sum(
        orders
            .iter()
            .filter(|order| order.side == Side::Sell)
            .map(|order| order.qty),
        "sell",
        "pipeline::stock_auction::checked_total_imbalance",
        "auction indicative imbalance quantity overflow",
    )?;
    Ok(buy.abs_diff(sell))
}

#[derive(Clone, Copy)]
struct CandidateStats {
    result: ClearingSelection,
    aggressive_difference: u64,
}

fn candidate_stats(
    orders: &[ClearingOrder],
    price: Money,
) -> Result<Option<CandidateStats>, StepFatal> {
    let sum = |side, predicate: &dyn Fn(Money) -> bool, label| {
        checked_sum(
            orders
                .iter()
                .filter(|order| order.side == side && predicate(order.limit))
                .map(|order| order.qty),
            label,
            "pipeline::stock_auction::select_clearing",
            "auction candidate quantity overflow",
        )
    };
    let buy = sum(Side::Buy, &|limit| limit >= price, "eligible buy")?;
    let sell = sum(Side::Sell, &|limit| limit <= price, "eligible sell")?;
    let volume = buy.min(sell);
    if volume == 0 {
        return Ok(None);
    }
    let aggressive_buy = sum(Side::Buy, &|limit| limit > price, "price-better buy")?;
    let aggressive_sell = sum(Side::Sell, &|limit| limit < price, "price-better sell")?;
    if aggressive_buy > volume || aggressive_sell > volume {
        return Ok(None);
    }
    Ok(Some(CandidateStats {
        result: ClearingSelection {
            price,
            volume,
            imbalance: buy.abs_diff(sell),
        },
        aggressive_difference: aggressive_buy.abs_diff(aggressive_sell),
    }))
}

fn checked_sum(
    quantities: impl IntoIterator<Item = u64>,
    label: &str,
    location: &'static str,
    description: &'static str,
) -> Result<u64, StepFatal> {
    quantities.into_iter().try_fold(0_u64, |total, qty| {
        total
            .checked_add(qty)
            .ok_or_else(|| StepFatal::InvariantViolation {
                description: format!("{description} while summing {label}"),
                location: location.to_owned(),
            })
    })
}

fn rounded_midpoint(low: Money, high: Money, tick: Money) -> Result<Money, StepFatal> {
    let tick_cents = i128::from(tick.cents());
    if tick_cents <= 0 {
        return Err(invariant("auction price tick must be positive"));
    }
    let sum = i128::from(low.cents())
        .checked_add(i128::from(high.cents()))
        .ok_or_else(|| invariant("Shanghai midpoint sum overflow"))?;
    let denominator = tick_cents
        .checked_mul(2)
        .ok_or_else(|| invariant("Shanghai midpoint denominator overflow"))?;
    let rounded_ticks = sum
        .checked_add(tick_cents)
        .and_then(|value| value.checked_div(denominator))
        .ok_or_else(|| invariant("Shanghai midpoint rounding overflow"))?;
    let cents = rounded_ticks
        .checked_mul(tick_cents)
        .ok_or_else(|| invariant("Shanghai midpoint price overflow"))?;
    let cents =
        i64::try_from(cents).map_err(|_| invariant("Shanghai midpoint does not fit Money"))?;
    Ok(Money::from_cents(cents))
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::stock_auction::select_clearing".to_owned(),
    }
}
