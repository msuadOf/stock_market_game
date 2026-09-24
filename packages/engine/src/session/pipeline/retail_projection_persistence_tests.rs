use super::p5_receipts::apply_session_receipt_transaction;
use super::p6_transaction::apply_session_p6_transaction;
use super::retail_projection::{
    canonical_unseen_receipts, RetailProjectionError, RetailProjectionSeen,
};
use super::{
    Envelope, EnvelopeAudit, EnvelopeKey, EnvelopeLedger, EnvelopeReceipt, FeeComponents,
    JournalRank, ReceiptDelta, ReceiptKind, ReceiptLocalKey, ReceiptSource, ReceiptTransition,
    ResVec,
};
use crate::{AccountId, GameSession, Money, OrderId, SaveDecodeLimits, Side, StockCode};

fn receipt(index: u64, order: u64) -> EnvelopeReceipt {
    receipt_for_stock(index, order, StockCode("600001".to_owned()))
}

fn receipt_for_stock(index: u64, order: u64, stock: StockCode) -> EnvelopeReceipt {
    let envelope = EnvelopeKey {
        account: AccountId(1),
        stock,
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

#[test]
fn restored_prefix_ignores_old_receipt_and_accepts_next_new_identity() {
    let old = receipt(0, 10);
    let new = receipt(1, 11);
    let seen = RetailProjectionSeen::from_authoritative_identities(
        [(old.index, old.local_key.clone())],
        1,
    )
    .expect("a contiguous restored receipt prefix must be valid");

    let receipts = [old, new.clone()];
    let unseen = canonical_unseen_receipts(&receipts, &seen)
        .expect("the old identity must be ignored while the new identity remains consumable");

    assert_eq!(unseen.len(), 1);
    assert_eq!(unseen[0].index, 1);
    assert_eq!(unseen[0].local_key, new.local_key);
}

#[test]
fn seen_index_shares_old_history_and_keeps_sparse_high_indices_sorted() {
    let old = receipt(0, 10);
    let middle = receipt(16, 11);
    let high = receipt(1_u64 << 60, 12);
    let mut source = RetailProjectionSeen::default();
    source.insert_test_identity(high.index, high.local_key.clone());
    source.insert_test_identity(old.index, old.local_key.clone());
    let mut candidate = source.clone();
    candidate.insert_test_identity(middle.index, middle.local_key.clone());

    assert_eq!(source.len(), 2);
    assert_eq!(candidate.len(), 3);
    assert_eq!(
        candidate.authoritative_identities(),
        vec![
            (old.index, old.local_key.clone()),
            (middle.index, middle.local_key.clone()),
            (high.index, high.local_key.clone()),
        ]
    );
    assert_eq!(
        canonical_unseen_receipts(&[middle.clone()], &source).unwrap()[0].index,
        middle.index
    );
    assert!(canonical_unseen_receipts(&[middle], &candidate)
        .unwrap()
        .is_empty());
    assert_eq!(
        serde_json::to_value(&candidate).unwrap()["receipts"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn save_decode_restore_preserves_seen_prefix_and_consumes_only_the_next_receipt() {
    let mut source = GameSession::new(
        crate::session::npc_working_quote_tests::retail_quote_setup(),
        44,
    )
    .expect("v2 retail fixture must be valid");
    let code = source.setup.stocks[0].code.clone();
    let old = receipt_for_stock(0, 1, code.clone());
    source.retail_projection_seen = RetailProjectionSeen::from_authoritative_identities(
        [(old.index, old.local_key.clone())],
        1,
    )
    .expect("one committed receipt must form a complete prefix");
    source.next_receipt_base = 1;
    source.next_order_id = 3;
    source.envelope_ledger = EnvelopeLedger::new(1, []).unwrap();

    let save = source
        .save()
        .expect("non-empty seen prefix must be saveable");
    assert_eq!(save.runtime_v2.retail_projection_seen.len(), 1);
    let bytes = serde_json::to_vec(&save).unwrap();
    let decoded = crate::session::decode_save_slot(&bytes, &SaveDecodeLimits::default())
        .expect("complete SaveSlot bytes must decode");
    let mut restored = GameSession::restore(&decoded)
        .expect("complete SaveSlot must restore the non-empty seen prefix");
    assert_eq!(
        restored
            .retail_projection_seen
            .authoritative_identities()
            .iter()
            .map(|(index, _)| *index)
            .collect::<Vec<_>>(),
        vec![0],
    );

    let next_key = EnvelopeKey {
        account: AccountId(1),
        stock: code.clone(),
        order: OrderId(2),
        side: Side::Buy,
    };
    let created = Envelope::p3_created(
        next_key.clone(),
        Money::from_cents(100_100),
        0,
        EnvelopeAudit {
            limit: Money::from_cents(1_000),
            remaining_qty: 100,
            filled_qty: 0,
            filled_value: Money::ZERO,
            nominal: FeeComponents::ZERO,
            charged: FeeComponents::ZERO,
        },
    );
    let candidate = receipt_for_stock(0, 2, code.clone());
    let committed = apply_session_receipt_transaction(
        &mut restored,
        vec![created],
        vec![vec![candidate]],
        vec![next_key],
    )
    .expect("P5 must allocate from the restored cursor");
    assert_eq!(committed[0].index, 1, "receipt index must not be reused");
    assert_eq!(restored.next_receipt_base, 2);

    let cash_before = restored.accounts[&AccountId(1)].cash;
    let first = apply_session_p6_transaction(&mut restored, &[old.clone(), committed[0].clone()])
        .expect("P6 must ignore the restored old identity and consume the next one");
    assert_eq!(first.settlement.applied_receipts, 1);
    assert_eq!(first.events.len(), 1);
    assert_eq!(restored.accounts[&AccountId(1)].positions[&code].qty, 100);
    assert_eq!(
        restored.accounts[&AccountId(1)].cash,
        cash_before.sub(Money::from_cents(100_100)).unwrap(),
    );
    assert_eq!(
        restored
            .retail_projection_seen
            .authoritative_identities()
            .iter()
            .map(|(index, _)| *index)
            .collect::<Vec<_>>(),
        vec![0, 1],
    );

    let after_first = restored.business_state_hash().unwrap();
    let replay = apply_session_p6_transaction(&mut restored, &[old, committed[0].clone()])
        .expect("exact replays must be idempotently ignored");
    assert_eq!(replay.settlement.applied_receipts, 0);
    assert!(replay.events.is_empty());
    assert_eq!(restored.business_state_hash().unwrap(), after_first);

    restored
        .envelope_ledger
        .rebase_live_for_next_tick()
        .expect("terminal tick evidence must rebase at the quiet point");
    let continued = restored
        .save()
        .expect("continued receipt prefix must remain saveable");
    assert_eq!(continued.runtime_v2.next_receipt_base, 2);
    assert_eq!(continued.runtime_v2.retail_projection_seen.len(), 2);
}
