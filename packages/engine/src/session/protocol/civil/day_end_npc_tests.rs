use super::ProtocolSession;
use crate::*;

fn active_npc_setup() -> SessionSetup {
    let mut setup = crate::session::protocol::civil::publication_tests::setup();
    setup.stocks[0].float_shares = 10_000;
    setup.npcs.retail_count = 3;
    setup.npcs.inst_count = 0;
    setup.npcs.retail_cash_median = Money::from_cents(100_000_000);
    setup.strategy_params.retail.arrival_rate = 1.0;
    setup.strategy_params.retail.order_size_mean = 200;
    setup.strategy_params.retail.chase_prob = 0.3;
    setup.ticks_per_day = 4;
    setup.start_date = CivilDate::from_iso("2030-01-07").unwrap();
    setup
}

#[test]
fn active_npc_two_day_archive_restores_and_continues_without_pending_inputs() {
    let mut session = ProtocolSession::new(active_npc_setup(), 2).unwrap();
    let mut npc_accepted = 0;
    for day in 1..=2 {
        for tick_in_day in 0..4 {
            let attention_before: Vec<_> = session
                .game()
                .state
                .npc_attention
                .iter()
                .map(|(account, state)| {
                    (
                        *account,
                        state.next_attention_candidate_tick,
                        state.rng_state,
                    )
                })
                .collect();
            let receipt_cursors_before = session.game().save().unwrap().ingress_receipt_cursors;
            let frame = session.step_frame().unwrap();
            if tick_in_day == 3 {
                let attention_after: Vec<_> = session
                    .game()
                    .state
                    .npc_attention
                    .iter()
                    .map(|(account, state)| {
                        (
                            *account,
                            state.next_attention_candidate_tick,
                            state.rng_state,
                        )
                    })
                    .collect();
                assert_eq!(
                    attention_after, attention_before,
                    "日界不能消费下一交易日的 due attention 或 RNG"
                );
                assert_eq!(
                    serde_json::to_value(session.game().save().unwrap().ingress_receipt_cursors).unwrap(),
                    serde_json::to_value(receipt_cursors_before).unwrap(),
                    "日界不能为下一交易日分配 NPC 入队 receipt"
                );
            }
            npc_accepted += frame.events.iter().filter(|event| {
                matches!(event, Event::OrderAccepted { account, .. } if *account != AccountId(0))
            }).count();
        }
        session.end_civil_day_update().unwrap();
        let daily = session.save().unwrap();
        assert_eq!(daily.snapshot.tick, day * 4);
        assert!(daily.pending_npc.is_none(), "日终必须推迟下一批 NPC 观察与请求");
        ProtocolSession::restore(&daily).expect("每个已完成交易日日终候选都必须可恢复");
    }
    assert!(npc_accepted > 0, "真实活跃 NPC 必须产生已受理事实");
    let saved = session.save().unwrap();
    assert_eq!(saved.snapshot.tick, 8);
    let mut restored = ProtocolSession::restore(&saved)
        .expect("真实活跃 NPC 的公共日终候选必须可恢复，不得新生成跨日待受理输入");
    assert!(saved.pending_npc.is_none(), "日终档不保留跨日 NPC 待处理批次");
    assert_eq!(
        serde_json::to_value(restored.save().unwrap()).unwrap(),
        serde_json::to_value(&saved).unwrap()
    );
    for branch in [&mut session, &mut restored] {
        let mut resumed_npc_observations = 0;
        let mut resumed_trace = Vec::new();
        let initial = branch.game().save().unwrap();
        let baseline = initial.ingress_receipt_cursors;
        let initial_attention = initial.npc_attention;
        for expected_tick in 9..=11 {
            let frame = branch.step_frame().unwrap();
            frame.validate().unwrap();
            assert_eq!(branch.tick(), expected_tick);
            let live = branch.game().save().unwrap();
            resumed_trace.push(serde_json::json!({
                "frame": frame,
                "pending_npc": live.pending_npc,
                "attention": live.npc_attention,
                "accounts": live.snapshot.accounts,
            }));
            let pending = live.pending_npc.as_ref().unwrap();
            resumed_npc_observations += pending.observed_accounts.len();
            assert_eq!(pending.observed_tick, expected_tick);
            for receipt in &pending.intents {
                assert!(
                    receipt.account_ordinal
                        < live.ingress_receipt_cursors.next_account_ordinal[&receipt.owner]
                );
                let code = match &receipt.intent {
                    Intent::PlaceLimit { code, .. }
                    | Intent::PlaceMarket { code, .. }
                    | Intent::Cancel { code, .. } => code,
                };
                assert!(
                    receipt.stock_ordinal < live.ingress_receipt_cursors.next_stock_ordinal[code]
                );
            }
            for (account, ordinal) in &baseline.next_account_ordinal {
                assert!(live.ingress_receipt_cursors.next_account_ordinal[account] >= *ordinal);
            }
            for (code, ordinal) in &baseline.next_stock_ordinal {
                assert!(live.ingress_receipt_cursors.next_stock_ordinal[code] >= *ordinal);
            }
        }
        assert!(
            resumed_npc_observations > 0,
            "下一交易日实际观察时必须恢复真实 NPC 决策：{}",
            serde_json::to_string(&resumed_trace).unwrap()
        );
        let live = branch.game().save().unwrap();
        assert!(
            live.npc_attention.iter().any(|(account, attention)| {
                attention.rng_state != initial_attention[account].rng_state
                    && attention.next_attention_candidate_tick
                        > initial_attention[account].next_attention_candidate_tick
            }),
            "正常 T-1 NPC 决策必须继续推进真实 attention 与 RNG"
        );
    }
}
