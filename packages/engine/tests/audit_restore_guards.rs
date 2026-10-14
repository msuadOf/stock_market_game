use engine::calendar::{
    CalendarExchange, CalendarPolicy, CivilDate, CivilInstant, OfficialCoverageEntry,
};
use engine::company::scheduler::{OperatingScheduler, ScheduledAction, SchedulerRequest};
use engine::company::CompanyId;
use engine::plans::{PlanBook, PlanOpen};
use engine::session::CivilClock;
use engine::{
    AccountId, OpinionSource, PlanId, PlanOpinion, PlanPolicy, PlanTarget, Side, StockId,
    TradingPlan, Urgency,
};

fn date(text: &str) -> CivilDate {
    CivilDate::from_iso(text).unwrap()
}

fn open(created_trading_day: u64, horizon_trading_days: u32) -> PlanOpen {
    PlanOpen {
        account: AccountId(7),
        code: StockId(0),
        direction: Side::Buy,
        target: PlanTarget::ShareCount(100),
        opinion: PlanOpinion {
            signal_score_bp: 2000,
            source: OpinionSource::Fundamental,
        },
        confidence_bp: 6000,
        urgency: Urgency::Normal,
        horizon_trading_days,
        created_trading_day,
    }
}

#[test]
fn plan_open_rejects_unrepresentable_horizon_without_mutation() {
    let mut book = PlanBook::default();
    let before = serde_json::to_value(&book).unwrap();
    assert!(book.create(open(u64::MAX, 2)).is_err());
    assert_eq!(serde_json::to_value(book).unwrap(), before);
}

#[test]
fn plan_last_day_accepts_maximum_day_with_one_day_horizon() {
    let plan =
        TradingPlan::from_open(PlanId(1), open(u64::MAX, 1), &PlanPolicy::default()).unwrap();
    assert_eq!(plan.last_valid_trading_day(), u64::MAX);
    let restored: TradingPlan =
        serde_json::from_value(serde_json::to_value(&plan).unwrap()).unwrap();
    assert_eq!(restored.last_valid_trading_day(), u64::MAX);
}

#[test]
fn plan_deserialization_rejects_zero_and_overflowing_horizons() {
    let plan = TradingPlan::from_open(PlanId(1), open(0, 1), &PlanPolicy::default()).unwrap();
    let baseline = serde_json::to_value(plan).unwrap();
    for (created, horizon) in [(0, 0), (u64::MAX, 2), (u64::MAX - 1, 3)] {
        let mut invalid = baseline.clone();
        invalid["created_trading_day"] = serde_json::json!(created);
        invalid["horizon_trading_days"] = serde_json::json!(horizon);
        assert!(serde_json::from_value::<TradingPlan>(invalid).is_err());
    }
}

#[test]
fn civil_instant_deserialization_enforces_constructor_second_domain() {
    for seconds in [0, 86399, 86400, u32::MAX] {
        let wire = serde_json::json!({ "date": "2030-01-07", "second_of_day": seconds });
        let decoded = serde_json::from_value::<CivilInstant>(wire);
        assert_eq!(
            decoded.is_ok(),
            CivilInstant::new(date("2030-01-07"), seconds).is_ok()
        );
        if let Ok(instant) = decoded {
            assert_eq!(instant.second_of_day(), seconds);
        }
    }
}

#[test]
fn official_source_digest_changes_frozen_policy_identity() {
    let mut spec = CalendarPolicy::current_default_policy().unwrap().spec();
    spec.official_coverage.push(OfficialCoverageEntry::new(
        CalendarExchange::Sse,
        2030,
        vec![(date("2030-09-30"), date("2030-09-30"))],
        "synthetic-test-notice".into(),
        "synthetic-source-one".into(),
    ));
    let original = CalendarPolicy::from_parts(spec.clone()).unwrap();
    spec.official_coverage[0].source_digest = "synthetic-source-two".into();
    let changed = CalendarPolicy::from_parts(spec).unwrap();
    assert_ne!(original.content_digest(), changed.content_digest());
    let mut wire = serde_json::to_value(original).unwrap();
    wire["official_coverage"][0]["source_digest"] = serde_json::json!("synthetic-source-two");
    let tampered: CalendarPolicy = serde_json::from_value(wire).unwrap();
    assert!(tampered.validate().is_err());
}

fn request(key: &str, due_date: &str) -> SchedulerRequest {
    SchedulerRequest::Due {
        key: key.into(),
        due_date: date(due_date),
        action: ScheduledAction::InterestAccrual {
            company: CompanyId(0),
        },
    }
}

#[test]
fn scheduler_restore_rejects_duplicate_id_even_on_different_dates() {
    let mut scheduler = OperatingScheduler::new();
    scheduler.submit(request("first", "2030-01-07")).unwrap();
    scheduler.submit(request("second", "2030-01-08")).unwrap();
    let mut wire = serde_json::to_value(scheduler).unwrap();
    wire["pending"][1]["id"] = wire["pending"][0]["id"].clone();
    assert!(serde_json::from_value::<OperatingScheduler>(wire).is_err());
}

#[test]
fn scheduler_exhaustion_is_explicit_and_preserves_state() {
    let mut scheduler = OperatingScheduler::from_parts(u64::MAX - 1, None, vec![]).unwrap();
    assert_eq!(
        scheduler
            .submit(request("last", "2030-01-07"))
            .unwrap()
            .value(),
        u64::MAX - 1
    );
    let before = serde_json::to_value(&scheduler).unwrap();
    assert!(scheduler
        .submit(request("exhausted", "2030-01-08"))
        .is_err());
    assert_eq!(serde_json::to_value(scheduler).unwrap(), before);
}

#[test]
fn scheduler_next_due_date_tracks_earliest_pending() {
    // 公司 due 的单一 authority：下一到期自然日由调度器自己回答，
    // 会话时间推进据此取合法时点；时钟不再镜像逐项待办。
    let mut scheduler = OperatingScheduler::new();
    assert_eq!(scheduler.next_due_date(), None, "空队列无下一到期");
    scheduler.submit(request("late", "2030-01-10")).unwrap();
    scheduler.submit(request("early", "2030-01-08")).unwrap();
    assert_eq!(scheduler.next_due_date(), Some(date("2030-01-08")));
    let dispatched = scheduler.pop_due_on(date("2030-01-08")).unwrap();
    assert_eq!(dispatched.len(), 1);
    assert_eq!(scheduler.next_due_date(), Some(date("2030-01-10")));
    scheduler.pop_due_on(date("2030-01-10")).unwrap();
    assert_eq!(scheduler.next_due_date(), None);
}

#[test]
fn cancelled_due_is_absent_after_restore_and_does_not_reuse_identity() {
    use engine::company::scheduler::SchedulerError;
    let mut scheduler = OperatingScheduler::new();
    let cancelled = scheduler.submit(request("early", "2030-01-08")).unwrap();
    let retained = scheduler.submit(request("late", "2030-01-10")).unwrap();
    assert_eq!(scheduler.cancel_due(cancelled).unwrap().id, cancelled);
    assert_eq!(scheduler.next_due_date(), Some(date("2030-01-10")));
    let state = serde_json::to_vec(&scheduler).unwrap();
    assert!(matches!(scheduler.cancel_due(cancelled), Err(SchedulerError::DueNotPending { id }) if id == cancelled));
    assert_eq!(serde_json::to_vec(&scheduler).unwrap(), state, "重复取消显式拒绝且不改状态");

    let mut restored: OperatingScheduler = serde_json::from_slice(&state).unwrap();
    assert!(restored.pop_due_on(date("2030-01-08")).unwrap().is_empty(), "取消项不因恢复重新派发");
    let replacement = restored.submit(request("early", "2030-01-09")).unwrap();
    assert!(replacement.value() > retained.value(), "取消后的来源 key 可复用，但 due id 不复用");
    let replacement_due = restored.pop_due_on(date("2030-01-09")).unwrap();
    assert_eq!(replacement_due.len(), 1);
    assert_eq!(replacement_due[0].id, replacement);
    let final_due = restored.pop_due_on(date("2030-01-10")).unwrap();
    assert_eq!(final_due.len(), 1);
    assert_eq!(final_due[0].id, retained, "取消不改变无关待办的次序与身份");
}

#[test]
fn clock_due_horizon_guard_uses_frozen_policy_applicability() {
    // 公司 due 单一 authority 后，日历适用范围守卫由时钟承担（原
    // register_due 的 day_status 校验迁移）：上界内合法、越上界/越下界拒绝。
    let start = date("2030-01-07");
    let clock = CivilClock::new(start, CalendarExchange::Sse).unwrap();
    clock
        .validate_due_horizon(date("2099-12-31"))
        .expect("runtime ceiling is inside applicability");
    assert!(
        clock.validate_due_horizon(date("2100-01-01")).is_err(),
        "due 越过运行上界必须拒绝"
    );
    assert!(
        clock.validate_due_horizon(date("1997-12-31")).is_err(),
        "due 早于初始化专用下界必须拒绝"
    );
}
