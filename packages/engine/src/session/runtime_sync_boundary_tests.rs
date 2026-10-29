use super::*;
use crate::session::protocol::ProtocolSession;

/// 复用真实报价夹具，并使用短交易日覆盖日界与保存边界。
fn boundary_setup(policy: crate::RuntimeSyncPolicy) -> crate::SessionSetup {
    let mut setup = super::npc_working_quote_tests::quote_setup(0);
    setup.start_date = crate::calendar::CivilDate::from_ymd(2026, 1, 5).unwrap();
    setup.ticks_per_day = 4;
    setup.auction_ticks = 1;
    setup.closing_auction_ticks = 0;
    setup.runtime_sync_policy = policy;
    setup
}

#[test]
fn game_explicit_flush_installs_current_source_without_advancing_progress_and_is_idempotent() {
    let mut game =
        GameSession::new(boundary_setup(crate::RuntimeSyncPolicy::OnDemand), 42).unwrap();
    game.step_frame().unwrap();
    let progress = (game.tick(), game.seq());
    assert_eq!(game.runtime_mirror_origin().source_tick, 0);

    game.flush_now().unwrap();
    let installed = game.runtime_mirror_origin();
    assert_eq!(installed.source_tick, 1);
    assert_eq!((game.tick(), game.seq()), progress, "flush 只安装当前来源");

    game.flush_now().unwrap();
    assert_eq!(
        game.runtime_mirror_origin(),
        installed,
        "同来源重复 flush 幂等"
    );
    assert_eq!((game.tick(), game.seq()), progress);
}

#[test]
fn protocol_flush_returns_current_snapshot_and_keeps_frame_publication_usable() {
    let mut session =
        ProtocolSession::new(boundary_setup(crate::RuntimeSyncPolicy::OnDemand), 42).unwrap();
    let first = session.step_frame().unwrap();
    let snapshot = session.flush_now().unwrap();
    assert_eq!(snapshot.tick, first.tick);
    assert_eq!(snapshot.seq, first.seq_to);
    assert_eq!(snapshot.mirror_origin.source_tick, first.tick);

    let next = session.step_frame().unwrap();
    let batch = session.tick_batch(vec![next.clone()]).unwrap();
    assert_eq!(batch.frames.len(), 1, "flush 后仍可发布真实后继帧");
    assert_eq!(batch.frames[0].tick, next.tick);
    assert_eq!(batch.frames[0].seq_to, next.seq_to);
}

#[test]
fn first_civil_day_save_reports_no_candidate_without_flushing_or_mutating_origin() {
    let mut session =
        ProtocolSession::new(boundary_setup(crate::RuntimeSyncPolicy::OnDemand), 42).unwrap();
    let start = crate::calendar::CivilDate::from_ymd(2026, 1, 5).unwrap();
    let origin = session.game().runtime_mirror_origin();

    let current_day_error = session.save_current_day(start).unwrap_err();
    assert!(matches!(current_day_error, SessionError::InvalidSave(fault) if fault.code == 3018));
    let latest_error = session.save().unwrap_err();
    assert!(matches!(latest_error, SessionError::InvalidSave(fault) if fault.code == 3015));
    let unknown = session
        .save_candidate(&crate::session::protocol::SaveCandidateKey {
            settled_date: start,
            seq: 1,
        })
        .unwrap_err();
    assert!(matches!(unknown, SessionError::InvalidSave(fault) if fault.code == 3016));
    assert_eq!(
        session.game().runtime_mirror_origin(),
        origin,
        "保存查询不得隐式 flush"
    );
}

#[test]
fn current_day_save_selects_requested_natural_day_while_latest_save_stays_at_prior_day() {
    let start = crate::calendar::CivilDate::from_ymd(2026, 1, 5).unwrap();
    let next_date = start.next().unwrap();
    let mut session =
        ProtocolSession::new(boundary_setup(crate::RuntimeSyncPolicy::OnDemand), 42).unwrap();

    for _ in 0..4 {
        session.step_frame().unwrap();
    }
    session.end_civil_day_update().unwrap();
    let settled = session.save().unwrap();
    let selected = session.save_current_day(start).unwrap();
    assert_eq!(selected.snapshot.seq, settled.snapshot.seq);
    assert_eq!(selected.civil_clock.settled_through, Some(start));
    let wrong_date = session
        .save_candidate(&crate::session::protocol::SaveCandidateKey {
            settled_date: next_date,
            seq: settled.snapshot.seq,
        })
        .unwrap_err();
    assert!(matches!(wrong_date, SessionError::InvalidSave(fault) if fault.code == 3017));

    session.step_frame().unwrap();
    let origin = session.game().runtime_mirror_origin();
    let still_latest = session.save().unwrap();
    assert_eq!(still_latest.snapshot.seq, settled.snapshot.seq);

    let next_day_error = session.save_current_day(next_date).unwrap_err();
    assert!(matches!(next_day_error, SessionError::InvalidSave(fault) if fault.code == 3018));
    assert_eq!(
        session.game().runtime_mirror_origin(),
        origin,
        "按自然日查询不得隐式 flush"
    );
}

#[test]
fn repeated_current_day_save_does_not_consume_candidate_or_flush() {
    let start = crate::calendar::CivilDate::from_ymd(2026, 1, 5).unwrap();
    let mut session =
        ProtocolSession::new(boundary_setup(crate::RuntimeSyncPolicy::OnDemand), 42).unwrap();
    for _ in 0..4 {
        session.step_frame().unwrap();
    }
    session.end_civil_day_update().unwrap();
    let origin = session.game().runtime_mirror_origin();

    let first = session.save_current_day(start).unwrap();
    let second = session.save_current_day(start).unwrap();
    assert_eq!(second.snapshot.seq, first.snapshot.seq);
    assert_eq!(second.snapshot.tick, first.snapshot.tick);
    assert_eq!(
        second.civil_clock.settled_through,
        first.civil_clock.settled_through
    );
    assert_eq!(session.game().runtime_mirror_origin(), origin);
}

#[test]
fn poisoned_sessions_replay_first_fatal_from_flush_saves_and_shared_receiver() {
    let fatal = StepFatal::InvariantViolation {
        location: "runtime_sync_boundary_tests".into(),
        description: "fixture poison".into(),
    };
    let mut game =
        GameSession::new(boundary_setup(crate::RuntimeSyncPolicy::OnDemand), 42).unwrap();
    let receiver = game.shared_ingress();
    game.poison_runtime_step(fatal.clone());
    assert_eq!(game.flush_now().unwrap_err(), fatal);
    assert!(matches!(game.save(), Err(SessionError::Step(error)) if error == fatal));
    assert!(matches!(receiver.enqueue_player_intent(
        AccountId(0),
        Intent::Cancel { code: StockId(0), id: OrderId(1) },
    ), Err(SessionError::Step(error)) if error == fatal));

    let mut protocol =
        ProtocolSession::new(boundary_setup(crate::RuntimeSyncPolicy::OnDemand), 42).unwrap();
    protocol.game_mut().poison_runtime_step(fatal.clone());
    assert_eq!(protocol.flush_now().unwrap_err(), fatal);
    assert!(
        matches!(protocol.save_current_day(protocol.game().civil_date()), Err(SessionError::Step(error)) if error == fatal)
    );
}
