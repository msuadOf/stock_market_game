use super::{
    Envelope, EnvelopeAudit, EnvelopeKey, EnvelopeLedger, EnvelopeReceipt, FeeComponents,
    JournalRank, ReceiptDelta, ReceiptKind, ReceiptLocalKey, ReceiptSource, ReceiptTransition,
    ResVec,
};
use crate::{AccountId, Money, OrderId, Side, StockCode};

pub(super) fn key() -> EnvelopeKey {
    EnvelopeKey {
        account: AccountId(1),
        stock: StockCode("600001".to_owned()),
        order: OrderId(1),
        side: Side::Buy,
    }
}
pub(super) fn second_key() -> EnvelopeKey {
    EnvelopeKey {
        account: AccountId(1),
        stock: StockCode("600002".to_owned()),
        order: OrderId(2),
        side: Side::Sell,
    }
}
pub(super) fn audit() -> EnvelopeAudit {
    EnvelopeAudit {
        limit: Money::ZERO,
        remaining_qty: 0,
        filled_qty: 0,
        filled_value: Money::ZERO,
        nominal: FeeComponents::ZERO,
        charged: FeeComponents::ZERO,
    }
}
pub(super) fn audit_with_remaining(remaining_qty: u32) -> EnvelopeAudit {
    EnvelopeAudit {
        remaining_qty,
        ..audit()
    }
}
pub(super) fn existing(key: EnvelopeKey, cash: Money) -> Envelope {
    Envelope::tick_start_existing(key, cash, 0, audit())
}
pub(super) fn extra() -> Envelope {
    existing(
        EnvelopeKey {
            account: AccountId(2),
            stock: StockCode("600003".to_owned()),
            order: OrderId(3),
            side: Side::Buy,
        },
        Money::ZERO,
    )
}
pub(super) fn created_ledger() -> EnvelopeLedger {
    EnvelopeLedger::new(
        7,
        [Envelope::p3_created(
            key(),
            Money::from_cents(100),
            0,
            audit_with_remaining(100),
        )],
    )
    .unwrap()
}
pub(super) fn receipt(ordinal: u64) -> EnvelopeReceipt {
    sealed_receipt(
        key(),
        ReceiptDelta::sealed(
            ResVec::new(Money::from_cents(100), 0),
            ResVec::ZERO,
            ResVec::ZERO,
        ),
        ordinal,
    )
}
pub(super) fn expiry_release(key: EnvelopeKey, released: ResVec) -> EnvelopeReceipt {
    EnvelopeReceipt {
        index: 0,
        local_key: ReceiptLocalKey::new(
            JournalRank::PreSeal,
            ReceiptSource::P0Expiry(0),
            ReceiptTransition {
                envelope: key.clone(),
                ordinal: 0,
            },
        )
        .unwrap(),
        envelope: key,
        kind: ReceiptKind::Release,
        qty_before: 0,
        qty_after: 0,
        value_before: Money::ZERO,
        value_after: Money::ZERO,
        delta: ReceiptDelta::sealed(ResVec::ZERO, released, ResVec::ZERO),
        nominal: FeeComponents::ZERO,
        charged: FeeComponents::ZERO,
        charged_before: FeeComponents::ZERO,
        charged_after: FeeComponents::ZERO,
        deliver_qty: 0,
        deliver_cash: Money::ZERO,
    }
}
pub(super) fn sealed_receipt(
    key: EnvelopeKey,
    delta: ReceiptDelta,
    ordinal: u64,
) -> EnvelopeReceipt {
    let (deliver_qty, deliver_cash) = if key.side == Side::Buy {
        (100, Money::ZERO)
    } else {
        (0, Money::from_cents(100))
    };
    EnvelopeReceipt {
        index: 0,
        local_key: ReceiptLocalKey::new(
            JournalRank::SealedBatch,
            ReceiptSource::SealedIntent(0),
            ReceiptTransition {
                envelope: key.clone(),
                ordinal,
            },
        )
        .unwrap(),
        envelope: key,
        kind: ReceiptKind::Fill,
        qty_before: 100,
        qty_after: 0,
        value_before: Money::ZERO,
        value_after: Money::from_cents(100),
        delta,
        nominal: FeeComponents::ZERO,
        charged: FeeComponents::ZERO,
        charged_before: FeeComponents::ZERO,
        charged_after: FeeComponents::ZERO,
        deliver_qty,
        deliver_cash,
    }
}
pub(super) struct ReceiptFixture {
    pub(super) key: EnvelopeKey,
    pub(super) source: ReceiptSource,
    pub(super) kind: ReceiptKind,
    pub(super) qty_before: u32,
    pub(super) qty_after: u32,
    pub(super) value_before: Money,
    pub(super) value_after: Money,
    pub(super) delta: ReceiptDelta,
}
pub(super) fn receipt_from_source(fixture: ReceiptFixture) -> EnvelopeReceipt {
    EnvelopeReceipt {
        index: 0,
        local_key: ReceiptLocalKey::new(
            fixture.source.journal(),
            fixture.source,
            ReceiptTransition {
                envelope: fixture.key.clone(),
                ordinal: 0,
            },
        )
        .unwrap(),
        envelope: fixture.key,
        kind: fixture.kind,
        qty_before: fixture.qty_before,
        qty_after: fixture.qty_after,
        value_before: fixture.value_before,
        value_after: fixture.value_after,
        delta: fixture.delta,
        nominal: FeeComponents::ZERO,
        charged: FeeComponents::ZERO,
        charged_before: FeeComponents::ZERO,
        charged_after: FeeComponents::ZERO,
        deliver_qty: 0,
        deliver_cash: Money::ZERO,
    }
}
