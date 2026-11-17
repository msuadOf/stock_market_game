use super::*;

#[test]
fn first_terminal_survives_late_recoverable_failure_and_restore_clears_it() {
    let now = Instant::now();
    let mut authority = Authority::new(1_000, 0, now);
    let fatal = HostFailure::step(StepFatal::InvariantViolation {
        location: "authority.test".into(),
        description: "不得公开".into(),
    });
    assert!(authority.latch(fatal.clone(), now).is_some());
    assert!(authority
        .latch(HostFailure::archive("磁盘错误"), now)
        .is_some());
    assert_eq!(authority.fatal_failure(), Some(&fatal));
    assert_eq!(authority.latest_failure(), Some(&fatal));
    assert!(!authority.is_running());
    assert!(authority.require_control(None).is_err());
    authority.reset_after_restore(0, now);
    assert!(authority.fatal_failure().is_none());
    assert!(authority.require_control(None).is_ok());
}

#[test]
fn blocked_archive_cannot_be_resumed_or_mistaken_for_terminal() {
    let now = Instant::now();
    let mut authority = Authority::new(1_000, 0, now);
    let failure = HostFailure::archive("候选尚未耐久");
    assert!(failure.recoverable);
    authority.latch(failure, now);
    assert!(authority.fatal_failure().is_none());
    assert_eq!(
        authority.latest_failure().unwrap().code,
        "DAY_END_ARCHIVE_FAILED"
    );
    assert!(authority.require_control(Some("候选尚未耐久")).is_err());
    assert!(authority.require_control(None).is_ok());
}

#[test]
fn failure_mapping_preserves_committed_prefix_and_shared_recovery_policy() {
    let fatal = StepFatal::InvariantViolation {
        location: "fixture".into(),
        description: "私有事实".into(),
    };
    let range = super::super::runner::CommittedRange {
        tick_from: 1,
        tick_to: 3,
        seq_from: 1,
        seq_to: 6,
    };
    let failure = classify_cycle(CycleError::<HostFailure>::Batch {
        source: fatal,
        unpublished: range,
    });
    assert_eq!(failure.code, "STEP_FATAL");
    assert!(failure.message.contains("tick_from=1"));
    assert!(failure.message.contains("seq_to=6"));
    assert!(!failure.message.contains("私有事实"));
    let archive = HostFailure::archive("磁盘满");
    assert_eq!(archive.r#where, "session.authority.save_day_end");
    assert_eq!(archive.context.operation, "saveDayEnd");
    assert!(archive
        .recovery_actions
        .iter()
        .any(|action| action.contains("当前日终")));
}

struct Fixture {
    session: ProtocolSession,
    authority: Authority,
    blocked: bool,
    delivered: usize,
}
impl Fixture {
    fn new() -> Self {
        let mut setup = super::super::npc_working_quote_tests::quote_setup(0);
        setup.ticks_per_day = 1;
        Self {
            session: ProtocolSession::new(setup, 7).unwrap(),
            authority: Authority::new(1_000, 0, Instant::now()),
            blocked: false,
            delivered: 0,
        }
    }
}
impl DrivePort for Fixture {
    type Error = HostFailure;
    fn session(&self) -> &ProtocolSession {
        &self.session
    }
    fn session_mut(&mut self) -> &mut ProtocolSession {
        &mut self.session
    }
    fn command_pending(&self) -> bool {
        false
    }
    fn stage_tick_batch(
        &mut self,
        _: super::super::protocol::TickBatch,
    ) -> Result<(), HostFailure> {
        self.delivered += 1;
        Ok(())
    }
    fn stage_civil(
        &mut self,
        _: super::super::protocol::CivilUpdate,
        _: bool,
    ) -> Result<(), HostFailure> {
        self.delivered += 1;
        Ok(())
    }
}
impl AuthorityPort for Fixture {
    fn authority(&mut self) -> &mut Authority {
        &mut self.authority
    }
    fn archive_blocked(&self) -> bool {
        self.blocked
    }
}
#[test]
fn shared_manual_commands_reject_without_advancing_or_publishing() {
    let mut fixture = Fixture::new();
    let before = (fixture.session.tick(), fixture.session.seq());
    fixture.blocked = true;
    assert!(manual_tick(&mut fixture).result.is_err());
    assert!(manual_civil(&mut fixture, Default::default())
        .result
        .is_err());
    fixture.blocked = false;
    let tick = fixture.session.tick();
    fixture.authority.set_running(true, tick, Instant::now());
    assert!(manual_tick(&mut fixture).result.is_err());
    assert_eq!((fixture.session.tick(), fixture.session.seq()), before);
    assert_eq!(fixture.delivered, 0);
}

#[test]
fn shutdown_never_turns_unconfirmed_queue_or_unknown_io_into_success() {
    assert!(finish_shutdown(Ok(()), Ok(()), Err("仅已排队".into()))
        .unwrap_err()
        .contains("仅已排队"));
    let failure = finish_shutdown(
        Err("入口关闭失败".into()),
        Err("写入结果未知".into()),
        Err("尚未确认耐久".into()),
    )
    .unwrap_err();
    assert!(failure.contains("入口关闭失败"));
    assert!(failure.contains("写入结果未知"));
    assert!(failure.contains("尚未确认耐久"));
    assert!(finish_shutdown(Ok(()), Ok(()), Ok(())).is_ok());
}

#[test]
fn successful_civil_commit_clears_correction_warning_but_resume_alone_does_not() {
    let mut fixture = Fixture::new();
    let source = SessionError::InvalidSave(crate::error::ValidationFault::bare(3009));
    fixture.authority.latch(
        HostFailure::correction_prepare_rejected(&source),
        Instant::now(),
    );
    fixture
        .authority
        .control_running(true, 0, Instant::now(), None)
        .unwrap();
    assert!(fixture.authority.latest_failure().is_some());
    fixture
        .authority
        .control_running(false, 0, Instant::now(), None)
        .unwrap();
    manual_tick(&mut fixture).result.unwrap();
    manual_civil(&mut fixture, Default::default())
        .result
        .unwrap();
    assert!(
        fixture.authority.latest_failure().is_none(),
        "成功日界后的baseline不再报告旧更正失败"
    );
}
