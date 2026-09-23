use super::retail_projection::{
    canonical_unseen_receipts, RetailProjectionError, RetailProjectionSeen,
};
use super::{
    EnvelopeKey, EnvelopeReceipt, FeeComponents, JournalRank, ReceiptDelta, ReceiptKind,
    ReceiptLocalKey, ReceiptSource, ReceiptTransition, ResVec,
};
use crate::{AccountId, Money, OrderId, Side, StockCode};

fn receipt(index: u64, order: u64) -> EnvelopeReceipt {
    let envelope = EnvelopeKey {
        account: AccountId(1),
        stock: StockCode("600001".to_owned()),
        order: OrderId(order),
        side: Side::Buy,
    };
    EnvelopeReceipt {
        index,
        local_key: ReceiptLocalKey::new(
            JournalRank::SealedBatch,
            ReceiptSource::SealedIntent(index),
            ReceiptTransition {
                envelope: envelope.clone(),
                ordinal: 0,
            },
        )
        .unwrap(),
        envelope,
        kind: ReceiptKind::Fill,
        qty_before: 100,
        qty_after: 0,
        value_before: Money::ZERO,
        value_after: Money::from_cents(100_000),
        delta: ReceiptDelta::sealed(
            ResVec::new(Money::from_cents(100_100), 0),
            ResVec::ZERO,
            ResVec::ZERO,
        ),
        nominal: FeeComponents {
            commission: Money::from_cents(100),
            ..FeeComponents::ZERO
        },
        charged: FeeComponents {
            commission: Money::from_cents(100),
            ..FeeComponents::ZERO
        },
        charged_before: FeeComponents::ZERO,
        charged_after: FeeComponents {
            commission: Money::from_cents(100),
            ..FeeComponents::ZERO
        },
        deliver_qty: 100,
        deliver_cash: Money::ZERO,
    }
}

#[test]
fn restored_seen_index_with_a_different_local_key_is_a_conflict() {
    let previously_committed = receipt(0, 10);
    let conflicting_replay = receipt(0, 11);
    let mut seen = RetailProjectionSeen::default();
    seen.insert_test_identity(0, previously_committed.local_key);

    assert_eq!(
        canonical_unseen_receipts(&[conflicting_replay], &seen).unwrap_err(),
        RetailProjectionError::ConflictingIdentity { index: 0 }
    );
}

#[test]
fn one_batch_cannot_reuse_a_receipt_index_for_two_local_keys() {
    let first = receipt(0, 10);
    let second = receipt(0, 11);

    assert_eq!(
        canonical_unseen_receipts(&[first, second], &RetailProjectionSeen::default()).unwrap_err(),
        RetailProjectionError::ConflictingIdentity { index: 0 }
    );
}
