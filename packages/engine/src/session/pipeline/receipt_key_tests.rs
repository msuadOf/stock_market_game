use super::{
    validate_indexed_receipt_keys, validate_receipt_keys, EnvelopeKey, IndexedReceiptKey,
    JournalRank, ReceiptIndex, ReceiptLocalKey, ReceiptSource, ReceiptTransition,
};
use crate::{AccountId, OrderId, Side, StockCode};

#[test]
fn receipt_keys_reject_duplicate_local_identity() -> Result<(), crate::session::StepFatal> {
    let key = receipt_key(ReceiptSource::SealedIntent(0), 0)?;

    assert!(validate_receipt_keys(&[key.clone(), key]).is_err());
    Ok(())
}

#[test]
fn receipt_keys_reject_non_contiguous_source_ordinal() -> Result<(), crate::session::StepFatal> {
    let keys = [
        receipt_key(ReceiptSource::SealedIntent(0), 0)?,
        receipt_key(ReceiptSource::SealedIntent(0), 2)?,
    ];

    assert!(validate_receipt_keys(&keys).is_err());
    Ok(())
}

#[test]
fn receipt_keys_reject_non_canonical_ordering_mutation() -> Result<(), crate::session::StepFatal> {
    let keys = [
        receipt_key(ReceiptSource::Auction(0), 0)?,
        receipt_key(ReceiptSource::SealedIntent(0), 0)?,
    ];

    assert!(validate_receipt_keys(&keys).is_err());
    Ok(())
}

#[test]
fn receipt_keys_accept_complete_explicit_canonical_source_order(
) -> Result<(), crate::session::StepFatal> {
    let first = envelope();
    let second = envelope_with_order(2);
    let keys = [
        receipt_key(ReceiptSource::P0Expiry(0), 0)?,
        receipt_key(ReceiptSource::P0Expiry(1), 0)?,
        receipt_key(ReceiptSource::SealedIntent(0), 0)?,
        receipt_key(ReceiptSource::SealedIntent(1), 0)?,
        receipt_key(ReceiptSource::Auction(0), 0)?,
        receipt_key(ReceiptSource::Auction(0), 1)?,
        receipt_key_with_envelope(ReceiptSource::Auction(0), second, 0)?,
        receipt_key(ReceiptSource::DayEnd(0), 0)?,
    ];

    assert_eq!(keys[0].cmp(&keys[1]), std::cmp::Ordering::Less);
    assert_eq!(keys[1].cmp(&keys[2]), std::cmp::Ordering::Less);
    assert_eq!(keys[2].cmp(&keys[3]), std::cmp::Ordering::Less);
    assert_eq!(keys[3].cmp(&keys[4]), std::cmp::Ordering::Less);
    assert_eq!(keys[4].cmp(&keys[5]), std::cmp::Ordering::Less);
    assert_eq!(keys[5].cmp(&keys[6]), std::cmp::Ordering::Less);
    assert_eq!(keys[6].cmp(&keys[7]), std::cmp::Ordering::Less);
    assert_eq!(first.order, OrderId(1));
    assert!(validate_receipt_keys(&keys).is_ok());
    Ok(())
}

#[test]
fn receipt_keys_allow_source_ordinal_restart_for_a_new_source(
) -> Result<(), crate::session::StepFatal> {
    let keys = [
        receipt_key(ReceiptSource::SealedIntent(0), 0)?,
        receipt_key(ReceiptSource::Auction(0), 0)?,
        receipt_key(ReceiptSource::DayEnd(0), 0)?,
    ];

    assert!(validate_receipt_keys(&keys).is_ok());
    Ok(())
}

#[test]
fn receipt_keys_allow_each_envelope_to_restart_within_one_source(
) -> Result<(), crate::session::StepFatal> {
    let keys = [
        receipt_key_with_envelope(ReceiptSource::SealedIntent(0), envelope(), 0)?,
        receipt_key_with_envelope(ReceiptSource::SealedIntent(0), envelope_with_order(2), 0)?,
    ];

    assert!(validate_receipt_keys(&keys).is_ok());
    Ok(())
}

#[test]
fn receipt_key_constructor_rejects_invalid_source_journal_pairing() {
    let result = ReceiptLocalKey::new(
        JournalRank::PreSeal,
        ReceiptSource::Auction(0),
        ReceiptTransition {
            envelope: envelope(),
            ordinal: 0,
        },
    );

    assert!(result.is_err());
}

#[test]
fn indexed_receipt_keys_reject_duplicate_envelope_index_pair(
) -> Result<(), crate::session::StepFatal> {
    let local_key = receipt_key(ReceiptSource::SealedIntent(0), 0)?;
    let keys = [
        indexed_key(ReceiptIndex(7), local_key.clone()),
        indexed_key(
            ReceiptIndex(7),
            receipt_key(ReceiptSource::SealedIntent(0), 1)?,
        ),
    ];

    assert!(validate_indexed_receipt_keys(&keys).is_err());
    Ok(())
}

#[test]
fn indexed_receipt_keys_reject_envelope_identity_mutation() -> Result<(), crate::session::StepFatal>
{
    let mut key = indexed_key(
        ReceiptIndex(7),
        receipt_key(ReceiptSource::SealedIntent(0), 0)?,
    );
    key.envelope.order = OrderId(2);

    assert!(validate_indexed_receipt_keys(&[key]).is_err());
    Ok(())
}

#[test]
fn indexed_receipt_keys_reject_non_contiguous_indices() -> Result<(), crate::session::StepFatal> {
    let keys = [
        indexed_key(
            ReceiptIndex(7),
            receipt_key(ReceiptSource::SealedIntent(0), 0)?,
        ),
        indexed_key(ReceiptIndex(9), receipt_key(ReceiptSource::Auction(0), 0)?),
    ];

    assert!(validate_indexed_receipt_keys(&keys).is_err());
    Ok(())
}

fn receipt_key(
    source: ReceiptSource,
    ordinal: u64,
) -> Result<ReceiptLocalKey, crate::session::StepFatal> {
    receipt_key_with_envelope(source, envelope(), ordinal)
}

fn receipt_key_with_envelope(
    source: ReceiptSource,
    envelope: EnvelopeKey,
    ordinal: u64,
) -> Result<ReceiptLocalKey, crate::session::StepFatal> {
    ReceiptLocalKey::new(
        source.journal(),
        source,
        ReceiptTransition { envelope, ordinal },
    )
}

fn indexed_key(index: ReceiptIndex, local_key: ReceiptLocalKey) -> IndexedReceiptKey {
    IndexedReceiptKey {
        index,
        envelope: envelope(),
        local_key,
    }
}

fn envelope() -> EnvelopeKey {
    envelope_with_order(1)
}

fn envelope_with_order(order: u64) -> EnvelopeKey {
    EnvelopeKey {
        account: AccountId(1),
        stock: StockCode("600001".to_owned()),
        order: OrderId(order),
        side: Side::Buy,
    }
}
