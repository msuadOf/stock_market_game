//! 串行 producer 的原 TickArc、稳定点与 DayEnd after 边界。

use super::*;
use crate::diagnostics::causal::{CausalFactKind, Termination};
use std::sync::Arc;

fn authority() -> GameSession {
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
    setup.npcs.inst_count = 0;
    setup.npcs.retail_count = 0;
    setup.ticks_per_day = 4;
    setup.auction_ticks = 0;
    setup.closing_auction_ticks = 0;
    setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
    GameSession::new(setup, 7).unwrap()
}

#[test]
fn d3_serial_keeps_original_arc_for_all_producers_and_clears_it_before_commit() {
    let mut authority = authority();
    authority
        .enqueue_player_intent(
            crate::AccountId(0),
            crate::Intent::PlaceLimit {
                code: crate::StockId(0),
                side: crate::Side::Buy,
                price: crate::LimitPrice::Fixed(crate::Money::from_cents(995)),
                qty: 100,
            },
        )
        .unwrap();
    let mut original_order = None;
    for tick in 1..=4 {
        assert!(authority.state.causal.active_tick_context.is_none());
        let count = authority.causal_facts().len();
        let mut plan = plan_tick(PhaseInput {
            session: &authority,
        })
        .unwrap();
        let context = plan
            .execute(|candidate| {
                let context = Arc::clone(candidate.causal_tick_context()?);
                let checkpoint = candidate.clone_for_tick_shadow()?;
                assert!(Arc::ptr_eq(&context, checkpoint.causal_tick_context()?));
                assert!(
                    TickShadowPlan::capture_candidate(candidate).is_err(),
                    "活跃 capture 必须先拒绝重入"
                );
                assert!(serde_json::to_value(&candidate.state.causal)
                    .unwrap()
                    .get("active_tick_context")
                    .is_none());
                Ok(context)
            })
            .unwrap();
        assert_eq!(context.after_input().tick, tick);
        assert!(Arc::ptr_eq(
            context.day().source(),
            authority.market_calendar_source()
        ));
        let output =
            market_tick_transaction::apply_tick_shadow_market_transaction(&mut plan).unwrap();
        if tick == 1 {
            original_order = output
                .event_values()
                .into_iter()
                .find_map(|event| match event {
                    crate::Event::OrderAccepted {
                        id,
                        remaining_qty: 100,
                        ..
                    } => Some(id),
                    _ => None,
                });
            assert!(original_order.is_some(), "首拍必须真实受理委托");
        }
        plan.execute(|candidate| {
            assert!(Arc::ptr_eq(&context, candidate.causal_tick_context()?));
            for fact in &candidate.causal_facts()[count..] {
                let clock = fact.time.stock_clock.expect("真实证券事实必须有本股时钟");
                assert_eq!(clock.code, crate::StockId(0));
                let projection = if tick == 4 {
                    context.after_projection()
                } else {
                    context.before_projection()
                };
                assert_eq!(
                    clock.completed_minutes,
                    projection
                        .stock(clock.code)
                        .unwrap()
                        .completed_minute_count()
                );
                assert_eq!(
                    fact.time.market_minute,
                    projection.completed_shared_minute_count()
                );
                assert_eq!(fact.time.phase, context.phase_before());
                assert_eq!(fact.time.civil, context.observation_instant_after());
            }
            Ok(())
        })
        .unwrap();
        candidate_commit::prepare_tick_shadow_plan_commit_with_evidence(
            &mut authority,
            plan,
            false,
        )
        .unwrap()
        .commit();
        assert!(
            authority.state.causal.active_tick_context.is_none(),
            "冻结提交必须已结束暂存 carrier"
        );
    }
    assert!(
        authority.prepare_market_calendar_tick().is_err(),
        "Completed 静点不得补造市场拍"
    );
    let (_, completed) = authority.prepare_market_calendar_projection().unwrap();
    assert_eq!(
        authority.causal_static_time().unwrap().market_minute,
        completed.completed_shared_minute_count()
    );
    assert_eq!(authority.causal_static_time().unwrap().stock_clock, None);
    let order = original_order.unwrap();
    assert!(authority.causal_facts().iter().any(|fact| matches!(fact.kind,
        CausalFactKind::Terminated { order: actual, qty: 100, reason: Termination::DayEnd, .. } if actual == order)));
    let report = authority.causal_diagnostics().unwrap();
    assert_eq!(report.orders[0].lifetime_market_minutes, Some(240));
    assert_eq!(
        (report.submitted_qty, report.canceled_qty, report.open_qty),
        (100, 100, 0)
    );
}

#[test]
fn d3_serial_missing_or_foreign_context_is_rejected_before_publishing() {
    let mut first = authority();
    let other = authority();
    let kind = CausalFactKind::Quote(first.causal_quote(&crate::StockId(0)));
    assert!(first.causal_record(kind.clone()).is_err());
    assert!(first.causal_facts().is_empty());
    let (_, foreign) = other.prepare_market_calendar_tick().unwrap();
    first.state.causal.active_tick_context = Some(foreign);
    assert!(
        first.causal_record(kind).is_err(),
        "同值异 owner 原 Arc 必须拒绝"
    );
    assert!(first.causal_facts().is_empty());
    let (day, valid) = first.prepare_market_calendar_tick().unwrap();
    let different_layout = day
        .tick(
            crate::session::exchange_calendar_model::SessionTickLayout {
                ticks_per_day: 8,
                auction_ticks: 0,
                closing_auction_ticks: 0,
            },
            valid.before_input(),
        )
        .unwrap();
    assert!(Arc::ptr_eq(
        different_layout.day().source(),
        first.market_calendar_source()
    ));
    assert_eq!(different_layout.before_input().tick, first.tick());
    assert_ne!(
        different_layout.observation_instant_after(),
        valid.observation_instant_after()
    );
    first.state.causal.active_tick_context = Some(different_layout);
    for kind in [
        CausalFactKind::Quote(first.causal_quote(&crate::StockId(0))),
        CausalFactKind::Decision {
            account: crate::AccountId(0),
        },
    ] {
        assert!(
            first.causal_record(kind).is_err(),
            "同 owner/day/tick 的错 layout 也必须拒绝"
        );
        assert!(first.causal_facts().is_empty());
    }
}

#[test]
fn d3_closed_static_and_restore_use_day_projection_without_security_facts() {
    let mut setup = authority().export_setup_for_runtime();
    setup.start_date = crate::CivilDate::from_iso("2030-01-05").unwrap();
    let mut game = GameSession::new(setup, 7).unwrap();
    let (day, projection) = game.prepare_market_calendar_projection().unwrap();
    assert!(!day.day_model().shared_trading_on_date());
    assert!(game.prepare_market_calendar_tick().is_err());
    let time = game.causal_time().unwrap();
    assert_eq!(time.stock_clock, None);
    assert_eq!(
        time.market_minute,
        projection.completed_shared_minute_count()
    );
    assert!(game.state.causal.active_tick_context.is_none());
    assert!(game
        .causal_record(CausalFactKind::Quote(game.causal_quote(&crate::StockId(0))))
        .is_err());
    assert!(game.causal_facts().is_empty());
    let restored = GameSession::restore(&game.save().unwrap()).unwrap();
    assert!(restored.state.causal.active_tick_context.is_none());
    assert_eq!(restored.causal_facts().len(), 1);
    assert!(matches!(
        restored.causal_facts()[0].kind,
        CausalFactKind::ObservationRestart
    ));
    assert_eq!(restored.causal_facts()[0].time.stock_clock, None);
    assert_eq!(
        restored.causal_facts()[0].time.market_minute,
        projection.completed_shared_minute_count()
    );
    assert!(matches!(
        restored.causal_diagnostics(),
        Err(crate::diagnostics::causal::CausalError::RestoredObservation)
    ));
}
