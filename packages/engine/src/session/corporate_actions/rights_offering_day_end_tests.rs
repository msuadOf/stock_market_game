//! 少量账户覆盖配股缴款、关窗、退款与发行分腿，不启动市场线程。
use super::*;
use crate::company::rights_offering::{RightsOfferingStatus, RightsSubscriptionStrategy};
use crate::company::share_registry::{AcquisitionSource, HolderId, ShareHolding, ShareLot, ShareRestriction};

fn date(value: &str) -> CivilDate { CivilDate::from_iso(value).unwrap() }
fn calendar() -> TradingCalendar { TradingCalendar::current_default_calendar().unwrap() }
fn plan() -> RightsOfferingEventPlan {
    RightsOfferingEventPlan {
        event_id: "rights-r3b".into(), approval_reference: "approved".into(), issuer: CompanyId(0),
        stock: StockCode("000001".into()), exchange: crate::calendar::CalendarExchange::Szse,
        approved_on: date("2030-06-01"), announced_on: date("2030-06-02"), registered_on: date("2030-06-03"),
        payment_start_on: date("2030-06-04"), payment_deadline_on: date("2030-06-11"),
        ex_rights_on: date("2030-06-12"), settlement_on: date("2030-06-13"), listing_on: date("2030-06-13"),
        price_per_share: Money::from_cents(800),
        mode: RightsOfferingMode::RightsToAllShareholders { shares_per_existing_share_micros: 500_000 },
        npc_subscription_strategy: RightsSubscriptionStrategy::FullByDefault,
    }
}
fn registry(on: CivilDate) -> ShareRegistry {
    ShareRegistry::new(plan().stock, CompanyId(0), 20, on, vec![ShareHolding {
        holder: HolderId::Account(AccountId(7)), lots: vec![ShareLot {
            id: "initial".into(), qty: 20, acquired_on: date("2020-01-02"),
            source: AcquisitionSource::InitialAllocation { evidence: "opening".into() },
            restriction: ShareRestriction::Unrestricted,
        }],
    }]).unwrap()
}
fn actions() -> SessionCorporateActions {
    let mut registry = registry(plan().registered_on);
    let snapshot = registry.register(plan().event_id, plan().registered_on).unwrap().clone();
    let mut book = RightsOfferingBook::new(plan()).unwrap();
    book.announce(plan().announced_on).unwrap();
    book.entitle(snapshot, &calendar()).unwrap();
    SessionCorporateActions { registries: vec![registry], rights_offerings: vec![book], ..Default::default() }
}
fn resource(cents: i64, kind: AccountKind) -> BTreeMap<AccountId, RightsFinalResources> {
    BTreeMap::from([(AccountId(7), RightsFinalResources { available_cash: Money::from_cents(cents), kind })])
}
fn queued(on: CivilDate, requested_shares: u64) -> QueuedRightsSubscription {
    QueuedRightsSubscription { event_id: plan().event_id, account: AccountId(7), submitted_on: on, requested_shares }
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
    let actions = actions(); let before = actions.clone(); let resources = resource(2500, AccountKind::Retail);
    let candidate = rights_offering_day_end_candidate(&actions, plan().payment_start_on, &calendar(), &resources).unwrap();
    assert_eq!(actions, before);
    assert_eq!(resources[&AccountId(7)].available_cash, Money::from_cents(2500));
    assert_eq!(candidate.results.len(), 1);
    let result = &candidate.results[0];
    assert_eq!((result.requested_shares, result.paid_shares, result.waived_shares), (10, 3, 7));
    assert_eq!(result.paid_amount, Money::from_cents(2400));
    assert_eq!(result.disposition, RightsAccountDisposition::PreparedSubscription);
    assert_eq!(candidate.intents, vec![RightsOfferingIntent::PaymentDebit { request: result.request.clone(), amount: Money::from_cents(2400) }]);
}
#[test]
fn deadline_consumes_same_day_explicit_request_before_closing_and_retry_is_noop() {
    let mut actions = actions(); actions.rights_subscription_queue.push(queued(plan().payment_deadline_on, 10));
    let resources = resource(8000, AccountKind::Player);
    let candidate = rights_offering_day_end_candidate(&actions, plan().payment_deadline_on, &calendar(), &resources).unwrap();
    assert_eq!(candidate.results[0].paid_shares, 10);
    assert!(candidate.rights_subscription_queue.is_empty());
    assert_eq!(candidate.rights_offerings[0].status(), &RightsOfferingStatus::Closed);
    install(&mut actions, candidate);
    let retried = rights_offering_day_end_candidate(&actions, plan().payment_deadline_on, &calendar(), &resources).unwrap();
    assert!(retried.intents.is_empty()); assert!(retried.results.is_empty());
}
#[test]
fn player_has_no_automatic_subscription_and_missing_account_is_error() {
    let actions = actions();
    let candidate = rights_offering_day_end_candidate(&actions, plan().payment_start_on, &calendar(), &resource(8000, AccountKind::Player)).unwrap();
    assert!(candidate.results.is_empty());
    assert!(matches!(rights_offering_day_end_candidate(&actions, plan().payment_start_on, &calendar(), &BTreeMap::new()), Err(RightsOfferingCandidateError::MissingAccount { account: AccountId(7), .. })));
}
