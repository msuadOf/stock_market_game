use super::{
    allocate_share_split, HolderSplitOutcome, ShareSplitBook, ShareSplitDirection,
    ShareSplitError, ShareSplitEventPlan, ShareSplitStatus,
};
use crate::company::{
    share_registry::{
        AcquisitionSource, HolderId, RegistrationSnapshot, ShareHolding, ShareLot,
        ShareRegistry, ShareRestriction,
    },
    CompanyId,
};
use crate::{account::StockCode, calendar::CivilDate, orderbook::AccountId};

fn date(value: &str) -> CivilDate {
    CivilDate::from_iso(value).unwrap()
}

fn lot(id: &str, qty: u64) -> ShareLot {
    ShareLot {
        id: id.to_owned(),
        qty,
        acquired_on: date("2020-01-02"),
        source: AcquisitionSource::InitialAllocation {
            evidence: format!("opening-{id}"),
        },
        restriction: ShareRestriction::Unrestricted,
    }
}

fn snapshot(holdings: Vec<ShareHolding>, issued_shares: u64) -> RegistrationSnapshot {
    let mut registry = ShareRegistry::new(
        StockCode("600001".to_owned()),
        CompanyId("issuer-1".to_owned()),
        issued_shares,
        date("2030-06-03"),
        holdings,
    )
    .unwrap();
    registry
        .register("split-1".to_owned(), date("2030-06-03"))
        .unwrap()
        .clone()
}

fn holding(holder: HolderId, lots: Vec<ShareLot>) -> ShareHolding {
    ShareHolding { holder, lots }
}

fn plan(direction: ShareSplitDirection, ratio: u64) -> ShareSplitEventPlan {
    ShareSplitEventPlan {
        event_id: "split-1".to_owned(),
        approval_reference: "shareholders-resolution-1".to_owned(),
        issuer: CompanyId("issuer-1".to_owned()),
        stock: StockCode("600001".to_owned()),
        exchange: crate::calendar::CalendarExchange::Sse,
        direction,
        ratio,
        approved_on: date("2030-06-01"),
        announced_on: date("2030-06-02"),
        registered_on: date("2030-06-03"),
        ex_rights_on: date("2030-06-04"),
    }
}

fn holder_outcome<'a>(
    receipt: &'a super::ShareSplitReceipt,
    holder: &HolderId,
) -> &'a HolderSplitOutcome {
    receipt
        .holders
        .iter()
        .find(|outcome| &outcome.holder == holder)
        .unwrap_or_else(|| panic!("holder {holder:?} missing from allocation"))
}

#[test]
fn split_multiplies_every_holder_including_issuer_treasury_without_fragments() {
    let snapshot = snapshot(
        vec![
            holding(HolderId::Account(AccountId(1)), vec![lot("a1", 3)]),
            holding(HolderId::Account(AccountId(2)), vec![lot("a2", 7)]),
            // 拆股／缩股不是权益分派：回购专户按同一比例换算（不排除）。
            holding(HolderId::IssuerTreasury, vec![lot("treasury", 2)]),
        ],
        12,
    );

    let receipt = allocate_share_split(&snapshot, &plan(ShareSplitDirection::Split, 10), 17)
        .expect("integer split allocation");

    assert_eq!(receipt.issued_shares_before, 12);
    assert_eq!(receipt.issued_shares_after, 120);
    assert_eq!(
        holder_outcome(&receipt, &HolderId::Account(AccountId(1))).new_shares,
        30
    );
    assert_eq!(
        holder_outcome(&receipt, &HolderId::Account(AccountId(2))).new_shares,
        70
    );
    assert_eq!(
        holder_outcome(&receipt, &HolderId::IssuerTreasury).new_shares,
        20
    );
    assert!(receipt.holders.iter().all(|outcome| {
        outcome.fractional_numerator == 0 && outcome.tie_break_award == 0
    }));
}

#[test]
fn consolidation_floors_each_holder_and_awards_aggregate_whole_shares_by_remainder() {
    // 3 + 3 + 2 = 8 股，2 并 1：基准 1 + 1 + 1，汇总目标 4 → extra 1；
    // 余数 1 = 1 > 0，同为 1 的两户由 seed 洗牌决出多得 1 股的一户。
    let snapshot = snapshot(
        vec![
            holding(HolderId::Account(AccountId(1)), vec![lot("a1", 3)]),
            holding(HolderId::Account(AccountId(2)), vec![lot("a2", 3)]),
            holding(HolderId::Account(AccountId(3)), vec![lot("a3", 2)]),
        ],
        8,
    );

    let receipt =
        allocate_share_split(&snapshot, &plan(ShareSplitDirection::Consolidate, 2), 17).unwrap();

    assert_eq!(receipt.issued_shares_before, 8);
    assert_eq!(receipt.issued_shares_after, 4);
    assert_eq!(
        receipt
            .holders
            .iter()
            .map(|outcome| outcome.fractional_numerator)
            .collect::<Vec<_>>(),
        vec![1, 1, 0]
    );
    let awarded = receipt
        .holders
        .iter()
        .filter(|outcome| outcome.tie_break_award == 1)
        .count();
    assert_eq!(awarded, 1);
    assert_eq!(
        receipt
            .holders
            .iter()
            .map(|outcome| outcome.new_shares)
            .sum::<u64>(),
        4
    );
    // 余数为零的持有人不参与整股奖励（碎股分子为零不进候选）。
    assert_eq!(
        holder_outcome(&receipt, &HolderId::Account(AccountId(3))).tie_break_award,
        0
    );
}

#[test]
fn consolidation_remainder_tie_break_is_repeatable_per_seed() {
    let snapshot = snapshot(
        (1..=8)
            .map(|id| {
                holding(
                    HolderId::Account(AccountId(id)),
                    vec![lot(&format!("a{id}"), 1)],
                )
            })
            .collect(),
        8,
    );
    let plan = plan(ShareSplitDirection::Consolidate, 4);
    // 8 股 4 并 1：基准全 0、余数全 1，汇总目标 2 → extra 2。
    let first = allocate_share_split(&snapshot, &plan, 42).unwrap();
    let repeated = allocate_share_split(&snapshot, &plan, 42).unwrap();
    assert_eq!(first, repeated);
    assert_eq!(first.issued_shares_after, 2);
    assert_eq!(
        first
            .holders
            .iter()
            .filter(|outcome| outcome.tie_break_award == 1)
            .count(),
        2
    );
    let different_seed = allocate_share_split(&snapshot, &plan, 43).unwrap();
    assert_ne!(first.holders, different_seed.holders);
    assert_eq!(different_seed.issued_shares_after, 2);
}

#[test]
fn allocation_rejects_snapshot_identity_mismatch_and_bad_ratio() {
    let snapshot = snapshot(
        vec![holding(HolderId::Account(AccountId(1)), vec![lot("a1", 4)])],
        4,
    );
    let mut wrong_stock = plan(ShareSplitDirection::Split, 2);
    wrong_stock.stock = StockCode("000002".to_owned());
    assert!(matches!(
        allocate_share_split(&snapshot, &wrong_stock, 1),
        Err(ShareSplitError::SnapshotMismatch { .. })
    ));
    let mut ratio_one = plan(ShareSplitDirection::Split, 1);
    ratio_one.ratio = 1;
    assert!(matches!(
        allocate_share_split(&snapshot, &ratio_one, 1),
        Err(ShareSplitError::InvalidPlan { .. })
    ));
}

#[test]
fn book_state_machine_advances_and_replays_restored_receipt() {
    let snapshot = snapshot(
        vec![
            holding(HolderId::Account(AccountId(1)), vec![lot("a1", 5)]),
            holding(HolderId::Account(AccountId(2)), vec![lot("a2", 4)]),
        ],
        9,
    );
    let mut book = ShareSplitBook::new(plan(ShareSplitDirection::Consolidate, 3)).unwrap();
    assert_eq!(*book.status(), ShareSplitStatus::Approved);
    book.announce(date("2030-06-02")).unwrap();
    assert_eq!(*book.status(), ShareSplitStatus::Announced);
    // 登记在公告前被拒绝。
    let mut premature = book.clone();
    premature.status = ShareSplitStatus::Approved;
    let calendar = crate::calendar::TradingCalendar::current_default_calendar().unwrap();
    assert!(premature
        .register(snapshot.clone(), &calendar)
        .is_err());
    let receipt = book
        .register(snapshot, &calendar)
        .expect("registration freezes allocation")
        .clone();
    // 9 股 3 并 1 → 3 股；5→1 余 2，4→1 余 1；extra = 3 − 2 = 1 给余数最大者。
    assert_eq!(receipt.issued_shares_after, 3);
    assert_eq!(
        holder_outcome(&receipt, &HolderId::Account(AccountId(1))).tie_break_award,
        1
    );
    assert_eq!(*book.status(), ShareSplitStatus::Registered);
    // 恢复（serde 往返 + validate）必须重放出同一回执。
    let restored: ShareSplitBook = serde_json::from_str(&serde_json::to_string(&book).unwrap())
        .expect("serialized book round-trips");
    assert_eq!(restored.receipt(), Some(&receipt));
    // 早于除权日结算被拒绝。
    assert!(book.mark_settled(date("2030-06-03")).is_err());
    book.mark_settled(date("2030-06-04")).unwrap();
    assert_eq!(*book.status(), ShareSplitStatus::Settled);
    let settled: ShareSplitBook = serde_json::from_str(&serde_json::to_string(&book).unwrap())
        .unwrap();
    assert_eq!(settled.settled_on(), Some(date("2030-06-04")));
    // 篡改回执在恢复校验中被拒。
    let mut tampered = book.clone();
    if let Some(mut_receipt) = tampered.receipt.as_mut() {
        mut_receipt.issued_shares_after += 1;
    }
    assert!(tampered.validate().is_err());
}

#[test]
fn restored_book_without_registration_serializes_strictly() {
    let book = ShareSplitBook::new(plan(ShareSplitDirection::Split, 2)).unwrap();
    let serialized = serde_json::to_string(&book).unwrap();
    let restored: ShareSplitBook = serde_json::from_str(&serialized).unwrap();
    assert_eq!(restored, book);
    // 严格持久化：未知字段显式拒绝。
    let mut injected = serde_json::from_str::<serde_json::Value>(&serialized).unwrap();
    injected
        .as_object_mut()
        .unwrap()
        .insert("surprise".into(), serde_json::json!(1));
    assert!(serde_json::from_str::<ShareSplitBook>(&injected.to_string()).is_err());
}
