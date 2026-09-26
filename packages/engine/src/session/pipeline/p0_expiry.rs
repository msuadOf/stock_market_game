use super::*;
use crate::session::continuous_cancellation::ContinuousCancellationCause;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpiryRelease {
    pub receipt_index: u64,
    pub account: crate::AccountId,
    pub stock: crate::StockCode,
    pub order_id: crate::OrderId,
    pub side: crate::Side,
    pub resources: ResVec,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExpiryOutput {
    pub releases: Vec<ExpiryRelease>,
    pub released_by_account: BTreeMap<crate::AccountId, ResVec>,
}

pub(super) fn plan_expiry(shadow: &mut TickShadowPlan) -> Result<ExpiryOutput, StepFatal> {
    if shadow.expiry_applied {
        return Err(invariant("P0 expiry was applied more than once"));
    }
    let (output, events, receipts) = shadow.state.execute(GameSession::apply_p0_expiry)?;
    let mut next_by_account = BTreeMap::new();
    for event in &events {
        let Event::OrderCanceled { account, .. } = event else {
            return Err(invariant("P0 emitted a non-cancellation event"));
        };
        let index = next_by_account.entry(*account).or_insert(0_u64);
        let local_index = super::event_key::P0_EVENT_INDEX_BASE
            .checked_add(*index)
            .ok_or_else(|| invariant("P0 event identity overflow"))?;
        shadow
            .event_keys
            .push(super::EventStableKey::for_event(event, local_index));
        *index = index
            .checked_add(1)
            .ok_or_else(|| invariant("P0 account event count overflow"))?;
    }
    shadow.event_outbox.extend(events);
    shadow
        .receipt_keys
        .extend(receipts.iter().map(|receipt| receipt.local_key.clone()));
    shadow.applied_receipts.extend(receipts);
    shadow.expiry_applied = true;
    Ok(output)
}

impl GameSession {
    fn apply_p0_expiry(
        &mut self,
    ) -> Result<(ExpiryOutput, Vec<Event>, Vec<EnvelopeReceipt>), StepFatal> {
        // TickShadow already owns an isolated candidate. A failure drops that candidate,
        // so cloning the whole session again here adds no authority protection.
        // P0 establishes the complete live-envelope view used by P1 allocation.
        self.hydrate_or_validate_envelope_ledger()?;
        if self.phase() != crate::TradingPhase::Continuous {
            return Ok((ExpiryOutput::default(), Vec::new(), Vec::new()));
        }
        let market_minute = self.current_market_minute();
        if !self
            .npc_order_lifecycles
            .iter()
            .any(|lifecycle| lifecycle.expires_market_minute <= market_minute)
        {
            return Ok((ExpiryOutput::default(), Vec::new(), Vec::new()));
        }
        let mut expired: Vec<_> = self
            .npc_order_lifecycles
            .iter()
            .filter(|lifecycle| lifecycle.expires_market_minute <= market_minute)
            .cloned()
            .collect();
        expired
            .sort_by(|left, right| (&left.code, left.order_id).cmp(&(&right.code, right.order_id)));

        let mut prepared = Vec::with_capacity(expired.len());
        for (source_index, lifecycle) in expired.iter().enumerate() {
            let source_index =
                u32::try_from(source_index).map_err(|_| invariant("P0 expiry index overflow"))?;
            let envelope = self
                .envelope_ledger
                .iter()
                .find(|(key, _)| {
                    key.account == lifecycle.account
                        && key.stock == lifecycle.code
                        && key.order == lifecycle.order_id
                })
                .map(|(key, envelope)| (key.clone(), envelope.clone()))
                .ok_or_else(|| invariant("expired lifecycle has no live envelope"))?;
            let receipt = expiry_receipt(envelope.0.clone(), &envelope.1, source_index)?;
            prepared.push((lifecycle.clone(), envelope.0, receipt));
        }
        let mut receipts: Vec<_> = prepared
            .iter()
            .map(|(_, _, receipt)| receipt.clone())
            .collect();
        self.envelope_ledger.apply(&mut receipts)?;
        let terminal_keys: Vec<_> = prepared
            .iter()
            .map(|(_, envelope, _)| envelope.clone())
            .collect();
        self.envelope_ledger.remove_terminal(&terminal_keys)?;
        self.next_receipt_base = self.envelope_ledger.next_receipt_index();

        let mut output = ExpiryOutput::default();
        let mut events = Vec::with_capacity(prepared.len());
        let applied_receipts = receipts.clone();
        let mut receipts_by_envelope: BTreeMap<_, _> = receipts
            .into_iter()
            .map(|receipt| (receipt.envelope.clone(), receipt))
            .collect();
        for (lifecycle, envelope, _) in prepared {
            let receipt = receipts_by_envelope
                .remove(&envelope)
                .ok_or_else(|| invariant("prepared P0 lifecycle has no applied receipt"))?;
            let fact = self
                .cancel_continuous_order_state_only(
                    lifecycle.account,
                    lifecycle.code,
                    lifecycle.order_id,
                    ContinuousCancellationCause::Expired,
                )
                .map_err(cancellation_failure)?;
            if fact.side != envelope.side {
                return Err(invariant("P0 cancellation side disagrees with envelope"));
            }
            let total = output
                .released_by_account
                .entry(fact.account)
                .or_insert(ResVec::ZERO);
            *total = total.checked_add(receipt.delta.released)?;
            output.releases.push(ExpiryRelease {
                receipt_index: receipt.index,
                account: fact.account,
                stock: fact.code.clone(),
                order_id: fact.order_id,
                side: fact.side,
                resources: receipt.delta.released,
            });
            events.push(Event::OrderCanceled {
                seq: self.next_seq(),
                account: fact.account,
                code: fact.code,
                id: fact.order_id,
                remaining_qty: fact.remaining_qty,
            });
        }
        Ok((output, events, applied_receipts))
    }
}

fn expiry_receipt(
    key: EnvelopeKey,
    envelope: &Envelope,
    source_index: u32,
) -> Result<EnvelopeReceipt, StepFatal> {
    let audit = envelope.audit();
    Ok(EnvelopeReceipt {
        index: 0,
        local_key: ReceiptLocalKey::new(
            JournalRank::PreSeal,
            ReceiptSource::P0Expiry(source_index),
            ReceiptTransition {
                envelope: key.clone(),
                ordinal: 0,
            },
        )?,
        envelope: key,
        kind: ReceiptKind::Release,
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
        deliver_cash: crate::Money::ZERO,
    })
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::p0_expiry".to_owned(),
    }
}

fn cancellation_failure(
    error: crate::session::continuous_cancellation::ContinuousCancellationError,
) -> StepFatal {
    let description = match error {
        crate::session::continuous_cancellation::ContinuousCancellationError::UnknownStock => {
            "P0 cancellation references an unknown stock".to_owned()
        }
        crate::session::continuous_cancellation::ContinuousCancellationError::OrderNotFound => {
            "P0 cancellation references a missing order".to_owned()
        }
        crate::session::continuous_cancellation::ContinuousCancellationError::OrderAlreadyFilled => {
            "P0 expiration references an order that has already filled".to_owned()
        }
        crate::session::continuous_cancellation::ContinuousCancellationError::NotOrderOwner => {
            "P0 cancellation lifecycle owner disagrees with order owner".to_owned()
        }
        crate::session::continuous_cancellation::ContinuousCancellationError::Market(error) => {
            error.to_string()
        }
    };
    StepFatal::InvariantViolation {
        description,
        location: "pipeline::p0_expiry".to_owned(),
    }
}
