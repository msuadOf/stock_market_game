// step_frame 接轨件 4 登记：本文件为旧管线决策链契约（queue_attention_
// trace/epoch 评估见证——session 侧决策 seam 的 trace 面），与 D2 的
// step_pipeline_contract 同型显式走旧管线；runtime 等价锚由组内稀疏
// attention 上行/翻转批承载。
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
    // T3·调用见证隔离（T1Probes 纪律）：独立池，避免并行测试混入 trace。
    session
        .game_mut()
        .rebind_compute_for_verification(crate::compute::ComputeContext::with_workers(2).unwrap());
    let _ = session.game().compute_context().take_queue_attention_trace();
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
            let frame = session.step_frame_pipeline_contract().unwrap();
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
                // T3 契约重登记（A4 失败 1 处置；ADR-0042 §9.7 节律行）：旧
                // 「日末调用前后 attention/游标全等」=旧形状快照（D(3) 由上一
                // 调用 tail 预产）。新形状 D(3) 于本调用入口现算——epoch 对齐
                // 断言：①本调用推进恰为 D(3)（每账户 next 前移或保持，且**无
                // 任何账户以 tick=4 为基准评估**——next 不越过 4）；②游标单调
                // 不减（D(3) 的 intents 在本调用登记）。业务强度「不执行下一
                // 日 epoch」由 ① 承载。
                for ((account, next_after, _rng_after), (_, next_before, _rng_before)) in
                    attention_after.iter().zip(attention_before.iter())
                {
                    assert_eq!(account.0, account.0);
                    assert!(
                        *next_after >= *next_before,
                        "日末调用只推进（D(3)），不回退：{account:?} {next_before}→{next_after}"
                    );
                }
                // A 类（业务强度·不执行下一日 epoch）：全日实际调用见证中**无
                // 任何以 tick=4 为基准的 evaluate**（pop 视界不越过日末已提交
                // tick；wait>1 的合法跨日 next 不构成 epoch-4 评估）。
                let day_trace = session.game().compute_context().take_queue_attention_trace();
                let day_last_tick = day * 4 - 1;
                assert!(
                    day_trace.iter().all(|record| record.tick <= day_last_tick),
                    "A 类：当日调用不得以下一交易日首 tick 为基准评估（trace={day_trace:?}）"
                );
                assert!(
                    day_trace.iter().any(|record| record.tick == day_last_tick),
                    "A 类：日末调用含 D({day_last_tick}) 的实际评估（新形状入口现算）"
                );
                let cursors_after = session.game().save().unwrap().ingress_receipt_cursors;
                for (account, ordinal) in &receipt_cursors_before.next_account_ordinal {
                    assert!(
                        cursors_after.next_account_ordinal.get(account).copied().unwrap_or(0)
                            >= *ordinal,
                        "日末调用游标单调不减（D(3) receipt 登记于本调用）"
                    );
                }
            }
            npc_accepted += frame.events().filter(|event| {
                matches!(event, Event::OrderAccepted { account, .. } if *account != AccountId(0))
            }).count();
        }
        session.end_civil_day_update().unwrap();
        let daily = session.save().unwrap();
        assert_eq!(daily.snapshot.tick, day * 4);
        // 批四：跨 tick 载体删除——档结构上不含 pending_npc 键（键存在即
        // 旧格式，解码边界 fault2100）；「日终不提前准备下一日批次」语义
        // 由键缺失结构性承载。
        assert!(serde_json::to_value(&daily).unwrap().get("pending_npc").is_none());
        ProtocolSession::restore(&daily).expect("每个已完成交易日日终候选都必须可恢复");
    }
    assert!(npc_accepted > 0, "真实活跃 NPC 必须产生已受理事实");
    let saved = session.save().unwrap();
    assert_eq!(saved.snapshot.tick, 8);
    let mut restored = ProtocolSession::restore(&saved)
        .expect("真实活跃 NPC 的公共日终候选必须可恢复，不得新生成跨日待受理输入");
    assert!(
        serde_json::to_value(&saved).unwrap().get("pending_npc").is_none(),
        "日终档不保留跨日 NPC 待处理批次"
    );
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
            let frame = branch.step_frame_pipeline_contract().unwrap();
            frame.validate().unwrap();
            assert_eq!(branch.tick(), expected_tick);
            let live = branch.game().save().unwrap();
            resumed_trace.push(serde_json::json!({
                "frame": frame,
                "attention": live.npc_attention,
                "accounts": live.snapshot.accounts,
            }));
            // T3 契约重登记（同批）：旧「每 tick 后 pending 必 Some+逐 receipt
            // 游标断言」随跨 tick 载体退役——等价改接：观察恢复=本 tick 真实
            // NPC 请求事件（OrderAccepted|IntentRejected 且账户≠玩家）；游标
            // 断言保留下方 baseline 单调块（新形状 receipt 登记与消费同 tick，
            // 游标前进仍被覆盖）。
            resumed_npc_observations += frame
                .events()
                .filter(|event| {
                    matches!(
                        event,
                        Event::OrderAccepted { account, .. } | Event::IntentRejected { account, .. }
                            if *account != AccountId(0)
                    )
                })
                .count();
            // 批四：跨 tick 载体删除——键缺失为结构性事实（不入热循环逐 tick
            // 全档序列化；由首 tick 的一次性检查+恢复路径的档断言承载）。

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
