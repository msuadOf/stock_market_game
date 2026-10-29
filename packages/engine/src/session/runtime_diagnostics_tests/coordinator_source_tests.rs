//! 真实协调者原 report 校验须先于安装与 frame callback 拒绝来源缺失。

use super::*;
use crate::runtime::diagnostics::report_fault_tests::HookArm;

fn prepared() -> GameSession {
    let mut game = d2_game();
    d2_place(&mut game, 2, 1, crate::Side::Sell, 995, 300);
    game.step_via_runtime().unwrap();
    assert!(!game.causal_facts().is_empty());
    assert_eq!(game.runtime_derive_envelopes().unwrap().len(), 1);
    d2_place(&mut game, 0, 1, crate::Side::Buy, 995, 100);
    game.step_via_runtime().unwrap();
    assert_eq!(d2_executions(&game).len(), 1, "失败前必须已有真实成交事实");
    d2_place(&mut game, 0, 1, crate::Side::Buy, 995, 100);
    game
}

fn check_failure(frame: bool) {
    let mut game = prepared();
    let checkpoint = game.clone_for_tick_shadow().unwrap();
    let prior_facts = serde_json::to_value(checkpoint.causal_facts()).unwrap();
    let hook = HookArm::new(1);
    let mut callback = false;
    let error = if frame {
        game.step_frame_and_commit_via_runtime(|_, _| {
            callback = true;
            Ok(())
        })
        .unwrap_err()
    } else {
        game.step_via_runtime().unwrap_err()
    };
    let evidence = hook.evidence();
    drop(hook);
    assert!(!callback, "来源缺失必须早于 frame callback");
    assert!(
        matches!(&error, StepFatal::InvariantViolation { location, description }
        if location == "runtime_mirror::prepare"
            && description.contains("ProtocolInvariant")
            && description.contains("causal Execution 缺失真实 Trade")),
        "必须由真实 coordinator 原 report 校验拒绝：{error:?}"
    );
    assert_eq!((evidence.tick, evidence.round, evidence.stock), (3, 0, 1));
    assert_eq!(
        (
            evidence.original_trades,
            evidence.executions_before,
            evidence.executions_after
        ),
        (1, 1, 0)
    );
    assert_eq!(
        (
            evidence.seller.order,
            evidence.seller.owner,
            evidence.buyer.order,
            evidence.buyer.owner
        ),
        ((1, 1), (0, 2), (1, 3), (0, 0))
    );
    assert_eq!((evidence.qty, evidence.price_cents), (100, 995));
    assert_eq!(
        serde_json::to_value(game.causal_facts()).unwrap(),
        prior_facts
    );
    assert_eq!(game.tick(), checkpoint.tick());
    assert_eq!(game.current_seq(), checkpoint.current_seq());
    assert_eq!(
        game.business_state_hash().unwrap(),
        checkpoint.business_state_hash().unwrap()
    );
    let mut poisoned = checkpoint.clone_for_tick_shadow().unwrap();
    poisoned.poison_runtime_step(error.clone());
    assert_eq!(
        game.session_state_hash().unwrap(),
        poisoned.session_state_hash().unwrap()
    );
    for id in [AccountId(0), AccountId(2)] {
        let actual = game.account(id).unwrap();
        let expected = checkpoint.account(id).unwrap();
        assert_eq!(actual.cash(), expected.cash());
        assert_eq!(
            serde_json::to_value(actual.positions()).unwrap(),
            serde_json::to_value(expected.positions()).unwrap()
        );
    }
    let envelopes = |session: &GameSession| {
        session
            .runtime_derive_envelopes()
            .unwrap()
            .into_iter()
            .map(|envelope| (envelope.key().clone(), envelope.audit()))
            .collect::<Vec<_>>()
    };
    assert_eq!(envelopes(&game), envelopes(&checkpoint));
    assert_eq!(envelopes(&game).len(), 1, "必须保住原真实非空卖余单");
    let mut retry = checkpoint;
    let events = retry.step_via_runtime().unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(
                event,
                Event::Trade {
                    maker: AccountId(2),
                    taker: AccountId(0),
                    qty: 100,
                    ..
                }
            ))
            .count(),
        1
    );
    assert_eq!(d2_executions(&retry).len(), 2);
    assert_eq!(retry.causal_diagnostics().unwrap().filled_qty, 400);
    assert!(retry
        .causal_facts()
        .iter()
        .enumerate()
        .all(|(index, fact)| fact.sequence == index as u64));
}

#[test]
fn d2_ordinary_real_coordinator_rejects_missing_execution_before_install() {
    check_failure(false);
}

#[test]
fn d2_frame_real_coordinator_rejects_missing_execution_before_callback() {
    check_failure(true);
}
