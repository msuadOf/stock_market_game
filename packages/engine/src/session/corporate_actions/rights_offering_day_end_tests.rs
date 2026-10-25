//! 少量账户覆盖配股缴款、关窗、退款与发行分腿，不启动市场线程。
use super::*;
use crate::company::rights_offering::{RightsOfferingStatus, RightsSubscriptionStrategy};
use crate::company::share_registry::{
    AcquisitionSource, HolderId, ShareHolding, ShareLot, ShareRestriction,
};

fn date(value: &str) -> CivilDate {
    CivilDate::from_iso(value).unwrap()
}
fn calendar() -> TradingCalendar {
    TradingCalendar::current_default_calendar().unwrap()
}
fn plan() -> RightsOfferingEventPlan {
    RightsOfferingEventPlan {
        event_id: "rights-r3b".into(),
        approval_reference: "approved".into(),
        issuer: CompanyId(0),
        stock: StockCode("000001".into()),
        exchange: crate::calendar::CalendarExchange::Szse,
        approved_on: date("2030-06-01"),
        announced_on: date("2030-06-02"),
        registered_on: date("2030-06-03"),
        payment_start_on: date("2030-06-04"),
        payment_deadline_on: date("2030-06-11"),
        ex_rights_on: date("2030-06-12"),
        settlement_on: date("2030-06-13"),
        listing_on: date("2030-06-13"),
        price_per_share: Money::from_cents(800),
        mode: RightsOfferingMode::RightsToAllShareholders {
            shares_per_existing_share_micros: 500_000,
        },
        npc_subscription_strategy: RightsSubscriptionStrategy::FullByDefault,
    }
}
fn registry(on: CivilDate) -> ShareRegistry {
    ShareRegistry::new(
        plan().stock,
        CompanyId(0),
        20,
        on,
        vec![ShareHolding {
            holder: HolderId::Account(AccountId(7)),
            lots: vec![ShareLot {
                id: "initial".into(),
                qty: 20,
                acquired_on: date("2020-01-02"),
                source: AcquisitionSource::InitialAllocation {
                    evidence: "opening".into(),
                },
                restriction: ShareRestriction::Unrestricted,
            }],
        }],
    )
    .unwrap()
}
fn actions() -> SessionCorporateActions {
    let mut registry = registry(plan().registered_on);
    let snapshot = registry
        .register(plan().event_id, plan().registered_on)
        .unwrap()
        .clone();
    let mut book = RightsOfferingBook::new(plan()).unwrap();
    book.announce(plan().announced_on).unwrap();
    book.entitle(snapshot, &calendar()).unwrap();
    SessionCorporateActions {
        registries: vec![registry],
        rights_offerings: vec![book],
        ..Default::default()
    }
}
fn resource(cents: i64, kind: AccountKind) -> BTreeMap<AccountId, RightsFinalResources> {
    BTreeMap::from([(
        AccountId(7),
        RightsFinalResources {
            available_cash: Money::from_cents(cents),
            kind,
        },
    )])
}
fn queued(on: CivilDate, requested_shares: u64) -> QueuedRightsSubscription {
    QueuedRightsSubscription {
        event_id: plan().event_id,
        account: AccountId(7),
        submitted_on: on,
        requested_shares,
    }
}
fn install(actions: &mut SessionCorporateActions, result: RightsOfferingDayEndCandidate) {
    actions.registries = result.registries;
    actions.dividend_tax_books = result.dividend_tax_books;
    actions.rights_offerings = result.rights_offerings;
    actions.rights_subscription_queue = result.rights_subscription_queue;
    actions.rejected_rights_subscriptions = result.rejected_rights_subscriptions;
}
#[test]
fn npc_uses_final_available_cash_and_records_partial_waiver_without_mutating_input() {
    let actions = actions();
    let before = actions.clone();
    let resources = resource(2500, AccountKind::Retail);
    let candidate = rights_offering_day_end_candidate(
        &actions,
        plan().payment_start_on,
        &calendar(),
        &resources,
        &BTreeSet::new(),
    )
    .unwrap();
    assert_eq!(actions, before);
    assert_eq!(
        resources[&AccountId(7)].available_cash,
        Money::from_cents(2500)
    );
    assert_eq!(candidate.results.len(), 1);
    let result = &candidate.results[0];
    assert_eq!(
        (
            result.requested_shares,
            result.paid_shares,
            result.waived_shares
        ),
        (10, 3, 7)
    );
    assert_eq!(result.paid_amount, Money::from_cents(2400));
    assert_eq!(
        result.disposition,
        RightsAccountDisposition::PreparedSubscription
    );
    assert_eq!(
        candidate.intents,
        vec![RightsOfferingIntent::PaymentDebit {
            request: result.request.clone(),
            amount: Money::from_cents(2400)
        }]
    );
}
#[test]
fn deadline_consumes_same_day_explicit_request_before_closing_and_retry_is_noop() {
    let mut actions = actions();
    actions
        .rights_subscription_queue
        .push(queued(plan().payment_deadline_on, 10));
    let resources = resource(8000, AccountKind::Player);
    let candidate = rights_offering_day_end_candidate(
        &actions,
        plan().payment_deadline_on,
        &calendar(),
        &resources,
        &BTreeSet::new(),
    )
    .unwrap();
    assert_eq!(candidate.results[0].paid_shares, 10);
    assert!(candidate.rights_subscription_queue.is_empty());
    assert_eq!(
        candidate.rights_offerings[0].status(),
        &RightsOfferingStatus::Closed
    );
    install(&mut actions, candidate);
    let retried = rights_offering_day_end_candidate(
        &actions,
        plan().payment_deadline_on,
        &calendar(),
        &resources,
        &BTreeSet::new(),
    )
    .unwrap();
    assert!(retried.intents.is_empty());
    assert!(retried.results.is_empty());
}
#[test]
fn player_has_no_automatic_subscription_and_missing_account_is_error() {
    let actions = actions();
    let candidate = rights_offering_day_end_candidate(
        &actions,
        plan().payment_start_on,
        &calendar(),
        &resource(8000, AccountKind::Player),
        &BTreeSet::new(),
    )
    .unwrap();
    assert!(candidate.results.is_empty());
    assert!(matches!(
        rights_offering_day_end_candidate(
            &actions,
            plan().payment_start_on,
            &calendar(),
            &BTreeMap::new(),
            &BTreeSet::new()
        ),
        Err(RightsOfferingCandidateError::MissingAccount {
            account: AccountId(7),
            ..
        })
    ));
}

fn settled_actions(paid: u64) -> SessionCorporateActions {
    let mut actions = actions();
    actions.rights_offerings[0]
        .record_subscription(RightsSubscriptionRecord {
            holder: HolderId::Account(AccountId(7)),
            requested_shares: 10,
            price_per_share: Money::from_cents(800),
            submitted_on: plan().payment_start_on,
            origin: SubscriptionOrigin::Explicit,
            paid_shares: paid,
            paid_amount: Money::from_cents(i64::try_from(paid * 800).unwrap()),
            waived_shares: 10 - paid,
        })
        .unwrap();
    actions.rights_offerings[0]
        .close_payment_window(plan().payment_deadline_on)
        .unwrap();
    let mut day = plan().registered_on.next().unwrap();
    while day <= plan().settlement_on {
        actions.registries[0]
            .close_day(ShareDayRequest {
                event_id: format!("market:{day}"),
                day,
                scope: MovementScope::PublicMarket,
                changes: vec![],
            })
            .unwrap();
        day = day.next().unwrap();
    }
    actions
}
#[test]
fn failed_offering_refunds_principal_without_shares_or_finance_proceeds() {
    let actions = settled_actions(6);
    let before = actions.clone();
    let candidate = rights_offering_day_end_candidate(
        &actions,
        plan().settlement_on,
        &calendar(),
        &resource(0, AccountKind::Player),
        &BTreeSet::new(),
    )
    .unwrap();
    assert_eq!(actions, before);
    assert_eq!(
        candidate.results[0].refunded_amount,
        Money::from_cents(4800)
    );
    assert_eq!(candidate.results[0].requested_shares, 10);
    assert_eq!(
        candidate.intents,
        vec![RightsOfferingIntent::FailureRefund {
            request: candidate.results[0].request.clone(),
            amount: Money::from_cents(4800)
        }]
    );
    assert_eq!(candidate.registries[0].issued_shares(), 20);
    assert_eq!(candidate.finance_settlements[0].issued_shares, 0);
    assert_eq!(candidate.finance_settlements[0].proceeds, Money::ZERO);
    assert!(candidate.rights_offerings[0].settlement().unwrap().failed);
    assert_eq!(candidate.rights_offerings[0].credited_on(), None);
}
#[test]
fn seventy_percent_success_keeps_subscription_cost_and_appends_same_day_tax_lot() {
    let mut actions = settled_actions(7);
    use crate::company::cash_dividend_tax::{
        convert_tax_source, DividendTaxLot, DividendTaxProfile, TaxShareClass,
    };
    let mut tax = CashDividendTaxBook::new(
        AccountId(7),
        plan().stock,
        DividendTaxProfile::IndividualPublicMarket,
        plan().settlement_on.prev().unwrap(),
        vec![DividendTaxLot {
            id: "initial-tax".into(),
            qty: 20,
            acquired_on: date("2020-01-02"),
            source: convert_tax_source(&AcquisitionSource::InitialAllocation {
                evidence: "opening".into(),
            }),
            class: TaxShareClass::PublicMarket,
        }],
    )
    .unwrap();
    tax.record_net_day("final-market".into(), plan().settlement_on, 0, None)
        .unwrap();
    actions.dividend_tax_books.push(tax);
    let candidate = rights_offering_day_end_candidate(
        &actions,
        plan().settlement_on,
        &calendar(),
        &resource(0, AccountKind::Player),
        &BTreeSet::from([AccountId(7)]),
    )
    .unwrap();
    assert_eq!(
        candidate.intents,
        vec![RightsOfferingIntent::NewShares {
            request: candidate.results[0].request.clone(),
            stock: plan().stock,
            shares: 7,
            paid_amount: Money::from_cents(5600)
        }]
    );
    assert_eq!(candidate.registries[0].issued_shares(), 27);
    assert_eq!(
        candidate.finance_settlements[0].proceeds,
        Money::from_cents(5600)
    );
    assert!(!candidate.rights_offerings[0].settlement().unwrap().failed);
    assert_eq!(
        candidate.rights_offerings[0].credited_on(),
        Some(plan().listing_on)
    );
    assert_eq!(
        candidate.dividend_tax_books[0]
            .lots()
            .iter()
            .map(|lot| lot.qty)
            .sum::<u64>(),
        27
    );
    let added = candidate.dividend_tax_books[0].lots().last().unwrap();
    assert_eq!((added.qty, added.acquired_on), (7, plan().listing_on));
}
#[test]
fn settlement_retry_emits_no_duplicate_refund_or_share_intent() {
    for paid in [6, 7] {
        let mut actions = settled_actions(paid);
        let resources = resource(0, AccountKind::Player);
        let first = rights_offering_day_end_candidate(
            &actions,
            plan().settlement_on,
            &calendar(),
            &resources,
            &BTreeSet::new(),
        )
        .unwrap();
        install(&mut actions, first);
        let retried = rights_offering_day_end_candidate(
            &actions,
            plan().settlement_on,
            &calendar(),
            &resources,
            &BTreeSet::new(),
        )
        .unwrap();
        assert!(retried.intents.is_empty());
        assert!(retried.finance_settlements.is_empty());
        assert_eq!(retried.registries, actions.registries);
    }
}
#[test]
fn explicit_npc_request_precedes_default_and_is_not_silently_replaced() {
    let mut actions = actions();
    actions
        .rights_subscription_queue
        .push(queued(plan().payment_start_on, 2));
    let candidate = rights_offering_day_end_candidate(
        &actions,
        plan().payment_start_on,
        &calendar(),
        &resource(8000, AccountKind::Retail),
        &BTreeSet::new(),
    )
    .unwrap();
    assert_eq!(candidate.results.len(), 1);
    assert_eq!(candidate.results[0].paid_shares, 2);
    assert_eq!(candidate.results[0].origin, SubscriptionOrigin::Explicit);
}
#[test]
fn multiple_events_share_one_final_cash_budget() {
    let mut actions = actions();
    let mut second_plan = plan();
    second_plan.event_id = "rights-second".into();
    let mut second = RightsOfferingBook::new(second_plan.clone()).unwrap();
    second.announce(second_plan.announced_on).unwrap();
    let snapshot = actions.registries[0]
        .register(second_plan.event_id, second_plan.registered_on)
        .unwrap()
        .clone();
    second.entitle(snapshot, &calendar()).unwrap();
    actions.rights_offerings.push(second);
    let candidate = rights_offering_day_end_candidate(
        &actions,
        plan().payment_start_on,
        &calendar(),
        &resource(8000, AccountKind::Retail),
        &BTreeSet::new(),
    )
    .unwrap();
    assert_eq!(
        candidate
            .results
            .iter()
            .map(|result| result.paid_shares)
            .collect::<Vec<_>>(),
        vec![10, 0]
    );
    assert_eq!(candidate.results[1].waived_shares, 10);
    assert_eq!(candidate.intents.len(), 1);
}
#[test]
fn invalid_queue_subscription_has_typed_rejection_and_no_debit() {
    let mut actions = actions();
    actions
        .rights_subscription_queue
        .push(queued(plan().payment_deadline_on, 11));
    let candidate = rights_offering_day_end_candidate(
        &actions,
        plan().payment_deadline_on,
        &calendar(),
        &resource(8800, AccountKind::Player),
        &BTreeSet::new(),
    )
    .unwrap();
    assert!(matches!(
        candidate.results[0].disposition,
        RightsAccountDisposition::Rejected { .. }
    ));
    assert_eq!(candidate.results[0].paid_amount, Money::ZERO);
    assert!(candidate.intents.is_empty());
    assert!(candidate.rights_subscription_queue.is_empty());
    assert_eq!(candidate.rejected_rights_subscriptions.len(), 1);
    assert!(candidate.rights_offerings[0].subscriptions().is_empty());
}
#[test]
fn locked_directed_plan_is_rejected_after_restore_without_rewriting_domain_support() {
    let mut locked = plan();
    locked.mode = RightsOfferingMode::DirectedPlacement {
        targets: vec![DirectedPlacementTarget::NamedHolder {
            holder: HolderId::Account(AccountId(7)),
            shares: 10,
            lock_until: Some(date("2031-01-02")),
        }],
    };
    locked.validate().unwrap();
    let actions = SessionCorporateActions {
        rights_offerings: vec![RightsOfferingBook::new(locked.clone()).unwrap()],
        ..Default::default()
    };
    let restored: SessionCorporateActions =
        serde_json::from_str(&serde_json::to_string(&actions).unwrap()).unwrap();
    assert!(matches!(
        validate_rights_offering_capability(&locked),
        Err(RightsOfferingCandidateError::UnsupportedLock { .. })
    ));
    assert!(matches!(
        rights_offering_day_end_candidate(
            &restored,
            plan().announced_on,
            &calendar(),
            &BTreeMap::new(),
            &BTreeSet::new()
        ),
        Err(RightsOfferingCandidateError::UnsupportedLock { .. })
    ));
}
#[test]
fn late_missing_account_discards_all_earlier_candidate_work() {
    let mut actions = actions();
    actions
        .rights_subscription_queue
        .push(queued(plan().payment_start_on, 2));
    let mut absent = queued(plan().payment_start_on, 2);
    absent.account = AccountId(999);
    actions.rights_subscription_queue.push(absent);
    let before = actions.clone();
    assert!(matches!(
        rights_offering_day_end_candidate(
            &actions,
            plan().payment_start_on,
            &calendar(),
            &resource(8000, AccountKind::Player),
            &BTreeSet::new()
        ),
        Err(RightsOfferingCandidateError::MissingAccount {
            account: AccountId(999),
            ..
        })
    ));
    assert_eq!(actions, before);
}

#[test]
fn duplicate_queue_request_identity_is_explicit_error_and_keeps_input() {
    let mut actions = actions();
    let queued = queued(plan().payment_start_on, 2);
    actions
        .rights_subscription_queue
        .extend([queued.clone(), queued]);
    let before = actions.clone();
    assert!(matches!(
        rights_offering_day_end_candidate(
            &actions,
            plan().payment_start_on,
            &calendar(),
            &resource(8000, AccountKind::Player),
            &BTreeSet::new()
        ),
        Err(RightsOfferingCandidateError::Invalid { .. })
    ));
    assert_eq!(actions, before);
}
#[test]
fn eligible_existing_holder_missing_tax_book_is_reconciliation_error() {
    let actions = settled_actions(7);
    let before = actions.clone();
    assert!(matches!(
        rights_offering_day_end_candidate(
            &actions,
            plan().settlement_on,
            &calendar(),
            &resource(0, AccountKind::Player),
            &BTreeSet::from([AccountId(7)])
        ),
        Err(RightsOfferingCandidateError::Invalid { .. })
    ));
    assert_eq!(actions, before);
}
#[test]
fn negative_final_cash_and_refund_overflow_are_explicit_errors() {
    let actions = actions();
    assert!(matches!(
        rights_offering_day_end_candidate(
            &actions,
            plan().payment_start_on,
            &calendar(),
            &resource(-1, AccountKind::Retail),
            &BTreeSet::new()
        ),
        Err(RightsOfferingCandidateError::NegativeCash { .. })
    ));
    let actions = settled_actions(6);
    let before = actions.clone();
    assert!(matches!(
        rights_offering_day_end_candidate(
            &actions,
            plan().settlement_on,
            &calendar(),
            &resource(i64::MAX, AccountKind::Player),
            &BTreeSet::new()
        ),
        Err(RightsOfferingCandidateError::Invalid { .. })
    ));
    assert_eq!(actions, before);
}
#[test]
fn missed_deadline_is_error_instead_of_silent_late_payment() {
    let mut actions = actions();
    actions
        .rights_subscription_queue
        .push(queued(plan().payment_deadline_on, 1));
    assert!(matches!(
        rights_offering_day_end_candidate(
            &actions,
            plan().ex_rights_on,
            &calendar(),
            &resource(8000, AccountKind::Player),
            &BTreeSet::new()
        ),
        Err(RightsOfferingCandidateError::Invalid { .. })
    ));
}
#[test]
fn directed_public_headroom_is_consumed_in_queue_order_and_oversubscription_is_rejected() {
    let mut directed = plan();
    directed.mode = RightsOfferingMode::DirectedPlacement {
        targets: vec![DirectedPlacementTarget::OpenPublicSubscription { shares: 10 }],
    };
    let mut registry = registry(directed.registered_on);
    let snapshot = registry
        .register(directed.event_id.clone(), directed.registered_on)
        .unwrap()
        .clone();
    let mut book = RightsOfferingBook::new(directed.clone()).unwrap();
    book.announce(directed.announced_on).unwrap();
    book.entitle(snapshot, &calendar()).unwrap();
    let mut actions = SessionCorporateActions {
        registries: vec![registry],
        rights_offerings: vec![book],
        ..Default::default()
    };
    actions
        .rights_subscription_queue
        .push(queued(directed.payment_deadline_on, 6));
    let mut second = queued(directed.payment_deadline_on, 5);
    second.account = AccountId(8);
    actions.rights_subscription_queue.push(second);
    let mut resources = resource(4800, AccountKind::Player);
    resources.insert(
        AccountId(8),
        RightsFinalResources {
            available_cash: Money::from_cents(4000),
            kind: AccountKind::Player,
        },
    );
    let candidate = rights_offering_day_end_candidate(
        &actions,
        directed.payment_deadline_on,
        &calendar(),
        &resources,
        &BTreeSet::new(),
    )
    .unwrap();
    assert_eq!(candidate.results[0].paid_shares, 6);
    assert!(matches!(
        candidate.results[1].disposition,
        RightsAccountDisposition::Rejected { .. }
    ));
    assert_eq!(candidate.intents.len(), 1);
    assert_eq!(
        candidate.rejected_rights_subscriptions[0].account,
        AccountId(8)
    );
}
#[test]
fn directed_first_holding_opens_personal_tax_book_from_previous_day_empty_baseline() {
    let mut directed = plan();
    directed.mode = RightsOfferingMode::DirectedPlacement {
        targets: vec![DirectedPlacementTarget::NamedHolder {
            holder: HolderId::Account(AccountId(8)),
            shares: 10,
            lock_until: None,
        }],
    };
    let mut actions = settled_actions(10);
    actions.rights_offerings.clear();
    let mut original_registry = registry(directed.registered_on);
    let snapshot = original_registry
        .register(directed.event_id.clone(), directed.registered_on)
        .unwrap()
        .clone();
    let mut book = RightsOfferingBook::new(directed.clone()).unwrap();
    book.announce(directed.announced_on).unwrap();
    book.entitle(snapshot, &calendar()).unwrap();
    book.record_subscription(RightsSubscriptionRecord {
        holder: HolderId::Account(AccountId(8)),
        requested_shares: 10,
        price_per_share: directed.price_per_share,
        submitted_on: directed.payment_start_on,
        origin: SubscriptionOrigin::Explicit,
        paid_shares: 10,
        paid_amount: Money::from_cents(8000),
        waived_shares: 0,
    })
    .unwrap();
    book.close_payment_window(directed.payment_deadline_on)
        .unwrap();
    actions.rights_offerings.push(book);
    let resources = BTreeMap::from([(
        AccountId(8),
        RightsFinalResources {
            available_cash: Money::ZERO,
            kind: AccountKind::Player,
        },
    )]);
    let candidate = rights_offering_day_end_candidate(
        &actions,
        directed.settlement_on,
        &calendar(),
        &resources,
        &BTreeSet::from([AccountId(8)]),
    )
    .unwrap();
    assert_eq!(candidate.dividend_tax_books.len(), 1);
    assert_eq!(candidate.dividend_tax_books[0].account(), AccountId(8));
    assert_eq!(candidate.dividend_tax_books[0].lots()[0].qty, 10);
    assert_eq!(
        candidate.dividend_tax_books[0].lots()[0].acquired_on,
        directed.listing_on
    );
}
#[test]
fn registration_retry_uses_final_registry_snapshot_without_duplicate_entitlement() {
    let mut actions = SessionCorporateActions {
        registries: vec![registry(plan().registered_on)],
        rights_offerings: vec![RightsOfferingBook::new(plan()).unwrap()],
        ..Default::default()
    };
    let announced = rights_offering_day_end_candidate(
        &actions,
        plan().announced_on,
        &calendar(),
        &BTreeMap::new(),
        &BTreeSet::new(),
    )
    .unwrap();
    install(&mut actions, announced);
    let registered = rights_offering_day_end_candidate(
        &actions,
        plan().registered_on,
        &calendar(),
        &BTreeMap::new(),
        &BTreeSet::new(),
    )
    .unwrap();
    assert_eq!(
        registered.rights_offerings[0]
            .entitlement()
            .unwrap()
            .planned_total_rights_shares,
        10
    );
    install(&mut actions, registered);
    let retried = rights_offering_day_end_candidate(
        &actions,
        plan().registered_on,
        &calendar(),
        &BTreeMap::new(),
        &BTreeSet::new(),
    )
    .unwrap();
    assert_eq!(retried.rights_offerings, actions.rights_offerings);
    assert_eq!(retried.registries, actions.registries);
}

#[test]
fn rejected_explicit_npc_request_does_not_fall_back_to_automatic_purchase() {
    let mut actions = actions();
    actions
        .rights_subscription_queue
        .push(queued(plan().payment_start_on, 11));
    let candidate = rights_offering_day_end_candidate(
        &actions,
        plan().payment_start_on,
        &calendar(),
        &resource(8800, AccountKind::Retail),
        &BTreeSet::new(),
    )
    .unwrap();
    assert_eq!(candidate.results.len(), 1);
    assert!(matches!(
        candidate.results[0].disposition,
        RightsAccountDisposition::Rejected { .. }
    ));
    assert!(candidate.intents.is_empty());
    assert!(candidate.rights_offerings[0].subscriptions().is_empty());
}
