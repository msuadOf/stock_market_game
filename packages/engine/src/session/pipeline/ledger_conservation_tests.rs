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
                    stock: crate::StockId(0),
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
            let error = crate::compute::ComputeContext::from_workers(threads)
                .install(|| ledger.validate_complete_evidence())
                .unwrap_err();
            assert!(
                matches!(error, super::StepFatal::InvariantViolation { description, location }
                if location == "pipeline::ResVec"
                    && if shares { description == "shares add" } else { description.contains("overflow in CashAdd") })
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
fn dense_conservation_without_explicit_context_stays_on_the_caller_thread() {
    // ≥512 行 + 未显式传 compute：row 守恒校验必须全部在调用线程串行执行。
    // 串行脊（如 ExpiryShadow 的 apply/remove_terminal 守恒校验）在非 worker
    // 线程触发时，不得隐式借用 caller 域的 Rayon registry——那会惰性初始化
    // 全局池，正是线程数 132→260 的元凶。这里直接驱动 apply/remove_terminal
    // 所触发的同一守恒校验入口（validate_with_observer 的 dense 分支）。
    let envelopes = (0..4096).map(|index| {
        Envelope::tick_start_existing(
            EnvelopeKey {
                account: crate::AccountId(index),
                stock: crate::StockId((index % 1_000_000) as u32),
                order: crate::OrderId(index),
                side: crate::Side::Buy,
            },
            Money::from_cents(1),
            0,
            audit(),
        )
    });
    let ledger = EnvelopeLedger::new(7, envelopes).unwrap();
    let caller = std::thread::current().id();
    let observed_rows = std::sync::atomic::AtomicUsize::new(0);
    super::ledger_conservation::validate_with_observer(&ledger, || {
        assert_eq!(
            std::thread::current().id(),
            caller,
            "None 域的 row 守恒校验逃逸到非调用线程"
        );
        // rayon 退役（批 7）：串行垫片无池线程概念，逃逸检测由线程 id 兜底。
        assert!(
            std::thread::current().id() == caller,
            "None 域的 row 守恒校验逃逸到非调用线程"
        );
        observed_rows.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    })
    .unwrap();
    // 分支行数计数：dense 分支必须逐行校验恰一次。
    assert_eq!(
        observed_rows.into_inner(),
        4096,
        "dense 分支必须逐行校验恰一次"
    );
}

#[test]
fn conservation_row_chunks_keep_source_error_order_and_skip_small_task_parallelism() {
    let rows: Vec<_> = (0..4096).collect();
    let errors = super::ledger_conservation::map_rows(&rows, None, |row| {
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
    let pool = crate::compute::ComputeContext::from_workers(4);
    pool.install(|| {
        let rows = super::ledger_conservation::map_rows(&small_rows, None, |_| {
            workers
                .lock()
                .unwrap()
                .insert(0usize);
            Ok(())
        });
        rows.into_iter().collect::<Result<Vec<_>, _>>()
    })
    .unwrap();
    assert_eq!(workers.into_inner().unwrap().len(), 1);
}
