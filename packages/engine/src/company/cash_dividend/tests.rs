use super::*;
use crate::company::share_registry::{
    AcquisitionSource, HolderId, RegistrationSnapshot, ShareHolding, ShareLot, ShareRegistry,
    ShareRestriction,
};
use crate::company::CompanyId;
use crate::{
    account::StockCode,
    calendar::{CalendarExchange, CivilDate, TradingCalendar},
    orderbook::AccountId,
};

fn day(value: &str) -> CivilDate {
    CivilDate::from_iso(value).unwrap()
}

fn calendar() -> TradingCalendar {
    TradingCalendar::current_default_calendar().unwrap()
}

fn lot(id: &str, qty: u64) -> ShareLot {
    ShareLot {
        id: id.into(),
        qty,
        acquired_on: day("2030-01-01"),
        source: AcquisitionSource::InitialAllocation {
            evidence: id.into(),
        },
        restriction: ShareRestriction::Unrestricted,
    }
}

fn snapshot() -> RegistrationSnapshot {
    snapshot_for("plan-1")
}

fn snapshot_for(plan_id: &str) -> RegistrationSnapshot {
    let mut registry = ShareRegistry::new(
        StockCode("600001".into()),
        CompanyId("issuer-A".into()),
        100,
        day("2030-01-07"),
        vec![
            ShareHolding {
                holder: HolderId::Account(AccountId(1)),
                lots: vec![lot("account", 30)],
            },
            ShareHolding {
                holder: HolderId::External("outside-owner".into()),
                lots: vec![lot("external", 60)],
            },
            ShareHolding {
                holder: HolderId::IssuerTreasury,
                lots: vec![lot("treasury", 10)],
            },
        ],
    )
    .unwrap();
    registry
        .register(plan_id.into(), day("2030-01-07"))
        .unwrap()
        .clone()
}

fn plan() -> CashDividendPlan {
    CashDividendPlan::new(
        "plan-1".into(),
        CompanyId("issuer-A".into()),
        StockCode("600001".into()),
        CalendarExchange::Sse,
        day("2030-01-03"),
        day("2030-01-04"),
        day("2030-01-07"),
        day("2030-01-08"),
        day("2030-01-10"),
        Money::from_cents(2),
        Money::from_cents(180),
        &calendar(),
    )
    .unwrap()
}

fn ready_book() -> CashDividendBook {
    let mut book = CashDividendBook::new(plan()).unwrap();
    book.announce(day("2030-01-04")).unwrap();
    book.register(snapshot(), &calendar()).unwrap();
    book
}

#[test]
fn registration_freezes_gross_entitlement_and_excludes_issuer_treasury() {
    let book = ready_book();
    let entitlements = book.entitlements().unwrap();
    assert_eq!(entitlements.len(), 2);
    assert_eq!(entitlements[0].gross, Money::from_cents(60));
    assert_eq!(
        entitlements[1].holder,
        HolderId::External("outside-owner".into())
    );
    assert_eq!(entitlements[1].gross, Money::from_cents(120));
    assert_eq!(book.total_gross().unwrap(), Money::from_cents(180));
}

#[test]
fn all_treasury_registration_is_rejected_without_creating_a_zero_payable_plan() {
    let mut registry = ShareRegistry::new(
        StockCode("600001".into()),
        CompanyId("issuer-A".into()),
        100,
        day("2030-01-07"),
        vec![ShareHolding {
            holder: HolderId::IssuerTreasury,
            lots: vec![lot("treasury-only", 100)],
        }],
    )
    .unwrap();
    let snapshot = registry
        .register("plan-1".into(), day("2030-01-07"))
        .unwrap()
        .clone();
    let mut book = CashDividendBook::new(plan()).unwrap();
    book.announce(day("2030-01-04")).unwrap();
    let before = book.clone();
    assert!(matches!(
        book.register(snapshot, &calendar()),
        Err(CashDividendError::NoEligibleHolders)
    ));
    assert_eq!(book, before);
}

#[test]
fn date_and_distributable_validation_is_explicit() {
    let mut wrong_dates = plan();
    wrong_dates.payable_on = day("2030-01-05");
    assert!(matches!(
        wrong_dates.validate(),
        Err(CashDividendError::InvalidPlan { .. })
    ));
    wrong_dates = plan();
    wrong_dates.ex_dividend_on = day("2030-01-09");
    assert!(matches!(
        wrong_dates.validate_calendar(&calendar()),
        Err(CashDividendError::InvalidExDividendDate { .. })
    ));
    let mut beyond_legal_deadline = plan();
    beyond_legal_deadline.payable_on = day("2030-07-04");
    assert!(matches!(
        beyond_legal_deadline.validate(),
        Err(CashDividendError::InvalidPlan { .. })
    ));
    let mut at_six_month_deadline = plan();
    at_six_month_deadline.payable_on = day("2030-07-03");
    assert!(at_six_month_deadline.validate().is_ok());

    let insufficient = CashDividendPlan::new(
        "plan-low".into(),
        CompanyId("issuer-A".into()),
        StockCode("600001".into()),
        CalendarExchange::Sse,
        day("2030-01-03"),
        day("2030-01-04"),
        day("2030-01-07"),
        day("2030-01-08"),
        day("2030-01-10"),
        Money::from_cents(3),
        Money::from_cents(269),
        &calendar(),
    )
    .unwrap();
    let mut book = CashDividendBook::new(insufficient).unwrap();
    book.announce(day("2030-01-04")).unwrap();
    let before = book.clone();
    assert!(matches!(
        book.register(snapshot_for("plan-low"), &calendar()),
        Err(CashDividendError::ExceedsDistributableAmount { .. })
    ));
    assert_eq!(book, before);
}

#[test]
fn failed_payment_requires_a_concrete_reason_and_is_atomic() {
    let mut book = ready_book();
    book.mark_payable(day("2030-01-10")).unwrap();
    let before = book.clone();
    assert!(matches!(
        book.settle(
            "blank-failure".into(),
            day("2030-01-10"),
            vec![
                HolderPaymentOutcome::Paid {
                    holder: HolderId::Account(AccountId(1)),
                    amount: Money::from_cents(60),
                },
                HolderPaymentOutcome::Failed {
                    holder: HolderId::External("outside-owner".into()),
                    reason: "  ".into(),
                },
            ],
        ),
        Err(CashDividendError::MissingFailureReason { .. })
    ));
    assert_eq!(book, before);
}

#[test]
fn actual_payment_after_statutory_deadline_is_preserved_as_noncompliant_fact() {
    let mut book = ready_book();
    book.mark_payable(day("2030-01-10")).unwrap();
    let receipt = book
        .settle(
            "late-pay".into(),
            day("2030-07-04"),
            vec![
                HolderPaymentOutcome::Paid {
                    holder: HolderId::Account(AccountId(1)),
                    amount: Money::from_cents(60),
                },
                HolderPaymentOutcome::Paid {
                    holder: HolderId::External("outside-owner".into()),
                    amount: Money::from_cents(120),
                },
            ],
        )
        .unwrap();
    assert!(!receipt.within_six_month_deadline());
    assert_eq!(book.unresolved_failures(), &[]);
    let restored: CashDividendBook =
        serde_json::from_value(serde_json::to_value(&book).unwrap()).unwrap();
    assert_eq!(restored, book);
}

#[test]
fn consecutive_failed_payment_retries_remain_restorable() {
    let mut book = ready_book();
    book.mark_payable(day("2030-01-10")).unwrap();
    let paid_account = HolderPaymentOutcome::Paid {
        holder: HolderId::Account(AccountId(1)),
        amount: Money::from_cents(60),
    };
    let failed_external = HolderPaymentOutcome::Failed {
        holder: HolderId::External("outside-owner".into()),
        reason: "external endpoint unavailable".into(),
    };
    book.settle(
        "pay-1".into(),
        day("2030-01-10"),
        vec![paid_account.clone(), failed_external.clone()],
    )
    .unwrap();
    book.settle("pay-2".into(), day("2030-01-11"), vec![failed_external])
        .unwrap();
    assert!(book.validate().is_ok());
    let restored: CashDividendBook =
        serde_json::from_value(serde_json::to_value(&book).unwrap()).unwrap();
    assert_eq!(restored, book);
}

#[test]
fn payment_facts_cannot_be_recorded_out_of_date_order() {
    let mut book = ready_book();
    book.mark_payable(day("2030-01-10")).unwrap();
    book.settle(
        "first".into(),
        day("2030-01-11"),
        vec![
            HolderPaymentOutcome::Paid {
                holder: HolderId::Account(AccountId(1)),
                amount: Money::from_cents(60),
            },
            HolderPaymentOutcome::Failed {
                holder: HolderId::External("outside-owner".into()),
                reason: "external endpoint unavailable".into(),
            },
        ],
    )
    .unwrap();
    let before = book.clone();
    assert!(matches!(
        book.settle(
            "earlier".into(),
            day("2030-01-10"),
            vec![HolderPaymentOutcome::Failed {
                holder: HolderId::External("outside-owner".into()),
                reason: "external endpoint unavailable".into(),
            }],
        ),
        Err(CashDividendError::PaymentDateRegression { .. })
    ));
    assert_eq!(book, before);
}

#[test]
fn stage_calls_are_idempotent_and_conflicting_replay_is_rejected() {
    let mut book = CashDividendBook::new(plan()).unwrap();
    book.announce(day("2030-01-04")).unwrap();
    book.announce(day("2030-01-04")).unwrap();
    assert!(book.announce(day("2030-01-03")).is_err());
    let snap = snapshot();
    book.register(snap.clone(), &calendar()).unwrap();
    book.register(snap, &calendar()).unwrap();
    let before = book.clone();
    let mut changed_registry = ShareRegistry::new(
        StockCode("600001".into()),
        CompanyId("issuer-A".into()),
        100,
        day("2030-01-07"),
        vec![
            ShareHolding {
                holder: HolderId::Account(AccountId(1)),
                lots: vec![lot("account", 31)],
            },
            ShareHolding {
                holder: HolderId::External("outside-owner".into()),
                lots: vec![lot("external", 59)],
            },
            ShareHolding {
                holder: HolderId::IssuerTreasury,
                lots: vec![lot("treasury", 10)],
            },
        ],
    )
    .unwrap();
    let conflicting = changed_registry
        .register("plan-1".into(), day("2030-01-07"))
        .unwrap()
        .clone();
    assert!(matches!(
        book.register(conflicting, &calendar()),
        Err(CashDividendError::SnapshotMismatch { .. })
    ));
    assert_eq!(book, before);
}

#[test]
fn completed_stage_replays_are_idempotent_without_rewinding_state() {
    let mut book = ready_book();
    book.mark_payable(day("2030-01-10")).unwrap();
    book.settle(
        "pay-1".into(),
        day("2030-01-10"),
        vec![
            HolderPaymentOutcome::Paid {
                holder: HolderId::Account(AccountId(1)),
                amount: Money::from_cents(60),
            },
            HolderPaymentOutcome::Paid {
                holder: HolderId::External("outside-owner".into()),
                amount: Money::from_cents(120),
            },
        ],
    )
    .unwrap();
    let before_replay = book.clone();
    book.announce(day("2030-01-04")).unwrap();
    book.mark_payable(day("2030-01-10")).unwrap();
    assert_eq!(book, before_replay);
}

#[test]
fn external_payment_failure_is_visible_and_successful_holders_are_not_repaid() {
    let mut book = ready_book();
    book.mark_payable(day("2030-01-10")).unwrap();
    book.mark_payable(day("2030-01-10")).unwrap();
    let batch = vec![
        HolderPaymentOutcome::Paid {
            holder: HolderId::Account(AccountId(1)),
            amount: Money::from_cents(60),
        },
        HolderPaymentOutcome::Failed {
            holder: HolderId::External("outside-owner".into()),
            reason: "external endpoint unavailable".into(),
        },
    ];
    let receipt = book
        .settle("pay-1".into(), day("2030-01-10"), batch.clone())
        .unwrap();
    assert_eq!(
        receipt.failed_holders(),
        &[HolderId::External("outside-owner".into())]
    );
    assert!(matches!(book.status(), CashDividendStatus::PartiallyPaid));
    let paid = book
        .settle(
            "pay-2".into(),
            day("2030-01-11"),
            vec![HolderPaymentOutcome::Paid {
                holder: HolderId::External("outside-owner".into()),
                amount: Money::from_cents(120),
            }],
        )
        .unwrap();
    assert!(paid.failed_holders().is_empty());
    assert!(matches!(book.status(), CashDividendStatus::Paid));
    let done = book.clone();
    assert!(matches!(
        book.settle("pay-3".into(), day("2030-01-12"), vec![]),
        Err(CashDividendError::AlreadyPaid)
    ));
    assert_eq!(book, done);
}

#[test]
fn invalid_payment_batch_is_atomic_and_requires_every_external_result() {
    let mut book = ready_book();
    book.mark_payable(day("2030-01-10")).unwrap();
    book.mark_payable(day("2030-01-10")).unwrap();
    let before = book.clone();
    assert!(matches!(
        book.settle(
            "incomplete".into(),
            day("2030-01-10"),
            vec![HolderPaymentOutcome::Paid {
                holder: HolderId::Account(AccountId(1)),
                amount: Money::from_cents(60)
            },]
        ),
        Err(CashDividendError::IncompletePaymentResults { .. })
    ));
    assert_eq!(book, before);
    assert!(matches!(
        book.settle(
            "wrong".into(),
            day("2030-01-10"),
            vec![
                HolderPaymentOutcome::Paid {
                    holder: HolderId::Account(AccountId(1)),
                    amount: Money::from_cents(59)
                },
                HolderPaymentOutcome::Paid {
                    holder: HolderId::External("outside-owner".into()),
                    amount: Money::from_cents(120)
                },
            ]
        ),
        Err(CashDividendError::PaymentAmountMismatch { .. })
    ));
    assert_eq!(book, before);
}

#[test]
fn persisted_payment_facts_round_trip_and_corruption_is_rejected() {
    let mut book = ready_book();
    book.mark_payable(day("2030-01-10")).unwrap();
    book.settle(
        "pay-1".into(),
        day("2030-01-10"),
        vec![
            HolderPaymentOutcome::Paid {
                holder: HolderId::Account(AccountId(1)),
                amount: Money::from_cents(60),
            },
            HolderPaymentOutcome::Failed {
                holder: HolderId::External("outside-owner".into()),
                reason: "external endpoint unavailable".into(),
            },
        ],
    )
    .unwrap();
    let value = serde_json::to_value(&book).unwrap();
    let restored: CashDividendBook = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(restored, book);

    let mut missing_nullable_registration = value.clone();
    missing_nullable_registration["registration"] = serde_json::Value::Null;
    missing_nullable_registration
        .as_object_mut()
        .unwrap()
        .remove("registration");
    assert!(serde_json::from_value::<CashDividendBook>(missing_nullable_registration).is_err());

    let mut corrupt_receipt = value;
    corrupt_receipt["payments"][0]["outcomes"][0]["Paid"]["amount"] =
        serde_json::to_value(Money::from_cents(59)).unwrap();
    assert!(serde_json::from_value::<CashDividendBook>(corrupt_receipt).is_err());
}

#[test]
fn restore_requires_caller_calendar_to_validate_ex_dividend_sequence() {
    let book = ready_book();
    let mut value = serde_json::to_value(&book).unwrap();
    value["plan"]["ex_dividend_on"] = serde_json::json!("2030-01-09");
    let restored: CashDividendBook = serde_json::from_value(value).unwrap();
    assert!(restored.validate().is_ok());
    assert!(matches!(
        restored.validate_with_calendar(&calendar()),
        Err(CashDividendError::InvalidExDividendDate { .. })
    ));
}

#[test]
fn payment_retry_identity_uses_canonical_holder_order() {
    let mut book = ready_book();
    book.mark_payable(day("2030-01-10")).unwrap();
    let outcomes = vec![
        HolderPaymentOutcome::Failed {
            holder: HolderId::External("outside-owner".into()),
            reason: "external endpoint unavailable".into(),
        },
        HolderPaymentOutcome::Paid {
            holder: HolderId::Account(AccountId(1)),
            amount: Money::from_cents(60),
        },
    ];
    let first = book
        .settle("pay-1".into(), day("2030-01-10"), outcomes.clone())
        .unwrap();
    let replay = book
        .settle(
            "pay-1".into(),
            day("2030-01-10"),
            outcomes.into_iter().rev().collect(),
        )
        .unwrap();
    assert_eq!(first, replay);
}
