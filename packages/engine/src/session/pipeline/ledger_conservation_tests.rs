use super::ledger_tests::*;
use super::{Envelope, EnvelopeKey, EnvelopeLedger, ReceiptDelta, ResVec};
use crate::Money;

#[test]
fn real_parallel_ledger_reports_early_total_overflow_before_later_row_failure() {
    for shares in [false, true] {
        let envelopes = (0..512).map(|index| {
            let amount = if index == 0 { i64::MAX } else { 1 };
            Envelope::tick_start_existing(
                EnvelopeKey {
                    account: crate::AccountId(index),
                    stock: crate::StockCode("600000".to_owned()),
                    order: crate::OrderId(index),
                    side: if shares {
                        crate::Side::Sell
                    } else {
                        crate::Side::Buy
                    },
                },
                if shares {
                    Money::ZERO
                } else {
                    Money::from_cents(amount)
                },
                if shares {
                    if index == 0 {
                        u32::MAX
                    } else {
                        1
                    }
                } else {
                    0
                },
                audit(),
            )
        });
        let mut ledger = EnvelopeLedger::new(7, envelopes).unwrap();
        let later = ledger.iter().nth(500).unwrap().0.clone();
        ledger.conservation.remove(&later);
        let before = ledger.clone();
        for threads in [1, 4] {
            let error = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .unwrap()
                .install(|| ledger.validate_complete_evidence())
                .unwrap_err();
            assert!(
                matches!(error, super::StepFatal::InvariantViolation { description, location }
                if location == "pipeline::ResVec"
                    && if shares { description == "shares add" } else { description.contains("overflow in add") })
            );
            assert_eq!(ledger, before);
        }
    }
}
use std::collections::BTreeSet;
use std::sync::Mutex;

#[test]
fn conservation_aggregate_sums_the_same_cash_and_share_equations_per_key() {
    let buy = key();
    let sell = second_key();
    let mut ledger = EnvelopeLedger::new(
        7,
        [
            Envelope::tick_start_existing(
                buy.clone(),
                Money::from_cents(100),
                0,
                audit_with_remaining(100),
            ),
            Envelope::created_at_validation(
                sell.clone(),
                Money::ZERO,
                100,
                audit_with_remaining(100),
            ),
        ],
    )
    .unwrap();
    let mut receipts = [
        sealed_receipt(
            buy,
            ReceiptDelta::sealed(
                ResVec::new(Money::from_cents(90), 0),
                ResVec::new(Money::from_cents(10), 0),
                ResVec::ZERO,
            ),
            0,
        ),
        sealed_receipt(
            sell,
            ReceiptDelta::sealed(ResVec::new(Money::ZERO, 100), ResVec::ZERO, ResVec::ZERO),
            0,
        ),
    ];
    receipts[0].value_after = Money::from_cents(90);
    ledger.apply(&mut receipts).unwrap();
    ledger.validate_conservation().unwrap();
}
#[test]
fn conservation_aggregate_rejects_cross_key_resource_leaks_without_mutation() {
    let first = Envelope::tick_start_existing(key(), Money::from_cents(100), 0, audit());
    let second = Envelope::tick_start_existing(second_key(), Money::ZERO, 100, audit());
    let mut ledger = EnvelopeLedger::new(7, [first, second]).unwrap();
    let before = ledger.clone();
    let mut receipts = [
        sealed_receipt(
            key(),
            ReceiptDelta::sealed(
                ResVec::new(Money::from_cents(100), 0),
                ResVec::ZERO,
                ResVec::ZERO,
            ),
            0,
        ),
        sealed_receipt(
            second_key(),
            ReceiptDelta::sealed(ResVec::new(Money::ZERO, 99), ResVec::ZERO, ResVec::ZERO),
            1,
        ),
    ];
    assert!(ledger.apply(&mut receipts).is_err());
    assert_eq!(ledger, before);
}
#[test]
fn conservation_apply_rechecks_the_aggregate_before_installing_receipts() {
    let mut ledger = created_ledger();
    let before = ledger.clone();
    let mut receipts = [sealed_receipt(
        key(),
        ReceiptDelta::sealed(
            ResVec::new(Money::from_cents(99), 0),
            ResVec::ZERO,
            ResVec::ZERO,
        ),
        0,
    )];
    assert!(ledger.apply(&mut receipts).is_err());
    assert_eq!(ledger, before);
}

#[test]
fn conservation_rows_execute_on_multiple_rayon_workers() {
    let envelopes = (0..4096).map(|index| {
        Envelope::tick_start_existing(
            EnvelopeKey {
                account: crate::AccountId(index),
                stock: crate::StockCode(format!("{:06}", index % 1_000_000)),
                order: crate::OrderId(index),
                side: crate::Side::Buy,
            },
            Money::from_cents(1),
            0,
            audit(),
        )
    });
    let ledger = EnvelopeLedger::new(7, envelopes).unwrap();
    let workers = Mutex::new(BTreeSet::new());
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .unwrap();

    pool.install(|| {
        super::ledger_conservation::validate_with_observer(&ledger, || {
            workers
                .lock()
                .unwrap()
                .insert(rayon::current_thread_index().unwrap());
        })
    })
    .unwrap();

    assert!(workers.into_inner().unwrap().len() > 1);
}

#[test]
fn conservation_row_chunks_keep_source_error_order_and_skip_small_task_parallelism() {
    let rows: Vec<_> = (0..4096).collect();
    let errors = super::ledger_conservation::map_rows(&rows, |row| {
        if *row == 127 || *row == 2048 {
            Err(super::ledger_validation::invariant(&format!("row {row}")))
        } else {
            Ok(())
        }
    });
    let first_error = errors.into_iter().find_map(Result::err).unwrap();
    assert!(matches!(
        first_error,
        super::StepFatal::InvariantViolation { description, .. } if description == "row 127"
    ));

    let small_rows: Vec<_> = (0..511).collect();
    let workers = Mutex::new(BTreeSet::new());
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .unwrap();
    pool.install(|| {
        let rows = super::ledger_conservation::map_rows(&small_rows, |_| {
            workers
                .lock()
                .unwrap()
                .insert(rayon::current_thread_index().unwrap());
            Ok(())
        });
        rows.into_iter().collect::<Result<Vec<_>, _>>()
    })
    .unwrap();
    assert_eq!(workers.into_inner().unwrap().len(), 1);
}
