#[path = "publications/session_fixture.rs"]
mod fixture;

use engine::session::protocol::*;
use engine::session::CivilPhase;
use engine::{CivilDate, DayStatus};

fn settled(date: &str) -> (ProtocolSession, TickBatch, CivilUpdate) {
    let mut session =
        ProtocolSession::new(fixture::civil_setup(CivilDate::from_iso(date).unwrap()), 41).unwrap();
    let mut frames = Vec::new();
    if session.game().civil_clock().phase() == CivilPhase::IntradayTrading {
        for _ in 0..fixture::TICKS_PER_DAY {
            frames.push(session.step_frame().unwrap());
        }
    }
    let batch = TickBatch {
        frames,
        runtime_snapshot: None,
    };
    let civil = session.end_civil_day_update().unwrap();
    (session, batch, civil)
}

#[test]
fn real_calendar_transitions_produce_exact_ordered_kinds() {
    for (date, kinds) in [
        ("2030-01-05", vec![CivilUpdateKind::CivilAdvance]),
        ("2030-01-06", vec![CivilUpdateKind::BeforeOpen]),
        ("2030-01-04", vec![CivilUpdateKind::AfterClose]),
        (
            "2030-01-02",
            vec![CivilUpdateKind::AfterClose, CivilUpdateKind::BeforeOpen],
        ),
    ] {
        let (_, _, update) = settled(date);
        assert_eq!(update.kinds, kinds);
        update.validate().unwrap();
    }
}

#[test]
fn malformed_boundary_matrix_rejects_without_relying_on_replay_guard() {
    let (_, _, original) = settled("2030-01-02");
    let (_, _, closed) = settled("2030-01-05");
    for variant in 0..10 {
        let mut bad = original.clone();
        match variant {
            0 => {
                bad.kinds.remove(0);
            }
            1 => {
                bad.boundary.settled_date = CivilDate::from_iso("2030-01-01").unwrap();
            }
            2 => {
                bad.boundary.next_date = CivilDate::from_iso("2030-01-04").unwrap();
            }
            3 => {
                bad.boundary.settled_phase = CivilPhase::ClosedDay;
            }
            4 => {
                bad.boundary.next_status = closed.boundary.next_status.clone();
            }
            5 => {
                bad.kinds.push(CivilUpdateKind::BeforeOpen);
            }
            6 => {
                bad.kinds.reverse();
            }
            7 => {
                bad.refresh.intraday.clear();
            }
            8 => {
                bad.refresh.intraday.remove(0);
            }
            9 => {
                bad.civil_date = "2030-01-04".into();
            }
            _ => unreachable!(),
        }
        assert!(bad.validate().is_err(), "variant {variant}");
    }
    for kind in [CivilUpdateKind::AfterClose, CivilUpdateKind::BeforeOpen] {
        let mut bad = closed.clone();
        bad.kinds = vec![kind];
        assert!(bad.validate().is_err());
    }
    let mut bad_phase = closed.clone();
    bad_phase.boundary.settled_phase = CivilPhase::IntradayTrading;
    assert!(bad_phase.validate().is_err());
    assert!(matches!(original.boundary.next_status, DayStatus::Trading));
}

#[test]
fn civil_replay_is_ordered_atomic_and_detects_coherent_payload_changes() {
    let (mut session, batch, civil) = settled("2030-01-02");
    let mut guard = ReplayGuard::new(0, 0);
    assert_eq!(
        guard.ingest(&EngineUpdate::TickBatch(batch)).unwrap(),
        ReplayDecision::Applied
    );
    let barrier = EngineUpdate::CivilUpdate(Box::new(civil.clone()));
    assert_eq!(guard.ingest(&barrier).unwrap(), ReplayDecision::Applied);
    let next = session.step_frame().unwrap();
    let following = EngineUpdate::TickBatch(TickBatch {
        frames: vec![next.clone()],
        runtime_snapshot: None,
    });
    assert_eq!(guard.ingest(&following).unwrap(), ReplayDecision::Applied);
    let mut reversed = civil.clone();
    reversed.events.reverse();
    reversed.facts.reverse();
    for frame in &mut reversed.refresh.intraday {
        frame.events.reverse();
        frame.facts.reverse();
    }
    assert_eq!(
        guard
            .ingest(&EngineUpdate::CivilUpdate(Box::new(reversed)))
            .unwrap(),
        ReplayDecision::ExactRetry
    );
    let mut changed = civil.clone();
    changed.refresh.securities[0].total_shares += 100;
    changed.validate().unwrap();
    assert_eq!(
        guard.ingest(&EngineUpdate::CivilUpdate(Box::new(changed))),
        Err(ProtocolError::ReplayMismatch)
    );
    assert_eq!(
        ReplayGuard::new(0, 0).ingest(&barrier),
        Err(ProtocolError::ReplayMismatch)
    );
    assert_eq!(
        ReplayGuard::new(civil.tick, civil.seq_from).ingest(&following),
        Err(ProtocolError::ReplayMismatch)
    );
    assert_eq!(
        ReplayGuard::new(next.tick, next.seq_to).ingest(&following),
        Err(ProtocolError::ReplayMismatch)
    );
}
