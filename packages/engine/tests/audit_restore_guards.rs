use engine::calendar::{
    CalendarExchange, CalendarPolicy, CivilDate, CivilInstant, OfficialCoverageEntry,
};
use engine::company::scheduler::{OperatingScheduler, ScheduledAction, SchedulerRequest};
use engine::company::CompanyId;
use engine::plans::{PlanBook, PlanOpen};
use engine::session::{CivilClock, DueKind};
use engine::{
    AccountId, OpinionSource, PlanId, PlanOpinion, PlanPolicy, PlanTarget, Side, StockCode,
    TradingPlan, Urgency,
};

fn date(text: &str) -> CivilDate {
    CivilDate::from_iso(text).unwrap()
}

fn open(created_trading_day: u64, horizon_trading_days: u32) -> PlanOpen {
    PlanOpen {
        account: AccountId(7),
        code: StockCode("600101".into()),
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
    let mut spec = CalendarPolicy::default_v1().unwrap().spec();
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
            company: CompanyId("C-1".into()),
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
fn clock_restore_rejects_duplicate_due_identity() {
    let start = date("2030-01-07");
    let mut clock = CivilClock::new(start, CalendarExchange::Sse).unwrap();
    let first = clock.register_due(start, DueKind::InterestAccrual).unwrap();
    clock
        .register_due(date("2030-01-08"), DueKind::ContractMaturity)
        .unwrap();
    let mut save = clock.save();
    save.pending_due[1].id = first.id;
    assert!(CivilClock::from_parts(start, &save, CalendarExchange::Sse).is_err());
}

#[test]
fn clock_restore_rejects_future_due_outside_frozen_policy() {
    let start = date("2030-01-07");
    let mut clock = CivilClock::new(start, CalendarExchange::Sse).unwrap();
    clock.register_due(start, DueKind::InterestAccrual).unwrap();
    let mut save = clock.save();
    save.pending_due[0].due_date = date("2100-01-01");
    assert!(CivilClock::from_parts(start, &save, CalendarExchange::Sse).is_err());
}

#[test]
fn clock_exhaustion_is_explicit_and_preserves_state() {
    let start = date("2030-01-07");
    let mut save = CivilClock::new(start, CalendarExchange::Sse)
        .unwrap()
        .save();
    save.next_due_seq = u32::MAX - 1;
    let mut clock = CivilClock::from_parts(start, &save, CalendarExchange::Sse).unwrap();
    assert_eq!(
        clock
            .register_due(start, DueKind::InterestAccrual)
            .unwrap()
            .id
            .value(),
        u32::MAX - 1
    );
    let before = serde_json::to_value(clock.save()).unwrap();
    assert!(clock
        .register_due(start, DueKind::ContractMaturity)
        .is_err());
    assert_eq!(serde_json::to_value(clock.save()).unwrap(), before);
}

#[test]
fn empty_clock_zero_cursor_keeps_existing_restore_contract() {
    let start = date("2030-01-07");
    let mut save = CivilClock::new(start, CalendarExchange::Sse)
        .unwrap()
        .save();
    save.next_due_seq = 0;
    let mut clock = CivilClock::from_parts(start, &save, CalendarExchange::Sse).unwrap();
    assert_eq!(
        clock
            .register_due(start, DueKind::InterestAccrual)
            .unwrap()
            .id
            .value(),
        0
    );
}
