#[path = "../../../packages/engine/tests/publications/session_fixture.rs"]
mod fixture;

use engine::session::{protocol::ProtocolSession, OpaqueSubjectId};
use native_store::{ArchiveWriter, NativeDatabase};
use server::{identity::IdentityService, SessionManager};

struct TemporaryDatabase(std::path::PathBuf);

impl Drop for TemporaryDatabase {
    fn drop(&mut self) {
        std::fs::remove_file(&self.0).expect("测试结束必须清理SQLite临时文件");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reopened_sqlite_keeps_control_grants_separate_from_loaded_economic_members() {
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.tmp/shared-market-restart");
    std::fs::create_dir_all(&directory).unwrap();
    let temporary = TemporaryDatabase(directory.join(format!("{}.sqlite", uuid::Uuid::new_v4())));
    let setup = fixture::civil_setup(engine::CivilDate::from_iso("2030-01-05").unwrap());
    let database = NativeDatabase::open(&temporary.0).unwrap();
    let identities = IdentityService::new(database.clone());
    let current = identities.guest().unwrap();
    let archived = identities.guest().unwrap();
    let current_subject = OpaqueSubjectId::new(current.subject.subject_id.clone()).unwrap();
    let archived_subject = OpaqueSubjectId::new(archived.subject.subject_id.clone()).unwrap();
    let manager = SessionManager::with_database(database.clone());
    let (id, created) = manager
        .new_shared_session(setup.clone(), 8, current_subject.clone())
        .unwrap();
    assert!(created);
    assert_eq!(
        database.market_controllers("shared-market").unwrap(),
        vec![current.subject.subject_id]
    );
    manager.remove(&id).unwrap().shutdown().await.unwrap();
    let mut economic_market = ProtocolSession::new(setup, 9).unwrap();
    economic_market
        .bind_market_creator(archived_subject.clone())
        .unwrap();
    let update = economic_market.end_civil_day_update().unwrap();
    let candidate = native_store::DayEndCandidate::capture(
        &economic_market,
        &engine::session::protocol::SaveCandidateKey {
            seq: update.seq_to,
            settled_date: update.boundary.settled_date,
        },
    )
    .unwrap();
    let saved_member = candidate.as_slot().market_memberships.members[&archived_subject].clone();
    ArchiveWriter::activate(database.clone(), "current")
        .unwrap()
        .save_day_end(&candidate)
        .unwrap();
    drop(economic_market);
    drop(manager);
    drop(identities);
    drop(database);
    let database = NativeDatabase::open(&temporary.0).unwrap();
    let identities = IdentityService::new(database.clone());
    assert_eq!(
        identities.authenticate(&current.token).unwrap().subject_id,
        current_subject.as_str()
    );
    let restarted = SessionManager::with_database(database.clone());
    let id = restarted.resume_session("current").unwrap().unwrap();
    let handles = restarted.lookup(&id).unwrap();
    let context = handles
        .market_context_for(current_subject.clone())
        .await
        .unwrap();
    assert!(context.can_control && context.needs_rejoin && context.member.is_none());
    assert_eq!(context.seed, "9");
    assert_eq!(context.generation, "1");
    assert!(handles
        .public_baseline_for(current_subject.clone())
        .await
        .unwrap()
        .snapshot
        .accounts
        .is_empty());
    assert!(handles.snapshot_for(current_subject.clone()).await.is_err());
    handles
        .set_running_for(1, current_subject.clone(), false)
        .await
        .unwrap();
    handles
        .set_admission_cash_for(1, current_subject.clone(), engine::Money::from_cents(12345))
        .await
        .unwrap();
    assert!(handles
        .require_control(archived_subject.clone())
        .await
        .is_err());
    assert!(handles
        .set_running_for(1, archived_subject.clone(), true)
        .await
        .is_err());
    assert_eq!(
        handles
            .resolve_member(archived_subject.clone())
            .await
            .unwrap(),
        saved_member
    );
    assert!(handles
        .join_member_for(1, current_subject.clone(), false)
        .await
        .is_err());
    let joined = handles
        .join_member_for(1, current_subject.clone(), true)
        .await
        .unwrap();
    assert_eq!(
        joined.admission_funding.external_cash,
        engine::Money::from_cents(12345)
    );
    let own = handles.snapshot_for(current_subject.clone()).await.unwrap();
    assert_eq!(
        own.accounts[&joined.account_id].cash,
        engine::Money::from_cents(12345)
    );
    assert!(own.accounts[&joined.account_id].positions.is_empty());
    assert_eq!(
        handles
            .join_member_for(1, current_subject.clone(), true)
            .await
            .unwrap(),
        joined
    );
    assert!(handles.require_control(current_subject).await.is_ok());
    assert!(handles.require_control(archived_subject).await.is_err());
    restarted.remove(&id).unwrap().shutdown().await.unwrap();
    drop(handles);
    drop(restarted);
    drop(identities);
    drop(database);
}
