//! 显式文件导出的 actor 生命周期：每客户端持有、精确确认及资源回收。

use super::correction_recovery_tests::{correction_actor, correction_controller, correction_game};
use super::*;

fn key(
    update: &engine::session::protocol::CivilUpdate,
) -> engine::session::protocol::SaveCandidateKey {
    engine::session::protocol::SaveCandidateKey {
        seq: update.seq_to,
        settled_date: update.boundary.settled_date,
    }
}

#[test]
fn export_subscribers_keep_each_weekend_candidate_after_archive_capture_and_baseline() {
    let (game, _) = correction_game("2030-06-08");
    let (mut actor, _, mut receiver) = correction_actor(game);
    actor.pause_preferences = PausePreferences::default();
    let database = native_store::NativeDatabase::open_in_memory().unwrap();
    actor.archive =
        Some(native_store::ArchiveWriter::activate(database, "export-weekend").unwrap());
    let subject = correction_controller();
    let (failure_tx, _failure_rx) = mpsc::unbounded_channel();
    let first = actor
        .subscribe_day_end_exports(1, &subject, "connection-one", failure_tx.clone())
        .unwrap();
    let second = actor
        .subscribe_day_end_exports(1, &subject, "connection-two", failure_tx)
        .unwrap();
    actor.prepare_civil_updates().unwrap();
    let mut keys = Vec::new();
    while let Ok(message) = receiver.try_recv() {
        if let Some(ProtocolUpdate::CivilUpdate(update)) = message.update {
            keys.push(key(&update));
        }
    }
    assert_eq!(keys.len(), 2, "周六和周日必须逐日日结");
    actor.game.prepare_public_baseline();
    for candidate in &keys {
        let a = actor
            .read_day_end_export(1, &subject, &first, candidate)
            .unwrap();
        let b = actor
            .read_day_end_export(1, &subject, &second, candidate)
            .unwrap();
        assert_eq!(a.snapshot.seq, candidate.seq);
        assert_eq!(
            serde_json::to_value(a).unwrap(),
            serde_json::to_value(b).unwrap()
        );
        actor
            .ack_day_end_export(1, &subject, &first, candidate)
            .unwrap();
        assert!(actor
            .read_day_end_export(1, &subject, &first, candidate)
            .is_err());
        assert_eq!(
            actor
                .read_day_end_export(1, &subject, &second, candidate)
                .unwrap()
                .snapshot
                .seq,
            candidate.seq
        );
        actor
            .ack_day_end_export(1, &subject, &second, candidate)
            .unwrap();
    }
    assert_eq!(actor.day_end_exports.retained_bytes().unwrap(), 0);
}

#[test]
fn export_owner_generation_and_disconnect_cleanup_are_strict() {
    let (game, _) = correction_game("2030-06-09");
    let (mut actor, _, _) = correction_actor(game);
    let subject = correction_controller();
    let (failure_tx, _failure_rx) = mpsc::unbounded_channel();
    let token = actor
        .subscribe_day_end_exports(1, &subject, "connection-one", failure_tx)
        .unwrap();
    let update = actor.game.end_civil_day_update().unwrap();
    actor.publish_civil_update(update.clone());
    let candidate = key(&update);
    let stranger = OpaqueSubjectId::new("different-subject".into()).unwrap();
    assert!(actor
        .read_day_end_export(1, &stranger, &token, &candidate)
        .is_err());
    assert!(actor
        .ack_day_end_export(2, &subject, &token, &candidate)
        .is_err());
    assert!(actor
        .unsubscribe_day_end_exports(1, &subject, "another-connection", &token)
        .is_err());
    assert!(actor
        .read_day_end_export(1, &subject, &token, &candidate)
        .is_ok());
    actor.drop_day_end_export_connection("connection-one");
    assert!(actor
        .read_day_end_export(1, &subject, &token, &candidate)
        .is_err());
    assert_eq!(actor.day_end_exports.retained_bytes().unwrap(), 0);
}

#[test]
fn export_capacity_failure_cancels_export_and_keeps_trading_and_archive_healthy() {
    let (game, _) = correction_game("2030-06-09");
    let (mut actor, _, _) = correction_actor(game);
    actor.day_end_exports = DayEndExports::new(1);
    let database = native_store::NativeDatabase::open_in_memory().unwrap();
    actor.archive =
        Some(native_store::ArchiveWriter::activate(database.clone(), "export-capacity").unwrap());
    let (failure_tx, mut failure_rx) = mpsc::unbounded_channel();
    let token = actor
        .subscribe_day_end_exports(1, &correction_controller(), "connection", failure_tx)
        .unwrap();
    let update = actor.game.end_civil_day_update().unwrap();
    actor.publish_civil_update(update.clone());
    let failure = failure_rx.try_recv().unwrap();
    assert_eq!(failure.subscription, token);
    assert_eq!(failure.code, "DAY_END_EXPORT_CAPACITY");
    assert!(actor.game.require_healthy().is_ok());
    assert!(actor.fatal_failure.is_none());
    assert!(database.load("export-capacity").unwrap().is_some());
    assert_eq!(actor.day_end_exports.retained_bytes().unwrap(), 0);
}

#[tokio::test]
async fn export_restore_releases_candidates_and_notifies_only_export_subscribers() {
    let (game, _) = correction_game("2030-06-09");
    let (mut actor, _, _) = correction_actor(game);
    let subject = correction_controller();
    let (failure_tx, mut failure_rx) = mpsc::unbounded_channel();
    let token = actor
        .subscribe_day_end_exports(1, &subject, "connection", failure_tx)
        .unwrap();
    let update = actor.game.end_civil_day_update().unwrap();
    actor.publish_civil_update(update.clone());
    assert!(actor.day_end_exports.retained_bytes().unwrap() > 0);
    let slot = actor.game.save().unwrap();
    let (reply, receive) = oneshot::channel();
    actor
        .handle_command(SessionCommand::Restore {
            slot: Box::new(slot),
            reply,
        })
        .await;
    assert!(receive.await.unwrap().is_ok());
    assert_eq!(actor.timeline_generation, 2);
    assert_eq!(failure_rx.try_recv().unwrap().subscription, token);
    assert_eq!(actor.day_end_exports.retained_bytes().unwrap(), 0);
    assert!(actor
        .read_day_end_export(1, &subject, &token, &key(&update))
        .is_err());
}

#[tokio::test]
async fn export_reset_releases_old_generation_and_allows_new_subscription() {
    let (game, _) = correction_game("2030-06-09");
    let (mut actor, _, _) = correction_actor(game);
    let subject = correction_controller();
    let (failure_tx, mut failure_rx) = mpsc::unbounded_channel();
    let token = actor
        .subscribe_day_end_exports(1, &subject, "connection", failure_tx.clone())
        .unwrap();
    let setup = actor.game.current_setup().clone();
    let (reply, receive) = oneshot::channel();
    actor
        .handle_command(SessionCommand::Member {
            subject: subject.clone(),
            request: MemberRequest::Reset {
                generation: 1,
                setup: Box::new(setup),
                seed: 7181,
            },
            reply,
        })
        .await;
    assert!(matches!(
        receive.await.unwrap(),
        Ok(MemberResponse::Snapshot(_))
    ));
    assert_eq!(actor.timeline_generation, 2);
    assert_eq!(failure_rx.try_recv().unwrap().subscription, token);
    assert_eq!(actor.day_end_exports.retained_bytes().unwrap(), 0);
    assert!(actor
        .subscribe_day_end_exports(1, &subject, "connection", failure_tx.clone())
        .is_err());
    assert!(actor
        .subscribe_day_end_exports(2, &subject, "connection", failure_tx)
        .is_ok());
}

#[tokio::test]
async fn export_disconnect_command_reclaims_only_its_connection() {
    let (game, _) = correction_game("2030-06-09");
    let (mut actor, sender, _) = correction_actor(game);
    let (failure_tx, _failure_rx) = mpsc::unbounded_channel();
    let token = actor
        .subscribe_day_end_exports(1, &correction_controller(), "cancelled-ws", failure_tx)
        .unwrap();
    sender
        .send(SessionCommand::DropDayEndExportConnection {
            connection: "cancelled-ws".into(),
        })
        .unwrap();
    let command = actor.cmd_rx.try_recv().unwrap();
    actor.handle_command(command).await;
    assert!(!actor.day_end_exports.has_subscribers());
    assert!(!token.is_empty());
}

#[tokio::test]
async fn export_subscription_watermark_excludes_already_published_ws_backlog() {
    let (game, _) = correction_game("2030-06-08");
    let (mut actor, _, mut backlog) = correction_actor(game);
    let first = actor.game.end_civil_day_update().unwrap();
    actor.publish_civil_update(first.clone());
    let (failures, _failure_rx) = mpsc::unbounded_channel();
    let (reply, receive) = oneshot::channel();
    actor
        .handle_command(SessionCommand::DayEndExport {
            generation: 1,
            subject: correction_controller(),
            request: DayEndExportRequest::Subscribe {
                connection: "backlogged-ws".into(),
                failures,
            },
            reply,
        })
        .await;
    let DayEndExportResponse::Subscribed {
        subscription,
        after_seq,
    } = receive.await.unwrap().unwrap()
    else {
        panic!("订阅必须返回边界")
    };
    assert_eq!(after_seq, first.seq_to);
    let message = backlog.try_recv().unwrap();
    let Some(ProtocolUpdate::CivilUpdate(old)) = message.update else {
        panic!("必须保留真实 WS backlog")
    };
    assert!(
        old.seq_to <= after_seq,
        "ACK 即使先交付，客户端也能识别订阅前日终"
    );
    assert!(actor
        .read_day_end_export(1, &correction_controller(), &subscription, &key(&old))
        .is_err());
    let second = actor.game.end_civil_day_update().unwrap();
    actor.publish_civil_update(second.clone());
    assert!(second.seq_to > after_seq);
    assert!(actor
        .read_day_end_export(1, &correction_controller(), &subscription, &key(&second))
        .is_ok());
}

#[test]
fn export_capacity_evicts_slow_owner_and_keeps_acknowledged_client_subscribed() {
    let (game, _) = correction_game("2030-06-08");
    let (mut actor, _, _) = correction_actor(game);
    let first = actor.game.end_civil_day_update().unwrap();
    let first_bytes = serde_json::to_vec(&actor.game.save().unwrap())
        .unwrap()
        .len() as u64;
    let second = actor.game.end_civil_day_update().unwrap();
    let second_bytes = serde_json::to_vec(&actor.game.save().unwrap())
        .unwrap()
        .len() as u64;
    actor.day_end_exports = DayEndExports::new(first_bytes.max(second_bytes));
    let (fast_tx, mut fast_rx) = mpsc::unbounded_channel();
    let (slow_tx, mut slow_rx) = mpsc::unbounded_channel();
    let subject = correction_controller();
    let fast = actor
        .subscribe_day_end_exports(1, &subject, "fast", fast_tx)
        .unwrap();
    let slow = actor
        .subscribe_day_end_exports(1, &subject, "slow", slow_tx)
        .unwrap();
    actor.publish_civil_update(first.clone());
    actor
        .ack_day_end_export(1, &subject, &fast, &key(&first))
        .unwrap();
    actor.publish_civil_update(second.clone());
    assert_eq!(slow_rx.try_recv().unwrap().subscription, slow);
    assert!(fast_rx.try_recv().is_err());
    assert!(actor
        .read_day_end_export(1, &subject, &fast, &key(&second))
        .is_ok());
    assert!(actor
        .read_day_end_export(1, &subject, &slow, &key(&second))
        .is_err());
}

#[test]
fn export_archive_write_failure_does_not_discard_the_file_candidate() {
    let (game, _) = correction_game("2030-06-09");
    let (mut actor, _, _) = correction_actor(game);
    let database = native_store::NativeDatabase::open_in_memory().unwrap();
    actor.archive = Some(
        native_store::ArchiveWriter::activate(database.clone(), "export-write-failure").unwrap(),
    );
    database
        .with_connection(|connection| {
            connection
                .execute_batch("PRAGMA query_only = ON")
                .map_err(Into::into)
        })
        .unwrap();
    let (failure_tx, mut failures) = mpsc::unbounded_channel();
    let subject = correction_controller();
    let token = actor
        .subscribe_day_end_exports(1, &subject, "file", failure_tx)
        .unwrap();
    let update = actor.game.end_civil_day_update().unwrap();
    actor.publish_civil_update(update.clone());
    assert!(actor
        .read_day_end_export(1, &subject, &token, &key(&update))
        .is_ok());
    assert!(failures.try_recv().is_err());
    assert!(actor.game.require_healthy().is_ok());
    assert!(actor.fatal_failure.is_none());
}
