//! 自然日经营时钟集成测试（company-information-npc-intentions W1-Task 5）。
//!
//! K1 双时钟语义：交易日 tick/市场分钟照旧推进，收盘后经当日经营终局窗口与
//! 18:00 披露阶段才前进自然日；休市日不产生 tick/成交/注意力 RNG 消费，当日
//! 到期业务恰好处理一次。所有失败先验证并原子生效（状态零部分变更）。
//!
//! 利息/到期"业务"本身由任务 14 实现；这里用 DueKind fixture 证明时钟侧
//! 恰好一次派发。场景锚定 2030 年春节：02-02（周六，除夕）至 02-05（周二，
//! 正月初三）连续 4 个休市自然日，前一个交易日为周五 02-01，下一个交易日为
//! 周三 02-06。

use engine::account::StockCode;
use engine::calendar::{CivilDate, CivilInstant, ClosedReason, DayStatus, HolidayKind};
use engine::money::Money;
use engine::orderbook::{AccountId, Side};
use engine::session::{
    CivilClockError, CivilDayEndReport, CivilPhase, DueBusiness, DueKind, Event, GameSession,
    NpcSetup, RejectionReason, SecurityCategory, SessionError, SessionSetup, StockExchange,
    StockSpec,
};
use engine::strategy::Intent;
use engine::{FloatAllocation, GameConfig};
use std::sync::Mutex;

/// 春节周末前最后一个交易日（周五）。
const PRE_HOLIDAY_FRIDAY: &str = "2030-02-01";
/// 春节休市四天：02-02（除夕/周六）..= 02-05（正月初三/周二）。
const SPRING_FESTIVAL_CLOSED: [&str; 4] = ["2030-02-02", "2030-02-03", "2030-02-04", "2030-02-05"];
/// 节后第一个交易日（周三）。
const POST_HOLIDAY_WEDNESDAY: &str = "2030-02-06";
/// 压缩测试时钟：每日 120 tick、无竞价窗口。
const TICKS_PER_DAY: u64 = 120;

fn date(iso: &str) -> CivilDate {
    CivilDate::from_iso(iso).expect("test dates are valid ISO civil dates")
}

fn civil_setup(start: &str) -> SessionSetup {
    SessionSetup {
        stocks: vec![StockSpec {
            code: StockCode("600101".to_string()),
            exchange: StockExchange::Shanghai,
            initial_price: Money::from_cents(1000),
            category: SecurityCategory::MainBoard,
            limit_pct: 0.10,
            tick: Money::from_cents(1),
            total_shares: 10_000_000,
            float_shares: 1_000_000,
        }],
        npcs: NpcSetup {
            retail_count: 16,
            inst_count: 1,
            hot_count: 1,
            retail_cash_median: Money::from_cents(10_000_000),
        },
        config: GameConfig::proposed_defaults(),
        strategy_params: engine::StrategyParams {
            retail: engine::RetailParams {
                arrival_rate: 1.0,
                order_size_mean: 100,
                chase_prob: 0.2,
                tick_cents: 1,
            },
            inst: engine::InstParams {
                margin: 0.05,
                order_size: 200,
            },
            hot: engine::HotParams {
                lookback: 3,
                trend_threshold: 0.02,
                order_size: 200,
            },
        },
        ticks_per_day: TICKS_PER_DAY,
        auction_ticks: 0,
        closing_auction_ticks: 0,
        history_len: 5,
        t1_enabled: true,
        float_allocation: FloatAllocation::Random,
        start_date: date(start),
    }
}

/// 春节场景：周五开局并注册整段窗口的到期业务，返回 (session, 注册清单)。
/// 注册顺序刻意与日期顺序不同，证明派发按日期而非注册顺序。
fn spring_festival_session() -> (GameSession, Vec<DueBusiness>) {
    let mut session = GameSession::new(civil_setup(PRE_HOLIDAY_FRIDAY), 42)
        .expect("spring festival setup must be valid");
    let mut registered = Vec::new();
    for (due_date, kind) in [
        (POST_HOLIDAY_WEDNESDAY, DueKind::InterestAccrual),
        (SPRING_FESTIVAL_CLOSED[2], DueKind::InterestAccrual),
        (SPRING_FESTIVAL_CLOSED[0], DueKind::InterestAccrual),
        (PRE_HOLIDAY_FRIDAY, DueKind::InterestAccrual),
        (SPRING_FESTIVAL_CLOSED[3], DueKind::InterestAccrual),
        (SPRING_FESTIVAL_CLOSED[1], DueKind::ContractMaturity),
        (SPRING_FESTIVAL_CLOSED[1], DueKind::InterestAccrual),
    ] {
        registered.push(
            session
                .civil_clock_mut()
                .register_due(date(due_date), kind)
                .expect("scenario due registration must be valid"),
        );
    }
    (session, registered)
}

/// 跑完一个完整交易日会话（ticks_per_day 个 step）。
fn run_full_trading_session(session: &mut GameSession) {
    for _ in 0..TICKS_PER_DAY {
        session.step();
    }
}

/// 任务 26 起会话自带公司经营 dues（滚动利息等）；时钟断言只看测试自注册的到期项。
fn own(
    items: impl IntoIterator<Item = DueBusiness>,
    registered: &[DueBusiness],
) -> Vec<DueBusiness> {
    let ids: std::collections::BTreeSet<u32> =
        registered.iter().map(|due| due.id.value()).collect();
    items
        .into_iter()
        .filter(|due| ids.contains(&due.id.value()))
        .collect()
}

fn assert_fired_once_each(reports: &[CivilDayEndReport], registered: &[DueBusiness]) {
    let ids: std::collections::BTreeSet<u32> =
        registered.iter().map(|due| due.id.value()).collect();
    let mut fired: Vec<u32> = reports
        .iter()
        .flat_map(|report| report.dispatched_due.iter().map(|due| due.id.value()))
        .filter(|id| ids.contains(id))
        .collect();
    fired.sort_unstable();
    let mut expected: Vec<u32> = registered.iter().map(|due| due.id.value()).collect();
    expected.sort_unstable();
    assert_eq!(
        fired, expected,
        "every registered due item must be dispatched exactly once across the reports"
    );
}

#[test]
fn closed_days_accrue_without_trading() {
    let (mut session, registered) = spring_festival_session();
    let code = StockCode("600101".to_string());
    let player = AccountId(0);

    // 休市起点不挪日期：开局 civil 日期就是 setup 声明的周五。
    assert_eq!(session.civil_date(), date(PRE_HOLIDAY_FRIDAY));
    assert_eq!(session.civil_clock().phase(), CivilPhase::IntradayTrading);

    // 周五盘中买入。价格必须在连续竞价价格笼子内（基准价 102%/98% 与十个
    // 最小价位孰宽）；1005 恰好会在前几个 tick 与 NPC 卖方报价交叉成交。
    session
        .enqueue_player_intent(
            player,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(1005),
                qty: 100,
            },
        )
        .expect("player buy intent must enqueue");
    let mut filled = false;
    for _ in 0..TICKS_PER_DAY {
        session.step();
        if let Some(position) = session.account(player).and_then(|a| a.positions.get(&code)) {
            if position.qty > 0 {
                assert!(
                    position.t1_locked > 0,
                    "today's buy must be T+1 locked on the buy day"
                );
                assert_eq!(
                    session.account(player).unwrap().sellable_qty(&code),
                    0,
                    "shares bought today are not sellable today (T+1)"
                );
                filled = true;
                break;
            }
        }
    }
    assert!(filled, "the seeded Friday buy must fill within the session");

    // 当日卖出被 T+1 拒绝（卖出价同样保持在价格笼子内，避免先撞笼子）。
    session
        .enqueue_player_intent(
            player,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Sell,
                price: Money::from_cents(1000),
                qty: 100,
            },
        )
        .expect("player sell intent must enqueue");
    let mut rejected_same_day = false;
    for _ in 0..TICKS_PER_DAY {
        let events = session.step();
        if events.iter().any(|event| {
            matches!(
                event,
                Event::IntentRejected {
                    account,
                    reason: RejectionReason::InsufficientShares,
                    ..
                } if *account == player
            )
        }) {
            rejected_same_day = true;
            break;
        }
    }
    assert!(
        rejected_same_day,
        "same-day sell of today's buy must be rejected as InsufficientShares"
    );

    // 跑完周五会话（日界解锁 T+1）。
    while session.tick() < TICKS_PER_DAY {
        session.step();
    }
    assert_eq!(session.day(), 1, "Friday session completed exactly once");
    let bought_qty = session.account(player).unwrap().sellable_qty(&code);
    assert!(bought_qty > 0, "day boundary unlocks the Friday buy");

    // 周五日结：经营终局 + 18:00 披露 + 前进到休市日。
    let before_weekend = session.save();
    let friday_report = session
        .end_civil_day()
        .expect("Friday day-end must succeed");
    assert_eq!(friday_report.settled_date, date(PRE_HOLIDAY_FRIDAY));
    assert_eq!(
        own(friday_report.dispatched_due.iter().cloned(), &registered).len(),
        1
    );
    assert_eq!(
        friday_report.disclosure_instant.date(),
        date(PRE_HOLIDAY_FRIDAY)
    );
    assert_eq!(
        friday_report.disclosure_instant.second_of_day(),
        18 * 3600,
        "disclosure phase runs at 18:00 of the settled civil day"
    );
    assert_eq!(
        friday_report.next_status,
        DayStatus::Closed(ClosedReason::Weekend),
        "2030-02-02 is CNY eve and a Saturday; weekend reason takes priority over holiday kind"
    );
    assert_eq!(friday_report.next_date, date(SPRING_FESTIVAL_CLOSED[0]));
    assert_eq!(session.civil_clock().phase(), CivilPhase::ClosedDay);

    // 四个休市自然日逐日推进：无 tick、无成交、无注意力/主 RNG 消费，当日
    // 到期业务恰好一次，按日期顺序派发。
    let mut closed_day_reports = Vec::new();
    for iso in SPRING_FESTIVAL_CLOSED {
        assert_eq!(session.civil_date(), date(iso));
        let report = session
            .end_civil_day()
            .unwrap_or_else(|e| panic!("closed day {iso} must advance: {e}"));
        assert_eq!(report.settled_date, date(iso));
        closed_day_reports.push(report);
        // 市场时间整体冻结在周五收盘。
        assert_eq!(
            session.tick(),
            TICKS_PER_DAY,
            "no ticks on closed civil days"
        );
        assert_eq!(session.day(), 1, "no market day boundary on closed days");
    }
    assert_eq!(
        session.civil_date(),
        date(POST_HOLIDAY_WEDNESDAY),
        "after the closed weekend the clock is at the next trading day"
    );
    assert_eq!(session.civil_clock().phase(), CivilPhase::IntradayTrading);

    // 节内工作日以模拟假日标注休市（周末优先标注 Weekend，两者都不交易）。
    assert_eq!(
        closed_day_reports[0].next_status,
        DayStatus::Closed(ClosedReason::Weekend),
        "2030-02-03 is a Sunday inside the closure"
    );
    assert_eq!(
        closed_day_reports[1].next_status,
        DayStatus::Closed(ClosedReason::SimulatedHoliday(HolidayKind::SpringFestival)),
        "2030-02-04 (正月初二, Monday) is a weekday inside the Spring Festival closure"
    );
    assert_eq!(
        closed_day_reports[2].next_status,
        DayStatus::Closed(ClosedReason::SimulatedHoliday(HolidayKind::SpringFestival)),
        "2030-02-05 (正月初三, Tuesday) stays closed as a weekday holiday"
    );
    assert_eq!(
        closed_day_reports[3].next_status,
        DayStatus::Trading,
        "2030-02-06 (Wednesday) reopens"
    );

    // 派发恰好一次：02-02 利息；02-03 利息+到期；02-04 利息；02-05 利息
    //（公司经营 dues 同日并行，时钟断言只核对测试自注册项）。
    let expected_daily: [(&str, Vec<DueKind>); 4] = [
        (SPRING_FESTIVAL_CLOSED[0], vec![DueKind::InterestAccrual]),
        (
            SPRING_FESTIVAL_CLOSED[1],
            vec![DueKind::ContractMaturity, DueKind::InterestAccrual],
        ),
        (SPRING_FESTIVAL_CLOSED[2], vec![DueKind::InterestAccrual]),
        (SPRING_FESTIVAL_CLOSED[3], vec![DueKind::InterestAccrual]),
    ];
    for (report, (iso, mut kinds)) in closed_day_reports.iter().zip(expected_daily) {
        let mut fired_kinds: Vec<DueKind> = own(report.dispatched_due.iter().cloned(), &registered)
            .iter()
            .map(|due| due.kind)
            .collect();
        kinds.sort_by_key(|kind| format!("{kind:?}"));
        fired_kinds.sort_by_key(|kind| format!("{kind:?}"));
        assert_eq!(
            fired_kinds, kinds,
            "closed day {iso} dispatches exactly its own dues"
        );
        for due in own(report.dispatched_due.iter().cloned(), &registered) {
            assert_eq!(due.due_date, date(iso));
        }
    }

    // RNG/市场状态字节级不变：主 RNG、每 NPC 注意力流、tick/日计数、分钟收盘。
    let after_weekend = session.save();
    assert_eq!(
        before_weekend.rng_state, after_weekend.rng_state,
        "closed days must not consume the session RNG"
    );
    assert_eq!(
        before_weekend.npc_attention, after_weekend.npc_attention,
        "closed days must not consume any NPC attention RNG"
    );
    assert_eq!(
        before_weekend.market_minute_closes, after_weekend.market_minute_closes,
        "market minutes only advance on trading days"
    );
    assert_eq!(
        before_weekend.price_history, after_weekend.price_history,
        "closed days produce no price ticks"
    );
    assert_eq!(after_weekend.snapshot.tick, TICKS_PER_DAY);
    assert_eq!(after_weekend.snapshot.day, 1);

    // 跨休市边界 T+1 正确：周五买入在下一个交易时点可卖。
    assert_eq!(
        session.account(player).unwrap().sellable_qty(&code),
        bought_qty,
        "the weekend must not disturb T+1 availability"
    );
    run_full_trading_session(&mut session);
    assert_eq!(session.day(), 2);
    let wednesday = session
        .end_civil_day()
        .expect("Wednesday day-end must succeed");
    assert_eq!(wednesday.settled_date, date(POST_HOLIDAY_WEDNESDAY));
    assert_eq!(wednesday.next_status, DayStatus::Trading);

    // 全窗口恰好一次（周五 1 + 休市 5 + 周三 1 = 7 条注册全部各一次）。
    let mut all_reports = vec![friday_report];
    all_reports.extend(closed_day_reports);
    all_reports.push(wednesday);
    assert_fired_once_each(&all_reports, &registered);
}

#[test]
fn restored_period_boundary_is_exactly_once() {
    let (mut session, registered) = spring_festival_session();

    // 周五会话完成后、日结前先存档 A。
    run_full_trading_session(&mut session);
    let save_before_day_end = session.save();

    let friday_report = session
        .end_civil_day()
        .expect("Friday day-end must succeed");
    assert_eq!(
        own(friday_report.dispatched_due.iter().cloned(), &registered).len(),
        1
    );
    let save_after_friday = session.save();
    assert_eq!(
        save_after_friday.civil_clock.settled_through,
        Some(date(PRE_HOLIDAY_FRIDAY))
    );
    assert_eq!(
        save_after_friday.civil_clock.current_date,
        date(SPRING_FESTIVAL_CLOSED[0])
    );

    // 原始会话推进整个休市窗口到周三开盘前。
    for _ in 0..4 {
        session
            .end_civil_day()
            .expect("closed days advance one by one");
    }
    assert_eq!(session.civil_date(), date(POST_HOLIDAY_WEDNESDAY));

    // 从"周五已日结"存档恢复：不得重复结算周五，剩余 dues 各恰好一次。
    let mut restored =
        GameSession::restore(&save_after_friday).expect("mid-weekend save must restore");
    assert_eq!(restored.civil_date(), date(SPRING_FESTIVAL_CLOSED[0]));
    assert_eq!(restored.day(), 1);

    let duplicate = restored
        .civil_clock_mut()
        .end_day(date(PRE_HOLIDAY_FRIDAY))
        .expect_err("re-settling the already-settled Friday must fail");
    assert!(
        matches!(duplicate, CivilClockError::DuplicateDayEnd { .. }),
        "{duplicate:?}"
    );
    let after_rejected_duplicate = restored.save();
    assert_eq!(
        serde_json::to_vec(&save_after_friday.civil_clock).unwrap(),
        serde_json::to_vec(&after_rejected_duplicate.civil_clock).unwrap(),
        "a rejected duplicate day-end must leave the clock untouched"
    );

    let remaining: Vec<DueBusiness> = registered
        .iter()
        .filter(|due| due.due_date != date(PRE_HOLIDAY_FRIDAY))
        .cloned()
        .collect();
    let mut restored_reports = Vec::new();
    for _ in 0..4 {
        restored_reports.push(restored.end_civil_day().expect("restored weekend advances"));
    }
    run_full_trading_session(&mut restored);
    restored_reports.push(
        restored
            .end_civil_day()
            .expect("restored Wednesday day-end"),
    );
    assert_fired_once_each(&restored_reports, &remaining);
    let friday_own: Vec<DueBusiness> =
        own(friday_report.dispatched_due.iter().cloned(), &registered);
    assert!(
        !restored_reports.iter().any(|report| report
            .dispatched_due
            .iter()
            .any(|due| due.id == friday_own[0].id)),
        "the Friday due must not re-fire after restore"
    );

    // 从"周五会话完成、尚未日结"存档恢复：周五恰好结算一次。
    let mut restored_pre =
        GameSession::restore(&save_before_day_end).expect("pre-day-end save must restore");
    assert_eq!(restored_pre.civil_date(), date(PRE_HOLIDAY_FRIDAY));
    assert_eq!(restored_pre.civil_clock().settled_through(), None);
    let mut pre_reports = Vec::new();
    for _ in 0..5 {
        pre_reports.push(
            restored_pre
                .end_civil_day()
                .expect("restored replay advances"),
        );
    }
    run_full_trading_session(&mut restored_pre);
    pre_reports.push(
        restored_pre
            .end_civil_day()
            .expect("restored replay Wednesday"),
    );
    assert_fired_once_each(&pre_reports, &registered);
}

#[test]
fn closed_start_date_is_preserved_not_shifted() {
    // 2030-01-01 是元旦（模拟假日）。开局即休市：日期保持原日，先处理当日
    // 业务再前进，而不是静默挪到 01-02 开市日。
    let mut session = GameSession::new(civil_setup("2030-01-01"), 7)
        .expect("a holiday start date is a legal runtime start");
    assert_eq!(session.civil_date(), date("2030-01-01"));
    assert_eq!(session.civil_clock().phase(), CivilPhase::ClosedDay);
    assert_eq!(session.day(), 0, "no market session ran");
    let report = session
        .end_civil_day()
        .expect("closed start day still ends");
    assert_eq!(report.settled_date, date("2030-01-01"));
    assert!(
        report
            .dispatched_due
            .iter()
            .all(|due| due.due_date == date("2030-01-01")),
        "开局休市日只派发当日到期的经营 dues（本测试未注册任何自有点期项）"
    );
    assert_eq!(report.next_date, date("2030-01-02"));
    assert_eq!(session.civil_date(), date("2030-01-02"));
    assert_eq!(session.tick(), 0);
}

#[test]
fn setup_start_date_defaults_and_range_gate() {
    // serde 缺省 = 政策默认开局 2030-01-01（K1）。
    let setup = civil_setup("2030-06-03");
    let mut value = serde_json::to_value(&setup).unwrap();
    let removed = value
        .as_object_mut()
        .unwrap()
        .remove("start_date")
        .expect("setup serializes start_date");
    assert_eq!(removed.as_str(), Some("2030-06-03"));
    let defaulted: SessionSetup =
        serde_json::from_value(value).expect("start_date has a documented serde default");
    assert_eq!(defaulted.start_date, date("2030-01-01"));

    // 1998–1999 是初始化专用段，不是合法运行开局；2100 超出运行上界。
    for (bad, reason) in [
        ("1999-12-31", "init-only prehistory"),
        ("2100-01-01", "beyond ceiling"),
    ] {
        let setup = civil_setup(bad);
        let error = setup.validate().expect_err(bad);
        assert!(
            matches!(error, SessionError::Calendar(_)),
            "{reason} must be a typed calendar rejection, got {error:?}"
        );
    }
}

#[test]
fn disclosure_phase_runs_at_18_for_every_civil_day() {
    static OBSERVED: Mutex<Vec<String>> = Mutex::new(Vec::new());
    fn record(instant: CivilInstant) {
        OBSERVED
            .lock()
            .expect("disclosure recorder lock")
            .push(instant.date().to_iso());
    }

    let (mut session, _registered) = spring_festival_session();
    session.civil_clock_mut().add_disclosure_observer(record);
    run_full_trading_session(&mut session);
    session.end_civil_day().expect("Friday day-end");
    for _ in 0..4 {
        session.end_civil_day().expect("closed day advance");
    }
    let observed = OBSERVED.lock().unwrap().clone();
    assert_eq!(
        observed,
        vec![
            PRE_HOLIDAY_FRIDAY.to_string(),
            SPRING_FESTIVAL_CLOSED[0].to_string(),
            SPRING_FESTIVAL_CLOSED[1].to_string(),
            SPRING_FESTIVAL_CLOSED[2].to_string(),
            SPRING_FESTIVAL_CLOSED[3].to_string(),
        ],
        "the 18:00 disclosure hook must run exactly once per ended civil day, closed days included"
    );
}

#[test]
fn duplicate_day_end_is_rejected_atomically() {
    let (mut session, _registered) = spring_festival_session();
    run_full_trading_session(&mut session);
    session.end_civil_day().expect("first day-end succeeds");
    let before = session.save();

    let error = session
        .civil_clock_mut()
        .end_day(date(PRE_HOLIDAY_FRIDAY))
        .expect_err("settling the same civil date twice must fail");
    assert!(
        matches!(error, CivilClockError::DuplicateDayEnd {
            date: rejected_date,
            settled_through,
        } if rejected_date == date(PRE_HOLIDAY_FRIDAY)
            && settled_through == Some(date(PRE_HOLIDAY_FRIDAY))),
        "{error:?}"
    );

    let after = session.save();
    assert_eq!(
        serde_json::to_vec(&before).unwrap(),
        serde_json::to_vec(&after).unwrap(),
        "a rejected day-end must not partially change any state"
    );
}

#[test]
fn out_of_order_day_end_is_rejected() {
    let (mut session, _registered) = spring_festival_session();
    run_full_trading_session(&mut session);
    session.end_civil_day().expect("Friday settled");
    session.end_civil_day().expect("Saturday settled");
    let before = session.save();

    let error = session
        .civil_clock_mut()
        .end_day(date(PRE_HOLIDAY_FRIDAY))
        .expect_err("ending an already-passed earlier date must fail");
    assert!(
        matches!(error, CivilClockError::OutOfOrderDayEnd { date: rejected_date, current }
            if rejected_date == date(PRE_HOLIDAY_FRIDAY)
                && current == date(SPRING_FESTIVAL_CLOSED[1])),
        "{error:?}"
    );
    let after = session.save();
    assert_eq!(
        serde_json::to_vec(&before).unwrap(),
        serde_json::to_vec(&after).unwrap()
    );
}

#[test]
fn skipping_a_day_with_unprocessed_dues_is_rejected() {
    let (mut session, registered) = spring_festival_session();
    run_full_trading_session(&mut session);
    session.end_civil_day().expect("Friday settled");

    let error = session
        .civil_clock_mut()
        .end_day(date(SPRING_FESTIVAL_CLOSED[2]))
        .expect_err("jumping across closed days with pending dues must fail");
    match error {
        CivilClockError::SkippedCivilDays {
            from,
            to,
            unprocessed,
        } => {
            assert_eq!(from, date(SPRING_FESTIVAL_CLOSED[0]));
            assert_eq!(to, date(SPRING_FESTIVAL_CLOSED[2]));
            let own_unprocessed = own(unprocessed.iter().cloned(), &registered);
            assert_eq!(
                own_unprocessed.len(),
                3,
                "02-02 accrual + 02-03 maturity + 02-03 accrual must all be listed: {own_unprocessed:?}"
            );
        }
        other => panic!("expected SkippedCivilDays, got {other:?}"),
    }
    // 被拒绝的跳日不得吞掉或提前派发任何到期业务。
    let pending: Vec<CivilDate> = own(
        session.civil_clock().pending_due().iter().cloned(),
        &registered,
    )
    .iter()
    .map(|due| due.due_date)
    .collect();
    assert_eq!(
        pending,
        vec![
            date(SPRING_FESTIVAL_CLOSED[0]),
            date(SPRING_FESTIVAL_CLOSED[1]),
            date(SPRING_FESTIVAL_CLOSED[1]),
            date(SPRING_FESTIVAL_CLOSED[2]),
            date(SPRING_FESTIVAL_CLOSED[3]),
            date(POST_HOLIDAY_WEDNESDAY),
        ],
        "pending dues stay intact after the rejected skip"
    );
}

#[test]
fn advancing_beyond_2099_is_rejected_without_state_change() {
    // 2099-12-31 是周四（交易日）。跑完当日会话后日结需要前进到 2100-01-01，
    // 超出运行上界：先验证、原子拒绝，到期业务不被派发，账户零变更。
    let mut setup = civil_setup("2099-12-31");
    setup.stocks[0].float_shares = 0;
    let mut session = GameSession::new(setup, 42).expect("2099-12-31 is a legal runtime start");
    assert_eq!(session.civil_clock().phase(), CivilPhase::IntradayTrading);
    let final_due = session
        .civil_clock_mut()
        .register_due(date("2099-12-31"), DueKind::InterestAccrual)
        .expect("registering a due on the final day is legal");
    run_full_trading_session(&mut session);
    let before = session.save();

    let error = session
        .end_civil_day()
        .expect_err("advancing beyond 2099-12-31 must fail");
    assert!(
        matches!(error, SessionError::CivilClock(CivilClockError::BeyondRuntimeCeiling {
            next,
            ceiling,
        }) if next == date("2100-01-01") && ceiling == date("2099-12-31")),
        "{error:?}"
    );

    let after = session.save();
    assert_eq!(
        serde_json::to_vec(&before).unwrap(),
        serde_json::to_vec(&after).unwrap(),
        "the rejected final advance must leave every field (accounts, RNG, dues) untouched"
    );
    assert_eq!(session.civil_date(), date("2099-12-31"));
    assert_eq!(session.civil_clock().settled_through(), None);
    assert_eq!(
        own(
            session.civil_clock().pending_due().iter().cloned(),
            &[final_due]
        )
        .len(),
        1
    );
}

#[test]
fn ending_a_civil_day_requires_the_market_session_to_be_complete() {
    let (mut session, _registered) = spring_festival_session();
    for _ in 0..4 {
        session.step();
    }
    let before = session.save();
    let error = session
        .end_civil_day()
        .expect_err("ending a trading civil day before its session completed must fail");
    assert!(
        matches!(error, SessionError::CivilClock(CivilClockError::MarketSessionOutOfSync {
            date: rejected_date,
            completed_sessions: 0,
            expected_sessions: 1,
        }) if rejected_date == date(PRE_HOLIDAY_FRIDAY)),
        "{error:?}"
    );
    let after = session.save();
    assert_eq!(
        serde_json::to_vec(&before).unwrap(),
        serde_json::to_vec(&after).unwrap()
    );
    // 补完会话后同一调用必须成功（守卫不堵死成功路径）。
    while session.tick() < TICKS_PER_DAY {
        session.step();
    }
    assert!(session.end_civil_day().is_ok());
}
