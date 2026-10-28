//! 原始 Drain 报告的双腿、余单及可逆失败边界，不制造成交或来源。
use super::*;
use crate::runtime::{group::{GroupScript, PlaceSpec, TickPlan}, world::World, retail_lifecycle::BatchContext};

fn closing_world() -> World {
    let script = Box::new(|_| Box::new(|tick: u64, _round: u32| {
        let mut plan = TickPlan::default();
        if tick == 4 {
            plan.places.push(PlaceSpec { local: 0, stock: 0, side: Side::Sell, price: 1_000, qty: 100 });
            plan.places.push(PlaceSpec { local: 1, stock: 0, side: Side::Buy, price: 1_000, qty: 300 });
        }
        plan
    }) as GroupScript);
    let mut spec = crate::runtime::skeleton_tests::basic_spec(1, 1, 2, script);
    spec.ticks_per_day = 4;
    spec.closing_auction_ticks = 1;
    let mut world = World::spawn(spec, crate::calendar::TradingCalendar::current_default_calendar().unwrap()).unwrap();
    world.run_ticks(4).unwrap();
    world
}

#[test]
fn d3_real_drain_validator_abort_retries_once_and_checks_day_end_report_identity() {
    let mut world = closing_world();
    world.set_day_end_validator(Box::new(|_| Err("D3 真实 Drain validator 拒绝".to_owned())));
    let error = world.run_day_end_public().unwrap_err();
    assert!(error.to_string().contains("D3 真实 Drain validator 拒绝"));
    assert!(world.drained_causal.is_empty());
    assert!(world.drained_causal_time.is_none());
    assert!(world.drained_causal_clocks.is_empty());
    world.set_day_end_validator(Box::new(|_| Ok(())));
    world.run_day_end_public().unwrap();
    let raw = &world.drained_causal;
    assert_eq!(raw.iter().filter(|fact| matches!(fact.kind, DeskCausalKind::Execution { .. })).count(), 1);
    assert_eq!(raw.iter().filter(|fact| matches!(fact.kind, DeskCausalKind::DayEnded { qty: 200, .. })).count(), 1);
    assert!(raw.iter().all(|fact| matches!(fact.context, BatchContext::Drain { final_tick: 4, stock: 0, .. })));
    assert!(world.drained_causal_time.is_some());
    validate_executions(0, raw, &world.drained_day_trades, true).unwrap();
    validate_day_ends(0, raw, &world.drained_lifecycle).unwrap();
    let index = raw.iter().position(|fact| matches!(fact.kind, DeskCausalKind::DayEnded { .. })).unwrap();
    for mutation in 0..6 {
        let mut bad = raw.clone();
        match mutation {
            0 => { bad.remove(index); },
            1 => bad.push(bad[index]),
            _ => if let DeskCausalKind::DayEnded { order, owner, qty } = &mut bad[index].kind {
                match mutation { 2 => *qty += 100, 3 => owner.1 += 1, 4 => order.1 += 1, _ => order.0 += 1 }
            },
        }
        assert!(validate_day_ends(0, &bad, &world.drained_lifecycle).is_err(), "原股必须拒绝余单终止变体{mutation}");
    }
    world.abort_day_end();
    assert!(world.drained_causal.is_empty());
    assert!(world.drained_causal_time.is_none());
    assert!(world.drained_causal_clocks.is_empty());
    world.run_day_end_public().unwrap();
    assert_eq!(world.drained_causal.iter().filter(|fact| matches!(fact.kind, DeskCausalKind::DayEnded { .. })).count(), 1);
    world.shutdown().unwrap();
}

#[test]
fn d3_real_drain_missing_execution_is_rejected_in_original_report_and_retry_has_no_residue() {
    let mut world = closing_world();
    let hook = super::report_fault_tests::HookArm::new_drain(0);
    let error = world.run_day_end_public().unwrap_err();
    assert!(error.to_string().contains("causal Execution 缺失真实 Trade"), "{error}");
    let (removed, trades) = hook.drain_evidence();
    assert!(matches!(removed.context, BatchContext::Drain { final_tick: 4, stock: 0, .. }));
    assert!(matches!(removed.kind, DeskCausalKind::Execution { ordinal: 0, qty: 100, side: None, .. }));
    assert_eq!(trades, 1);
    assert!(world.drained_causal.is_empty());
    assert!(world.drained_causal_time.is_none());
    assert!(world.drained_causal_clocks.is_empty());
    drop(hook);
    world.run_day_end_public().unwrap();
    assert_eq!(world.drained_day_trades.len(), 1);
    assert_eq!(world.drained_causal.iter().filter(|fact| matches!(fact.kind, DeskCausalKind::Execution { .. })).count(), 1);
    world.shutdown().unwrap();
}

#[test]
fn d3_closed_manual_without_tickarc_keeps_empty_drain_without_constructing_time() {
    let mut spec = crate::runtime::skeleton_tests::basic_spec(1, 1, 1, Box::new(|_| Box::new(|_, _| TickPlan::default())));
    spec.ticks_per_day = 2;
    spec.closing_auction_ticks = 0;
    spec.calendar_start_date = crate::CivilDate::from_ymd(2026, 1, 9).unwrap();
    spec.civil_start_date = crate::CivilDate::from_ymd(2026, 1, 10).unwrap();
    spec.initial_committed_tick = 2;
    let mut world = World::spawn(spec, crate::calendar::TradingCalendar::current_default_calendar().unwrap()).unwrap();
    assert!(world.committed_calendar_tick().unwrap().is_none(), "新 authority 不补造历史 TickArc");
    world.run_day_end_public().expect("合法 Closed 日期沿用原 Manual DayArc 合同");
    assert!(world.drained_causal.is_empty());
    assert!(world.drained_causal_time.is_none(), "空 Drain 不得为取时而要求市场拍");
    assert!(world.committed_calendar_tick().unwrap().is_none());
    assert_eq!(world.global.committed_tick, 2);
    world.shutdown().unwrap();
}
