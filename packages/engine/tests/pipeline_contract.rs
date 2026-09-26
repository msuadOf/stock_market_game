use engine::session::pipeline::*;
use engine::{AccountId, OrderId, Side, StockCode};

#[test]
fn step_phases_have_fixed_complete_rank_mapping() {
    let observed: Vec<_> = TickPhase::ALL
        .into_iter()
        .map(|phase| (phase.rank(), phase.name()))
        .collect();
    assert_eq!(
        observed,
        vec![
            (0, "expiry_shadow"),
            (1, "seal_allocation_snapshot"),
            (2, "decision_shadow"),
            (3, "account_validation"),
            (4, "stock_processing"),
            (5, "receipt_aggregation"),
            (6, "settlement_shadow"),
            (7, "derivation_audit"),
            (8, "pre_commit_validation"),
            (9, "commit_tick")
        ]
    );
}

#[test]
fn event_source_numeric_contract_and_entity_order_are_explicit() {
    assert_eq!(
        [
            EventSourceIndex::Sealed,
            EventSourceIndex::P0,
            EventSourceIndex::PriceTick,
            EventSourceIndex::DayEnd,
            EventSourceIndex::Session
        ]
        .map(EventSourceIndex::rank),
        [0, 1, 2, 3, 4]
    );
    let expected = vec![
        EntityTag::Stock(StockCode("000001".into())),
        EntityTag::Stock(StockCode("600001".into())),
        EntityTag::Account(AccountId(1)),
        EntityTag::Account(AccountId(2)),
        EntityTag::Session,
    ];
    for shift in 0..expected.len() {
        let mut permutation = expected.clone();
        permutation.rotate_left(shift);
        permutation.sort();
        assert_eq!(permutation, expected);
    }
}

#[test]
fn receipt_payload_precedes_envelope_and_envelope_precedes_ordinal() {
    let envelope = EnvelopeKey {
        account: AccountId(1),
        stock: StockCode("600001".into()),
        order: OrderId(1),
        side: Side::Buy,
    };
    let mut later_envelope = envelope.clone();
    later_envelope.order = OrderId(2);
    let inputs = [
        (0, envelope.clone(), 0),
        (0, envelope.clone(), 1),
        (0, later_envelope, 0),
        (1, envelope, 0),
    ];
    let expected: Vec<_> = inputs
        .into_iter()
        .map(|(payload, envelope, ordinal)| {
            ReceiptLocalKey::new(
                JournalRank::SealedBatch,
                ReceiptSource::SealedIntent(payload),
                ReceiptTransition { envelope, ordinal },
            )
            .unwrap()
        })
        .collect();
    for shift in 0..expected.len() {
        let mut observed = expected.clone();
        observed.rotate_left(shift);
        observed.sort();
        assert_eq!(observed, expected);
    }
}

#[test]
fn receipt_order_is_journal_source_payload_envelope_then_transition() {
    let envelope = EnvelopeKey {
        account: AccountId(1),
        stock: StockCode("600001".into()),
        order: OrderId(1),
        side: Side::Buy,
    };
    let sources = [
        ReceiptSource::P0Expiry(9),
        ReceiptSource::SealedIntent(0),
        ReceiptSource::SealedIntent(1),
        ReceiptSource::Auction(0),
        ReceiptSource::DayEnd(0),
    ];
    let expected: Vec<_> = sources
        .into_iter()
        .map(|source| {
            ReceiptLocalKey::new(
                source.journal(),
                source,
                ReceiptTransition {
                    envelope: envelope.clone(),
                    ordinal: 0,
                },
            )
            .unwrap()
        })
        .collect();
    let mut observed = expected.clone();
    observed.reverse();
    observed.sort();
    assert_eq!(observed, expected);
    assert_eq!(JournalRank::PreSeal.rank(), 0);
    assert_eq!(JournalRank::SealedBatch.rank(), 1);
    assert_eq!(sources.map(|source| source.rank()), [0, 0, 0, 1, 2]);
    assert!(matches!(
        ReceiptLocalKey::new(
            JournalRank::SealedBatch,
            ReceiptSource::P0Expiry(0),
            ReceiptTransition {
                envelope,
                ordinal: 0
            }
        ),
        Err(engine::session::StepFatal::InvariantViolation { .. })
    ));
    assert!(validate_receipt_keys(&[expected[0].clone(), expected[0].clone()]).is_err());
}
