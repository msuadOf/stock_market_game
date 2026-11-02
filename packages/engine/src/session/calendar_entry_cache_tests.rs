//! V1 工作草案：真实入口Calendar前置、cache提交与派生域排除；已安装；未编译/运行。
//! 计数只接到真实 freeze/take/Start/Drain 发送前，默认关闭且仅 caller 线程 scope 生效。
use super::*;
use crate::calendar::CalendarExchange;
use crate::runtime::calendar_public_restore_fixture as fixture;
use crate::session::protocol::ProtocolSession;
use std::cell::Cell;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct EntryCounts {
    pub freeze: u32,
    pub take: u32,
    pub start: u32,
    pub drain: u32,
}
#[derive(Clone, Copy)]
pub(crate) enum EntryCall {
    Freeze,
    Take,
    Start,
    Drain,
}
thread_local! { static ENTRY_COUNTS: Cell<Option<EntryCounts>> = const { Cell::new(None) }; }

/// 只见证本caller线程的真实调用尝试；不新增运行时消息、共享状态或持久字段。
pub(crate) fn record_entry_call(call: EntryCall) {
    ENTRY_COUNTS.with(|slot| {
        if let Some(mut counts) = slot.get() {
            match call {
                EntryCall::Freeze => counts.freeze += 1,
                EntryCall::Take => counts.take += 1,
                EntryCall::Start => counts.start += 1,
                EntryCall::Drain => counts.drain += 1,
            }
            slot.set(Some(counts));
        }
    });
}
pub(crate) struct EntryScope {
    _caller_thread: std::marker::PhantomData<std::rc::Rc<()>>,
}
impl EntryScope {
    pub(crate) fn enter() -> Self {
        ENTRY_COUNTS.with(|slot| {
            assert!(slot.get().is_none(), "不允许嵌套或串case计数");
            slot.set(Some(EntryCounts::default()));
        });
        Self {
            _caller_thread: std::marker::PhantomData,
        }
    }
    pub(crate) fn counts(&self) -> EntryCounts {
        ENTRY_COUNTS.with(|slot| slot.get().expect("scope必须仍在位"))
    }
}
impl Drop for EntryScope {
    fn drop(&mut self) {
        ENTRY_COUNTS.with(|slot| slot.set(None));
    }
}

fn initial_save() -> SaveSlot {
    let template = fixture::initial_save(CalendarExchange::Sse, StockExchange::Shanghai, false);
    let mut setup = template.setup.clone();
    setup.ticks_per_day = 4;
    let initial = GameSession::new(setup, 73).unwrap();
    let open = initial.stock_id("000101").unwrap();
    let mut save = initial.save().unwrap();
    save.civil_clock.policy = template.civil_clock.policy;
    save.civil_clock.policy_digest = template.civil_clock.policy_digest;
    save.plans
        .create(crate::plans::PlanOpen {
            account: AccountId(0),
            code: open,
            direction: Side::Buy,
            target: crate::plans::PlanTarget::ShareCount(100),
            opinion: crate::plans::PlanOpinion {
                signal_score_bp: 3_000,
                source: crate::plans::OpinionSource::Blended,
            },
            confidence_bp: 8_000,
            urgency: crate::plans::Urgency::Normal,
            horizon_trading_days: 3,
            created_trading_day: 0,
        })
        .unwrap();
    save
}
fn buy(stock: StockId) -> Intent {
    Intent::PlaceLimit {
        code: stock,
        side: Side::Buy,
        price: LimitPrice::Fixed(Money::from_cents(1_000)),
        qty: 100,
    }
}
#[derive(Clone, Copy)]
enum Entry {
    Step,
    Frame,
}
impl Entry {
    fn run(self, session: &mut ProtocolSession) -> Result<(), StepFatal> {
        assert!(
            crate::runtime::step_runtime_enabled(),
            "本case只验证native生产runtime入口"
        );
        match self {
            Self::Step => session.game_mut().step().map(|_| ()),
            Self::Frame => session.step_frame().map(|_| ()),
        }
    }
}

fn entry_rejection(entry: Entry) {
    for warm in [false, true] {
        for foreign_owner in [false, true] {
            let save = initial_save();
            let mut session = ProtocolSession::restore_initial_checkpoint_for_test(&save).unwrap();
            if warm {
                let open = session.stock_id("000101").unwrap();
                session
                    .game_mut()
                    .enqueue_player_intent(AccountId(0), buy(open))
                    .unwrap();
                entry.run(&mut session).unwrap();
                assert!(
                    session.snapshot().accounts[&AccountId(0)]
                        .reserved_cash
                        .cents()
                        > 0,
                    "暖入口须已有真实非零买腿purse/活动订单，不能只比较空资源"
                );
            }
            let game = session.game_mut();
            let open = game.stock_id("000101").unwrap();
            let closed = game.stock_id("600101").unwrap();
            let shared = game.shared_ingress();
            let received = game
                .receive_private_intent(AccountId(0), buy(closed))
                .unwrap();
            game.state.pending_player.push(received);
            assert_eq!(
                game.state.pending_player.len(),
                1,
                "private pending必须恰有一条前置回执"
            );
            shared
                .enqueue_player_intent(AccountId(0), buy(open))
                .unwrap();
            // 只破坏本case的待校验前置来源；不伪造已发生的市场拍或静默补默认owner。
            if foreign_owner {
                let foreign = GameSession::restore(&save).unwrap();
                game.market_calendar_day = Some(foreign.prepare_market_calendar_day().unwrap());
            } else {
                game.state.day += 1;
            }
            let before = serde_json::to_value(game.snapshot_inner(true, true)).unwrap();
            let hash = game.business_state_hash().unwrap();
            let pending = serde_json::to_value(&game.state.pending_player).unwrap();
            let cursors = serde_json::to_value(&game.state.ingress_receipt_cursors).unwrap();
            assert!(
                game.state.plans.plan_ids().count() > 0,
                "合法初始PlanBook必须非空"
            );
            let plans = serde_json::to_value(&game.state.plans).unwrap();
            let receiver = serde_json::to_value(shared.receipts_probe()).unwrap();
            let cached = game.market_calendar_day.clone();
            let publication = game
                .runtime_driver
                .as_ref()
                .map(|cell| cell.lock().unwrap().publication().load());
            let world_log = game.runtime_driver.as_ref().map(|cell| {
                cell.lock()
                    .unwrap()
                    .world_report_log()
                    .iter()
                    .map(|row| {
                        (
                            row.tick,
                            row.rounds,
                            row.group_hashes.clone(),
                            row.desk_hashes.clone(),
                            row.desk_resting.clone(),
                        )
                    })
                    .collect::<Vec<_>>()
            });
            let scope = EntryScope::enter();
            let error = entry.run(&mut session).unwrap_err();
            assert_eq!(
                scope.counts(),
                EntryCounts::default(),
                "非法Calendar必须在freeze/take/Start真实尝试前拒绝"
            );
            drop(scope);
            assert!(
                matches!(error, StepFatal::InvariantViolation { ref location, ref description }
                if (foreign_owner && location == "GameSession::prepare_market_calendar_day" && description.contains("source"))
                || (!foreign_owner && location == "GameSession::prepare_market_calendar_tick" && description.contains("计数不一致"))),
                "必须命中指定Calendar源因：{error:?}"
            );
            let game = session.game_mut();
            assert_eq!(
                serde_json::to_value(game.snapshot_inner(true, true)).unwrap(),
                before
            );
            assert_eq!(game.business_state_hash().unwrap(), hash);
            assert_eq!(
                serde_json::to_value(&game.state.pending_player).unwrap(),
                pending
            );
            assert_eq!(
                serde_json::to_value(&game.state.ingress_receipt_cursors).unwrap(),
                cursors
            );
            assert_eq!(serde_json::to_value(&game.state.plans).unwrap(), plans);
            assert_eq!(
                serde_json::to_value(shared.receipts_probe()).unwrap(),
                receiver
            );
            match (&cached, &game.market_calendar_day) {
                (Some(before), Some(after)) => assert!(Arc::ptr_eq(before, after)),
                (None, None) => {}
                other => panic!("拒绝不能发布或清除cache：{other:?}"),
            }
            if warm {
                let driver = game.runtime_driver.as_ref().unwrap().lock().unwrap();
                assert!(Arc::ptr_eq(
                    publication.as_ref().unwrap(),
                    &driver.publication().load()
                ));
                assert_eq!(
                    &driver
                        .world_report_log()
                        .iter()
                        .map(|row| (
                            row.tick,
                            row.rounds,
                            row.group_hashes.clone(),
                            row.desk_hashes.clone(),
                            row.desk_resting.clone()
                        ))
                        .collect::<Vec<_>>(),
                    world_log.as_ref().unwrap()
                );
            } else {
                assert!(game.runtime_driver.is_none(), "冷入口不得偷偷构造World");
            }
            // 从失败后的真实authority fork再freeze，必须仍能取得原shared回执且不重号。
            let mut retry = game.clone_for_tick_shadow().unwrap();
            retry.freeze_shared_ingress().unwrap();
            let receipts = &retry.state.pending_player;
            assert_eq!(receipts.len(), 2);
            assert_eq!(receipts[0].account_ordinal, u64::from(warm));
            assert_eq!(receipts[1].account_ordinal, u64::from(warm) + 1);
            assert_eq!(receipts[0].owner, AccountId(0));
            assert_eq!(receipts[1].owner, AccountId(0));
            assert_eq!(receipts[0].stock_ordinal, 0);
            assert_eq!(receipts[1].stock_ordinal, u64::from(warm));
            assert_eq!(serde_json::to_value(&receipts[0]).unwrap(), pending[0]);
            assert_eq!(
                serde_json::to_value(&receipts[1].intent).unwrap(),
                serde_json::to_value(buy(open)).unwrap()
            );
            ENTRY_COUNTS.with(|slot| assert!(slot.get().is_none(), "RAII必须恢复默认关闭"));
        }
    }
}

#[test]
fn mixed_public_step_calendar_rejection_keeps_cold_and_warm_ingress_before_all_side_effects() {
    entry_rejection(Entry::Step);
}
#[test]
fn mixed_protocol_frame_calendar_rejection_keeps_cold_and_warm_ingress_before_all_side_effects() {
    entry_rejection(Entry::Frame);
}

#[test]
fn mixed_valid_entry_proves_real_freeze_take_start_observers_are_live_and_scoped() {
    for entry in [Entry::Step, Entry::Frame] {
        let mut session =
            ProtocolSession::restore_initial_checkpoint_for_test(&initial_save()).unwrap();
        let open = session.stock_id("000101").unwrap();
        session
            .game_mut()
            .enqueue_player_intent(AccountId(0), buy(open))
            .unwrap();
        session
            .shared_ingress()
            .enqueue_player_intent(AccountId(0), buy(open))
            .unwrap();
        let scope = EntryScope::enter();
        entry.run(&mut session).unwrap();
        let counts = scope.counts();
        assert_eq!(counts.freeze, 1);
        assert_eq!(counts.take, 1);
        assert_eq!(
            counts.start, 3,
            "两个真实Desk Start加一个真实Group Start尝试"
        );
        assert!(session.game().state.pending_player.is_empty());
        assert_eq!(session.tick(), 1);
        assert!(session.market_calendar_day.is_some());
        drop(scope);
        ENTRY_COUNTS.with(|slot| assert!(slot.get().is_none()));
    }
}

#[test]
fn mixed_hit_and_miss_cache_survive_real_last_frame_projection_failure() {
    for warm in [false, true] {
        let mut game = GameSession::restore(&initial_save()).unwrap();
        if warm {
            game.step().unwrap();
        }
        let old = game.market_calendar_day.clone();
        assert_eq!(old.is_some(), warm);
        let source = Arc::clone(game.market_calendar_source());
        let static_before = serde_json::to_value(game.snapshot_inner(true, true)).unwrap();
        let hash = game.business_state_hash().unwrap();
        if !warm {
            // 冷路径只构造合法runtime owner，不推进tick，保留Calendar cache miss。
            game.construct_runtime_driver_lazy_for_test().unwrap();
            let handed_over = serde_json::to_value(game.snapshot_inner(true, true)).unwrap();
            let mut expected = static_before.clone();
            let expected_origin = expected["mirror_origin"].as_object_mut().unwrap();
            for field in ["authority_epoch", "install_sequence"] {
                let value = expected_origin[field].as_u64().unwrap();
                expected_origin.insert(field.to_owned(), serde_json::json!(value + 1));
            }
            assert_eq!(handed_over, expected, "惰性接管仅推进镜像来源代数");
        }
        let before = serde_json::to_value(game.snapshot_inner(true, true)).unwrap();
        assert_eq!(game.business_state_hash().unwrap(), hash);
        assert_eq!(
            game.market_calendar_day.is_some(),
            warm,
            "构造owner须保留原cache冷热状态"
        );
        let fatal = StepFatal::InvariantViolation {
            location: "mixed_v1_final_frame".into(),
            description: "最后frame投影拒绝".into(),
        };
        let mut reached = false;
        let result: Result<((), Option<crate::session::pipeline::TickCommitEvidence>), StepFatal> =
            game.step_frame_and_commit_via_runtime(|projection, facts| {
                reached = true;
                let frame = projection.project_frame(facts)?;
                frame.validate().unwrap();
                assert_eq!(frame.tick, if warm { 2 } else { 1 });
                Err(fatal.clone())
            });
        assert!(reached);
        assert_eq!(result.unwrap_err(), fatal);
        match (&old, &game.market_calendar_day) {
            (Some(before), Some(after)) => assert!(Arc::ptr_eq(before, after)),
            (None, None) => {}
            other => panic!("最后frame拒绝不得发布新cache：{other:?}"),
        }
        assert!(Arc::ptr_eq(&source, game.market_calendar_source()));
        assert_eq!(
            serde_json::to_value(game.snapshot_inner(true, true)).unwrap(),
            before
        );
        assert_eq!(game.business_state_hash().unwrap(), hash);
        assert_eq!(game.poison_reason(), Some(&fatal));
    }
}

#[test]
fn mixed_failed_civil_projection_keeps_warm_cache_and_original_day() {
    let mut game = GameSession::restore(&initial_save()).unwrap();
    for _ in 0..4 {
        game.step().unwrap();
    }
    let old = game
        .market_calendar_day
        .clone()
        .expect("真实成功末拍必须保原日cache");
    let source = Arc::clone(game.market_calendar_source());
    let before = serde_json::to_value(game.snapshot_inner(true, true)).unwrap();
    let hash = game.business_state_hash().unwrap();
    let fatal = StepFatal::InvariantViolation {
        location: "mixed_v1_civil_cache".into(),
        description: "完整civil候选投影拒绝".into(),
    };
    let mut reached = false;
    let result: Result<(), SessionError> = game.with_prepared_civil_day(|candidate, report| {
        reached = true;
        assert_eq!(report.settled_date, fixture::date("2030-01-02"));
        assert_eq!(candidate.civil_date(), fixture::date("2030-01-03"));
        assert!(candidate.market_calendar_day.is_none());
        Err(SessionError::Step(fatal.clone()))
    });
    assert!(reached);
    assert!(matches!(result, Err(SessionError::Step(ref error)) if error == &fatal));
    assert_eq!(game.civil_date(), fixture::date("2030-01-02"));
    assert!(Arc::ptr_eq(&source, game.market_calendar_source()));
    assert!(Arc::ptr_eq(
        &old,
        game.market_calendar_day.as_ref().unwrap()
    ));
    assert_eq!(
        serde_json::to_value(game.snapshot_inner(true, true)).unwrap(),
        before
    );
    assert_eq!(game.business_state_hash().unwrap(), hash);
    assert_eq!(game.poison_reason(), Some(&fatal));
}

#[test]
fn mixed_populated_cache_is_derived_for_clone_hash_save_and_successful_civil_clear() {
    let mut game = GameSession::restore(&initial_save()).unwrap();
    let before = serde_json::to_value(game.save().unwrap()).unwrap();
    let hash = game.business_state_hash().unwrap();
    let day = game.prepare_market_calendar_day().unwrap();
    game.install_market_calendar_day(Arc::clone(&day));
    assert_eq!(game.business_state_hash().unwrap(), hash);
    assert_eq!(serde_json::to_value(game.save().unwrap()).unwrap(), before);
    let shadow = game.clone_for_tick_shadow().unwrap();
    assert!(Arc::ptr_eq(
        game.market_calendar_source(),
        shadow.market_calendar_source()
    ));
    assert!(Arc::ptr_eq(
        &day,
        shadow.market_calendar_day.as_ref().unwrap()
    ));
    game.commit_tick_shadow(shadow);
    assert!(Arc::ptr_eq(
        &day,
        game.market_calendar_day.as_ref().unwrap()
    ));
    for _ in 0..4 {
        game.step().unwrap();
    }
    let source = Arc::clone(game.market_calendar_source());
    let old = game
        .market_calendar_day
        .clone()
        .expect("真实市场成功后必须保本日cache");
    assert_eq!(old.day_model().current_date(), fixture::date("2030-01-02"));
    let report = game.end_civil_day().unwrap();
    assert_eq!(report.settled_date, fixture::date("2030-01-02"));
    assert_eq!(game.civil_date(), fixture::date("2030-01-03"));
    assert!(game.market_calendar_day.is_none());
    assert!(Arc::ptr_eq(&source, game.market_calendar_source()));
    let next = game.prepare_market_calendar_day().unwrap();
    assert!(!Arc::ptr_eq(&old, &next));
    assert_eq!(next.day_model().current_date(), game.civil_date());
    assert!(game.market_calendar_day.is_none());
}
