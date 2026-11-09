#[path = "../../engine/tests/publications/session_fixture.rs"]
mod fixture;

use engine::session::protocol::{ProtocolSession, SaveCandidateKey};
use engine::CivilDate;
use native_store::{DayEndCandidate, DayEndExportSpool};

fn two_days() -> (DayEndCandidate, DayEndCandidate) {
    let mut game = ProtocolSession::new(
        fixture::civil_setup(CivilDate::from_iso("2030-01-05").unwrap()),
        41,
    )
    .unwrap();
    let mut capture = || {
        let update = game.end_civil_day_update().unwrap();
        DayEndCandidate::capture(
            &game,
            &SaveCandidateKey {
                seq: update.seq_to,
                settled_date: update.boundary.settled_date,
            },
        )
        .unwrap()
    };
    (capture(), capture())
}

fn key(candidate: &DayEndCandidate) -> SaveCandidateKey {
    SaveCandidateKey {
        seq: candidate.snapshot.seq,
        settled_date: candidate.civil_clock.settled_through.unwrap(),
    }
}

#[test]
fn spool_keeps_exact_days_until_every_subscriber_acknowledges() {
    let (first, second) = two_days();
    let mut spool = DayEndExportSpool::new(u64::MAX).unwrap();
    spool.subscribe("first-client").unwrap();
    spool.subscribe("second-client").unwrap();
    assert_eq!(
        spool.subscription_tokens().unwrap(),
        ["first-client", "second-client"]
    );
    spool.publish(&first).unwrap();
    let first_bytes = spool.retained_bytes().unwrap();
    assert!(first_bytes > 0);
    spool.publish(&second).unwrap();
    let both_bytes = spool.retained_bytes().unwrap();
    assert!(both_bytes > first_bytes);
    assert_eq!(spool.pending_bytes("first-client").unwrap(), both_bytes);
    assert_eq!(spool.pending_bytes("second-client").unwrap(), both_bytes);
    for client in ["first-client", "second-client"] {
        for _ in 0..2 {
            assert_eq!(
                spool.read(client, &key(&first)).unwrap().snapshot.seq,
                first.snapshot.seq
            );
        }
    }
    spool.acknowledge("first-client", &key(&first)).unwrap();
    assert_eq!(
        spool.pending_bytes("first-client").unwrap(),
        both_bytes - first_bytes
    );
    assert_eq!(spool.pending_bytes("second-client").unwrap(), both_bytes);
    assert!(spool.read("first-client", &key(&first)).is_err());
    assert_eq!(spool.retained_bytes().unwrap(), both_bytes);
    spool.acknowledge("second-client", &key(&first)).unwrap();
    assert_eq!(spool.retained_bytes().unwrap(), both_bytes - first_bytes);
    spool.unsubscribe("first-client").unwrap();
    spool.unsubscribe("second-client").unwrap();
    spool.unsubscribe("second-client").unwrap();
    assert_eq!(spool.retained_bytes().unwrap(), 0);
    assert!(spool.subscription_tokens().unwrap().is_empty());
}

#[test]
fn spool_never_retains_days_without_subscription_or_retroactively_grants_them() {
    let (first, second) = two_days();
    let mut spool = DayEndExportSpool::new(u64::MAX).unwrap();
    spool.publish(&first).unwrap();
    assert_eq!(spool.retained_bytes().unwrap(), 0);
    spool.subscribe("late-client").unwrap();
    assert!(spool.read("late-client", &key(&first)).is_err());
    spool.publish(&second).unwrap();
    assert_eq!(
        spool
            .read("late-client", &key(&second))
            .unwrap()
            .snapshot
            .seq,
        second.snapshot.seq
    );
    spool.reset().unwrap();
    assert_eq!(spool.retained_bytes().unwrap(), 0);
    assert!(spool.read("late-client", &key(&second)).is_err());
    spool.publish(&first).unwrap();
    assert_eq!(spool.retained_bytes().unwrap(), 0);
}

#[test]
fn spool_capacity_failure_preserves_pending_days_and_can_release_slow_subscriber() {
    let (first, second) = two_days();
    let bytes = serde_json::to_vec(first.as_slot()).unwrap().len() as u64;
    let mut spool = DayEndExportSpool::new(bytes).unwrap();
    spool.subscribe("slow-client").unwrap();
    spool.publish(&first).unwrap();
    assert_eq!(spool.retained_bytes().unwrap(), bytes);
    assert!(spool
        .publish(&second)
        .unwrap_err()
        .to_string()
        .contains("容量"));
    assert_eq!(spool.retained_bytes().unwrap(), bytes);
    assert_eq!(
        spool
            .read("slow-client", &key(&first))
            .unwrap()
            .snapshot
            .seq,
        first.snapshot.seq
    );
    spool.unsubscribe("slow-client").unwrap();
    assert_eq!(spool.retained_bytes().unwrap(), 0);
    spool.subscribe("replacement-client").unwrap();
    spool.publish(&first).unwrap();
    assert_eq!(spool.retained_bytes().unwrap(), bytes);
    spool
        .acknowledge("replacement-client", &key(&first))
        .unwrap();
    assert_eq!(spool.retained_bytes().unwrap(), 0);
    assert!(DayEndExportSpool::new(0).is_err());
}

#[test]
fn spool_rejects_wrong_date_unknown_subscription_and_duplicate_publication() {
    let (first, _) = two_days();
    let mut spool = DayEndExportSpool::new(u64::MAX).unwrap();
    spool.subscribe("client").unwrap();
    assert!(spool.subscribe("client").is_err());
    spool.publish(&first).unwrap();
    let bytes = spool.retained_bytes().unwrap();
    assert!(spool.publish(&first).is_err());
    let wrong = SaveCandidateKey {
        seq: first.snapshot.seq,
        settled_date: CivilDate::from_iso("2030-01-06").unwrap(),
    };
    assert!(spool.read("client", &wrong).is_err());
    assert!(spool.acknowledge("client", &wrong).is_err());
    assert!(spool.read("outsider", &key(&first)).is_err());
    assert_eq!(spool.retained_bytes().unwrap(), bytes);
}
