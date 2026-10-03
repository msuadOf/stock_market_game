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

/// 普通 NPC 报价生命周期的窄编辑器，借用 GameSession 保存的原始 Vec。
pub(in crate::session) struct NpcOrderLifecycleBook<'a> {
    lifecycles: &'a mut Vec<crate::session::NpcOrderLifecycle>,
}

impl<'a> NpcOrderLifecycleBook<'a> {
    pub(in crate::session) fn new(
        lifecycles: &'a mut Vec<crate::session::NpcOrderLifecycle>,
    ) -> Self {
        Self { lifecycles }
    }

    pub(in crate::session) fn ensure_order_absent(&self, order: crate::OrderId) {
        if self
            .lifecycles
            .iter()
            .any(|lifecycle| lifecycle.order_id == order)
        {
            panic!("NPC quote lifecycle already exists for order {}", order.0);
        }
    }

    pub(in crate::session) fn append_prepared(
        &mut self,
        lifecycle: crate::session::NpcOrderLifecycle,
    ) {
        self.lifecycles.push(lifecycle);
    }

    pub(in crate::session) fn remove(
        &mut self,
        account: crate::AccountId,
        code: &crate::StockCode,
        order: crate::OrderId,
    ) {
        self.lifecycles.retain(|lifecycle| {
            !(lifecycle.account == account
                && lifecycle.code == *code
                && lifecycle.order_id == order)
        });
    }

    pub(in crate::session) fn due_at(
        &self,
        market_minute: u64,
    ) -> impl Iterator<Item = &crate::session::NpcOrderLifecycle> {
        self.lifecycles
            .iter()
            .filter(move |lifecycle| lifecycle.expires_market_minute <= market_minute)
    }

    pub(in crate::session) fn clear(&mut self) {
        self.lifecycles.clear();
    }
}

impl ExpiryOutput {
    pub(super) fn record_release(&mut self, release: ExpiryRelease) -> Result<(), StepFatal> {
        let total = self
            .released_by_account
            .entry(release.account)
            .or_insert(ResVec::ZERO);
        *total = total.checked_add(release.resources)?;
        self.releases.push(release);
        Ok(())
    }
}

pub(super) fn plan_expiry(shadow: &mut TickShadowPlan) -> Result<ExpiryOutput, StepFatal> {
    if shadow.expiry_applied {
        return Err(invariant("P0 expiry was applied more than once"));
    }
    let (output, events, receipts) = shadow.state.execute(GameSession::apply_quote_expiry)?;
    let mut next_by_account = BTreeMap::new();
    for event in &events {
        let Event::OrderCanceled { account, .. } = event else {
            return Err(invariant("P0 emitted a non-cancellation event"));
        };
        let index = next_by_account.entry(*account).or_insert(0_u64);
        let local_index = super::event_key::QUOTE_EXPIRY_EVENT_INDEX_BASE
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
    fn apply_quote_expiry(
        &mut self,
    ) -> Result<(ExpiryOutput, Vec<Event>, Vec<EnvelopeReceipt>), StepFatal> {
        // TickShadow 已拥有隔离的 candidate，失败由外层丢弃；P0 建立供 P1 分配的完整 live envelope 视图。
        self.hydrate_or_validate_envelope_ledger()?;
        if self.phase() != crate::TradingPhase::Continuous {
            return Ok((ExpiryOutput::default(), Vec::new(), Vec::new()));
        }
        let market_minute = self.current_market_minute();
        let mut expired: Vec<_> = NpcOrderLifecycleBook::new(&mut self.state.npc_order_lifecycles)
            .due_at(market_minute)
            .cloned()
            .collect();
        if expired.is_empty() {
            return Ok((ExpiryOutput::default(), Vec::new(), Vec::new()));
        }
        expired
            .sort_by(|left, right| (&left.code, left.order_id).cmp(&(&right.code, right.order_id)));

        let mut prepared = Vec::with_capacity(expired.len());
        for (source_index, lifecycle) in expired.iter().enumerate() {
            let source_index =
                u32::try_from(source_index).map_err(|_| invariant("P0 expiry index overflow"))?;
            let envelope = self
                .state
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
        self.state.envelope_ledger.apply(&mut receipts)?;
        let terminal_keys: Vec<_> = prepared
            .iter()
            .map(|(_, envelope, _)| envelope.clone())
            .collect();
        self.state.envelope_ledger.remove_terminal(&terminal_keys)?;
        self.state.next_receipt_base = self.state.envelope_ledger.next_receipt_index();

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
            output.record_release(ExpiryRelease {
                receipt_index: receipt.index,
                account: fact.account,
                stock: fact.code.clone(),
                order_id: fact.order_id,
                side: fact.side,
                resources: receipt.delta.released,
            })?;
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
            ReceiptSource::QuoteExpiry(source_index),
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
        location: "pipeline::quote_expiry".to_owned(),
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
        location: "pipeline::quote_expiry".to_owned(),
    }
}

#[cfg(test)]
mod ownership_tests {
    use super::*;

    fn lifecycle(
        account: u64,
        code: &str,
        order: u64,
        expires: u64,
    ) -> crate::session::NpcOrderLifecycle {
        crate::session::NpcOrderLifecycle {
            account: crate::AccountId(account),
            code: crate::StockCode(code.to_owned()),
            order_id: crate::OrderId(order),
            placed_market_minute: 1,
            expires_market_minute: expires,
        }
    }

    #[test]
    fn lifecycle_book_removal_uses_all_three_identity_fields_and_due_keeps_order() {
        let mut values = vec![
            lifecycle(1, "600888", 1, 5),
            lifecycle(2, "600888", 1, 4),
            lifecycle(1, "600999", 1, 6),
        ];
        let mut book = NpcOrderLifecycleBook::new(&mut values);
        assert_eq!(
            book.due_at(5)
                .map(|entry| entry.account)
                .collect::<Vec<_>>(),
            vec![crate::AccountId(1), crate::AccountId(2)]
        );
        book.remove(
            crate::AccountId(1),
            &crate::StockCode("600888".to_owned()),
            crate::OrderId(1),
        );
        book.remove(
            crate::AccountId(99),
            &crate::StockCode("600888".to_owned()),
            crate::OrderId(1),
        );
        assert_eq!(
            book.due_at(6)
                .map(|entry| entry.account)
                .collect::<Vec<_>>(),
            vec![crate::AccountId(2), crate::AccountId(1)]
        );
        book.clear();
        assert_eq!(book.due_at(u64::MAX).count(), 0);
    }

    #[test]
    #[should_panic(expected = "NPC quote lifecycle already exists for order 1")]
    fn lifecycle_book_duplicate_order_is_global_across_account_and_stock() {
        let mut values = vec![lifecycle(1, "600888", 1, 5)];
        NpcOrderLifecycleBook::new(&mut values).ensure_order_absent(crate::OrderId(1));
    }

    fn release(account: u64, shares: u32, cash: i64) -> ExpiryRelease {
        ExpiryRelease {
            receipt_index: 0,
            account: crate::AccountId(account),
            stock: crate::StockCode("600888".to_owned()),
            order_id: crate::OrderId(1),
            side: crate::Side::Buy,
            resources: ResVec::new(crate::Money::from_cents(cash), shares),
        }
    }

    #[test]
    fn expiry_release_aggregation_keeps_resource_units_and_failed_append_boundary() {
        let mut output = ExpiryOutput::default();
        output.record_release(release(1, u32::MAX, 4)).unwrap();
        let error = output.record_release(release(1, 1, 5)).unwrap_err();
        assert_eq!(
            error,
            StepFatal::InvariantViolation {
                description: "shares add".to_owned(),
                location: "pipeline::ResVec".to_owned()
            }
        );
        assert_eq!(output.releases.len(), 1);
        assert_eq!(
            output.released_by_account[&crate::AccountId(1)],
            ResVec::new(crate::Money::from_cents(4), u32::MAX)
        );
        output.record_release(release(2, 1, 9)).unwrap();
        assert_eq!(output.releases.len(), 2);
        assert_eq!(
            output.released_by_account[&crate::AccountId(2)],
            ResVec::new(crate::Money::from_cents(9), 1)
        );
    }
}
