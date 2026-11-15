use super::*;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

#[test]
fn 固定节拍首次等待并跳过错过的时隙() {
    let epoch = Instant::now();
    let mut cadence = Cadence::new(ms(5));
    assert_eq!(cadence.wake_plan(), WakePlan::Idle);
    cadence.set_running(true, epoch);
    assert_eq!(cadence.wake_plan(), WakePlan::At(epoch + ms(5)));
    assert!(cadence.begin_turn(epoch + ms(4)).is_none());
    assert_eq!(cadence.begin_turn(epoch + ms(17)).unwrap().limit(), 1,);
    assert_eq!(cadence.wake_plan(), WakePlan::At(epoch + ms(20)));
    assert!(cadence.begin_turn(epoch + ms(17)).is_none());
}

#[test]
fn 暂停恢复与变速重置到期但重复开跑不会重置() {
    let epoch = Instant::now();
    let mut cadence = Cadence::new(ms(5));
    cadence.set_running(true, epoch);
    cadence.set_running(true, epoch + ms(2));
    assert_eq!(cadence.wake_plan(), WakePlan::At(epoch + ms(5)));
    cadence.set_running(false, epoch + ms(3));
    assert_eq!(cadence.wake_plan(), WakePlan::Idle);
    cadence
        .set_speed(Speed::Fixed(ms(3)), epoch + ms(4))
        .unwrap();
    cadence.set_running(true, epoch + ms(6));
    assert_eq!(cadence.wake_plan(), WakePlan::At(epoch + ms(9)));
    assert_eq!(
        cadence.set_speed(Speed::Fixed(Duration::ZERO), epoch),
        Err(InvalidPeriod)
    );
    assert_eq!(cadence.wake_plan(), WakePlan::At(epoch + ms(9)));
}

#[test]
fn 最快模式只按共同硬限并维持最后固定周期() {
    let epoch = Instant::now();
    let mut cadence = Cadence::new(ms(5));
    cadence.set_speed(Speed::Fastest, epoch).unwrap();
    assert_eq!(cadence.wake_plan(), WakePlan::Idle);
    cadence.set_running(true, epoch);
    assert_eq!(cadence.wake_plan(), WakePlan::Immediate);
    assert_eq!(cadence.begin_turn(epoch).unwrap().limit(), MAX_BATCH_TICKS);
    assert_eq!(cadence.period(), ms(5));
    assert_eq!(
        fixed_tick_duration(1000, 60.0),
        Duration::from_secs_f64(1.0 / 60.0)
    );
    assert_eq!(fixed_tick_duration(1000, 1000.0), Duration::from_millis(1));
}

struct Probe {
    session: ProtocolSession,
    time: Instant,
    tick_cost: Duration,
    staged: Vec<super::super::protocol::EngineUpdate>,
    control_after: Option<u64>,
    fail_after: Option<u64>,
}

impl Probe {
    fn new(ticks_per_day: u64) -> Self {
        let mut setup = super::super::npc_working_quote_tests::quote_setup(0);
        setup.ticks_per_day = ticks_per_day;
        Self {
            session: ProtocolSession::new(setup, 42).unwrap(),
            time: Instant::now(),
            tick_cost: ms(5),
            staged: Vec::new(),
            control_after: None,
            fail_after: None,
        }
    }
}

impl DrivePort for Probe {
    type Error = std::convert::Infallible;

    fn session(&self) -> &ProtocolSession {
        &self.session
    }
    fn session_mut(&mut self) -> &mut ProtocolSession {
        &mut self.session
    }
    fn command_pending(&self) -> bool {
        self.control_after == Some(self.session.tick())
    }
    fn now(&self) -> Instant {
        self.time
    }
    fn step_frame(&mut self) -> Result<TickFrame, StepFatal> {
        if self.fail_after == Some(self.session.tick()) {
            let failure = StepFatal::InvariantViolation {
                location: "runner.test".to_owned(),
                description: "模拟后续 tick 失败".to_owned(),
            };
            return Err(self.session.game_mut().poison_runtime_step(failure));
        }
        let frame = self.session.step_frame()?;
        self.time = self.time + self.tick_cost;
        Ok(frame)
    }
    fn stage_tick_batch(&mut self, batch: TickBatch) -> Result<(), Self::Error> {
        self.staged
            .push(super::super::protocol::EngineUpdate::TickBatch(Box::new(
                batch,
            )));
        Ok(())
    }
    fn stage_civil(
        &mut self,
        civil: CivilUpdate,
        _barrier_paused: bool,
    ) -> Result<(), Self::Error> {
        self.staged
            .push(super::super::protocol::EngineUpdate::CivilUpdate(Box::new(
                civil,
            )));
        Ok(())
    }
}

#[test]
fn 已入队控制阻断下一拍且不会隐式消费() {
    let mut port = Probe::new(10);
    port.control_after = Some(1);
    let permit = Permit::for_explicit_limit(10);
    run_cycle(&mut port, permit, PausePreferences::default()).unwrap();
    assert_eq!(port.session.tick(), 1);
    assert_eq!(port.staged.len(), 1);
    run_cycle(&mut port, permit, PausePreferences::default()).unwrap();
    assert_eq!(port.session.tick(), 1);
    assert_eq!(port.staged.len(), 1);
    port.control_after = None;
    run_cycle(&mut port, permit, PausePreferences::default()).unwrap();
    assert_eq!(port.session.tick(), 4, "预算 14ms 后让位，但不抢占 tick");
}

#[test]
fn 失败之后只使用冻结成功前缀且保持真正首错() {
    use super::super::protocol::EngineUpdate;
    let mut port = Probe::new(10);
    port.fail_after = Some(1);
    let result = run_cycle(
        &mut port,
        Permit::for_explicit_limit(10),
        PausePreferences::default(),
    );
    let Err(CycleError::Advance {
        source: SessionError::Step(StepFatal::InvariantViolation { location, .. }),
        unpublished,
    }) = result
    else {
        panic!("应保留真正首错，收到 {result:?}");
    };
    assert_eq!(location, "runner.test");
    assert_eq!(unpublished, None, "冻结成功前缀已经交接");
    let [EngineUpdate::TickBatch(batch)] = port.staged.as_slice() else {
        panic!("必须只交接一个已提交批次");
    };
    assert_eq!(batch.frames.len(), 1);
    assert_eq!(batch.frames[0].tick, 1);
    assert!(batch.validate().is_ok());
}

#[test]
fn 日界先交付已提交交易批次并按同一规则暂停() {
    use super::super::protocol::{CivilUpdateKind, EngineUpdate};
    let mut port = Probe::new(2);
    let pause = PausePreferences {
        pause_after_close: true,
        pause_before_open: false,
    };
    let outcome = run_cycle(&mut port, Permit::for_explicit_limit(10), pause).unwrap();
    assert!(outcome.paused);
    let [EngineUpdate::TickBatch(batch), EngineUpdate::CivilUpdate(boundary)] =
        port.staged.as_slice()
    else {
        panic!("交易提交必须在日界交付之前，暂停后不能多次推进");
    };
    assert_eq!(batch.frames.len(), 2);
    assert!(boundary.kinds.contains(&CivilUpdateKind::AfterClose));
}

#[test]
fn 手工单步不隐式结算且待日界请求不修改权威() {
    use super::super::protocol::CivilUpdateKind;
    use super::manual::{manual_civil, manual_tick, ManualCivilError, ManualStepError};

    let mut port = Probe::new(2);
    let initial_day = port.session.civil_date();
    let initial_seq = port.session.seq();
    assert!(matches!(
        manual_tick(&mut port.session, true),
        Err(ManualStepError::MustBePaused)
    ));
    assert_eq!(port.session.tick(), 0);
    assert!(matches!(
        manual_civil(&mut port.session, PausePreferences::default()),
        Err(ManualCivilError::Rejected(_))
    ));
    assert_eq!(port.session.seq(), initial_seq);
    for tick in 1..=2 {
        let batch = manual_tick(&mut port.session, false).unwrap();
        assert_eq!(batch.frames.len(), 1);
        assert_eq!(batch.frames[0].tick, tick);
        assert!(batch.validate().is_ok());
        assert_eq!(port.session.civil_date(), initial_day);
    }
    let seq = port.session.seq();
    assert!(matches!(
        manual_tick(&mut port.session, false),
        Err(ManualStepError::BarrierPending)
    ));
    assert_eq!(port.session.tick(), 2);
    assert_eq!(port.session.seq(), seq);
    assert_eq!(port.session.civil_date(), initial_day);

    let (civil, paused) = manual_civil(
        &mut port.session,
        PausePreferences {
            pause_after_close: true,
            pause_before_open: false,
        },
    )
    .unwrap();
    assert!(civil.kinds.contains(&CivilUpdateKind::AfterClose));
    assert!(civil.validate().is_ok());
    assert!(paused);
    assert_eq!(port.session.tick(), 2);
    assert_ne!(port.session.civil_date(), initial_day);
}

#[test]
fn 旧版逐条读取在下一拍前只返回一次待交付日界() {
    use super::super::protocol::EngineUpdate;
    use super::manual::manual_next;
    let mut port = Probe::new(2);
    for expected in 1..=2 {
        assert!(matches!(
            manual_next(&mut port.session).unwrap(),
            EngineUpdate::TickBatch(batch) if batch.frames.len() == 1 && batch.frames[0].tick == expected
        ));
    }
    assert!(matches!(
        manual_next(&mut port.session).unwrap(),
        EngineUpdate::CivilUpdate(civil) if civil.tick == 2 && civil.validate().is_ok()
    ));
    assert_eq!(port.session.tick(), 2);
    assert!(matches!(
        manual_next(&mut port.session).unwrap(),
        EngineUpdate::TickBatch(batch) if batch.frames[0].tick == 3
    ));
}

#[test]
fn 共同完成阶段暂停重置采样而失败保留最后采样() {
    let now = Instant::now();
    let later = now + ms(600);
    let mut pacing = Pacing::new(1000, 0, now);
    pacing.set_running(true, 0, now);
    let running = pacing.refresh_metrics(3, later);
    assert_eq!(running.actual_multiplier, Some(5.0));
    let result = apply_cycle_pacing::<()>(
        &mut pacing,
        3,
        later,
        Err(CycleError::Advance {
            source: SessionError::InvalidSave(crate::ValidationFault::bare(1294)),
            unpublished: None,
        }),
    );
    assert!(matches!(
        result,
        Err(CycleError::Advance {
            unpublished: None,
            ..
        })
    ));
    assert!(!pacing.is_running());
    assert_eq!(
        pacing.refresh_metrics(3, later).actual_multiplier,
        Some(5.0)
    );

    pacing.set_running(true, 3, later);
    apply_cycle_pacing::<()>(&mut pacing, 4, later, Ok(CycleOutcome { paused: true })).unwrap();
    assert_eq!(pacing.cadence.wake_plan(), WakePlan::Idle);
    assert_eq!(
        pacing.refresh_metrics(4, later).actual_multiplier,
        Some(0.0)
    );
}
