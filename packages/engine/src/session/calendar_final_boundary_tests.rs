//! V12 工作草案：按当前 Full swap 合同验证 mixed Calendar 的最终失败边界。
//! 已注册；尚未编译或运行，不把注入失败等同生产通过。

use super::*;
#[path = "../runtime/calendar_public_restore_fixture.rs"]
mod fixture;
use crate::calendar::CalendarExchange;
use crate::runtime::drive::RuntimeDriver;
use crate::{StockExchange, StockId};

fn game(closed: CalendarExchange) -> (GameSession, StockId, StockId) {
    let first = if closed == CalendarExchange::Sse { StockExchange::Shanghai } else { StockExchange::Shenzhen };
    let setup = fixture::restore(closed, first, false);
    (setup.session, setup.closed, setup.open)
}

#[test]
fn mixed_full_candidate_install_rejection_keeps_old_session_and_terminal_world_version() {
    for closed_exchange in [CalendarExchange::Sse, CalendarExchange::Szse] {
        let (mut game, closed, open) = game(closed_exchange);
        // 本case仅验证零NPC/零成交的mixed Full安装拒绝；raw core不冒玩家入队路由。
        let before = serde_json::to_value(game.snapshot_inner(true, true)).unwrap();
        let before_hash = game.business_state_hash().unwrap();
        let before_save = serde_json::to_value(game.save().unwrap()).unwrap();
        let mut driver = RuntimeDriver::from_session(&mut game).unwrap();
        let old_source = driver.publication().load();
        game.inject_runtime_install_failure();
        let fatal = game.runtime_step_with_install(&mut driver).unwrap_err();
        assert!(fatal.to_string().contains("安装失败注入"));
        assert_eq!(driver.world_committed_tick(), 1, "World已提交的市场版本不能倒放");
        let published = driver.publication().load();
        assert!(published.generation > old_source.generation);
        assert_eq!(published.tick, 1);
        assert_eq!(driver.account_writes_for_test(), 0);
        assert_eq!(driver.personal_installs_for_test(), 0);
        assert_eq!(serde_json::to_value(game.snapshot_inner(true, true)).unwrap(), before);
        assert_eq!(game.business_state_hash().unwrap(), before_hash);
        assert_eq!(game.poison_reason(), Some(&fatal));
        assert!(matches!(game.save(), Err(SessionError::Step(actual)) if actual == fatal));
        assert_eq!(game.step().unwrap_err(), fatal);
        assert!(matches!(game.end_civil_day(), Err(SessionError::Step(error)) if error == fatal));
        assert_eq!(driver.world_committed_tick(), 1);
        assert_eq!(before_save["snapshot"]["tick"], serde_json::json!(0));
        assert_ne!(closed, open);
    }
}

#[test]
fn mixed_full_projection_callback_rejection_does_not_install_any_session_domain() {
    for closed_exchange in [CalendarExchange::Sse, CalendarExchange::Szse] {
        let (mut game, closed, open) = game(closed_exchange);
        let before = serde_json::to_value(game.snapshot_inner(true, true)).unwrap();
        let before_hash = game.business_state_hash().unwrap();
        let mut driver = RuntimeDriver::from_session(&mut game).unwrap();
        let fatal = StepFatal::InvariantViolation {
            location: "mixed_full_projection_callback".into(), description: "真实混合开闭市候选最后投影拒绝".into(),
        };
        let mut reached = false;
        let result: Result<(), StepFatal> = game.runtime_step_with_prepared_mirror(&mut driver, |projection, facts| {
            reached = true;
            assert_eq!(projection.tick(), 1);
            projection.project_frame(facts)?.validate().unwrap();
            Err(fatal.clone())
        });
        assert!(reached, "必须真实到达最后候选投影，不能前置错误假红");
        assert_eq!(result.unwrap_err(), fatal);
        assert_eq!(driver.world_committed_tick(), 1);
        assert_eq!(driver.account_writes_for_test(), 0);
        assert_eq!(driver.personal_installs_for_test(), 0);
        assert_eq!(serde_json::to_value(game.snapshot_inner(true, true)).unwrap(), before);
        assert_eq!(game.business_state_hash().unwrap(), before_hash);
        assert_eq!(game.poison_reason(), Some(&fatal));
        assert_eq!(driver.step_frame_equivalent(&mut game).unwrap_err(), fatal);
        assert!(matches!(game.save(), Err(SessionError::Step(actual)) if actual == fatal));
        assert_ne!(closed, open);
    }
}

#[test]
fn mixed_civil_callback_failure_keeps_installed_t1_candles_retained_and_first_error() {
    for closed_exchange in [CalendarExchange::Sse, CalendarExchange::Szse] {
        let (mut game, closed, open) = game(closed_exchange);
        // raw 安装失败 fixture：非零T+1 sentinel在driver生成前装入，不冒公共日终档。
        game.fixture_account_mut(&AccountId(0)).unwrap().fixture_set_positions([
            (closed, crate::account::Position::from_restored_parts(100, 100, 100_000, 0)),
            (open, crate::account::Position::from_restored_parts(200, 200, 200_000, 0)),
        ].into());
        let mut driver = RuntimeDriver::from_session(&mut game).unwrap();
        for _ in 0..fixture::TICKS_PER_DAY { game.runtime_step_with_install(&mut driver).unwrap(); }
        assert_eq!(game.tick(), 2);
        assert_eq!(game.account(AccountId(0)).unwrap().position(&closed).unwrap().t1_locked(), 100);
        assert_eq!(game.account(AccountId(0)).unwrap().position(&open).unwrap().t1_locked(), 0);
        let installed = serde_json::to_value(game.snapshot_inner(true, true)).unwrap();
        let installed_hash = game.business_state_hash().unwrap();
        let publication = driver.publication().load();
        let date = game.civil_date();
        let before_failure = game.snapshot_inner(true, true);
        assert_eq!(before_failure.daily_candles[&closed].len(), fixture::PRESET_HISTORY_DAYS);
        assert_eq!(before_failure.daily_candles[&open].len(), fixture::PRESET_HISTORY_DAYS + 1);
        assert_eq!(before_failure.daily_candles[&open].last().unwrap().time, super::candles::candle_date_time(date));
        assert_eq!(publication.retained.days.len(), 1, "必须已有真实非空日结retained，不能空Arc自等假阳性");
        let retained_day = &publication.retained.days[0];
        assert_eq!(retained_day.date, date);
        assert_eq!(retained_day.securities.keys().copied().collect::<Vec<_>>(), vec![0, 1]);
        assert!(!retained_day.securities[&closed.0].trading);
        assert!(retained_day.securities[&open.0].trading);
        assert!(retained_day.securities[&closed.0].bars.is_empty());
        assert!(retained_day.securities[&open.0].bars.is_empty(), "零成交open仍有明确trading状态及真实日K");
        let fatal = StepFatal::InvariantViolation {
            location: "mixed_installed_civil_callback".into(), description: "真实mixed市场Full安装后civil投影拒绝".into(),
        };
        let mut reached = false;
        let result: Result<(), SessionError> = game.with_prepared_civil_day(|candidate, report| {
            reached = true;
            assert_eq!(report.settled_date, date);
            assert_eq!(candidate.civil_date(), date.next().unwrap());
            Err(SessionError::Step(fatal.clone()))
        });
        assert!(reached);
        assert!(matches!(result, Err(SessionError::Step(ref error)) if error == &fatal));
        assert_eq!(game.civil_date(), date);
        assert_eq!(driver.world_committed_tick(), 2);
        assert_eq!(game.business_state_hash().unwrap(), installed_hash);
        assert_eq!(serde_json::to_value(game.snapshot_inner(true, true)).unwrap(), installed);
        let after = driver.publication().load();
        assert_eq!((after.tick, after.generation), (publication.tick, publication.generation));
        assert!(std::sync::Arc::ptr_eq(&after.retained, &publication.retained));
        assert_eq!(game.poison_reason(), Some(&fatal));
        assert!(matches!(game.save(), Err(SessionError::Step(actual)) if actual == fatal));
        assert_eq!(game.step().unwrap_err(), fatal);
        assert!(matches!(game.end_civil_day(), Err(SessionError::Step(error)) if error == fatal));
    }
}

#[test]
fn mixed_strict_restore_keeps_two_nonzero_plan_identities_and_distinct_stock_days() {
    for closed_exchange in [CalendarExchange::Sse, CalendarExchange::Szse] {
        let (mut game, closed, open) = game(closed_exchange);
        for _ in 0..fixture::TICKS_PER_DAY { game.step().unwrap(); }
        game.end_civil_day().unwrap();
        let days = game.stock_calendar_days().unwrap();
        assert_eq!(days[&closed], Some(0));
        assert_eq!(days[&open], Some(1));
        let mut ids = Vec::new();
        let mut editable_save = game.save().unwrap();
        // 当前Jan3两股均已开市：初始计划只使用真实本股ordinal，不造共享事件时间。
        for stock in [closed, open] {
            ids.push(editable_save.plans.create(crate::plans::PlanOpen {
                account: AccountId(0), code: stock, direction: crate::Side::Buy,
                target: crate::plans::PlanTarget::ShareCount(100),
                opinion: crate::plans::PlanOpinion { signal_score_bp: 3_000, source: crate::plans::OpinionSource::Blended },
                confidence_bp: 8_000, urgency: crate::plans::Urgency::Normal,
                horizon_trading_days: 3, created_trading_day: days[&stock].unwrap(),
            }).unwrap());
        }
        // 只编辑完整日终档事实，再经公共restore生成新owner；不绕过已经启动的driver权威。
        game = GameSession::restore(&editable_save).expect("非零计划日终档必须先严格恢复合法");
        // 个人计划作为合法初态交给真实native runtime；不把fixture create当已验collector Create。
        for _ in 0..fixture::TICKS_PER_DAY { game.step().unwrap(); }
        game.end_civil_day().unwrap();
        let save = game.save().unwrap();
        assert_eq!(save.plans.plan_ids().collect::<Vec<_>>(), ids);
        assert!(ids.iter().all(|id| save.plans.plan(*id).unwrap().execution().is_none()));
        let wire = serde_json::to_vec(&save).unwrap();
        let decoded = crate::decode_save_slot(&wire, &crate::SaveDecodeLimits::default()).unwrap();
        let mut restored = GameSession::restore(&decoded).unwrap();
        assert_eq!(serde_json::to_value(restored.save().unwrap()).unwrap(), serde_json::to_value(&save).unwrap());
        for id in &ids {
            let plan = save.plans.plan(*id).unwrap();
            assert_eq!(plan.created_trading_day(), days[plan.code()].unwrap());
            assert_eq!(plan.last_event_trading_day(), days[plan.code()].unwrap());
        }
        for _ in 0..fixture::TICKS_PER_DAY {
            assert_eq!(serde_json::to_value(restored.step().unwrap()).unwrap(), serde_json::to_value(game.step().unwrap()).unwrap());
        }
        restored.end_civil_day().unwrap();game.end_civil_day().unwrap();
        let left = restored.save().unwrap();let right = game.save().unwrap();
        assert_eq!(left.plans.plan_ids().collect::<Vec<_>>(), ids);
        assert_eq!(serde_json::to_value(left).unwrap(), serde_json::to_value(right).unwrap());
    }
}
