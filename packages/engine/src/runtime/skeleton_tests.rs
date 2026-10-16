//! 批 2 骨架协议测试（G 批 2 验收判据）：收束计数、时序因果、Abort 回滚等价
//! （含多轮 Continue 后失败）、panic 运输（resume_unwind 原 payload）、断连显式
//! 失败、Drain→Shutdown→Final 对账、看门狗相位感知。

use super::coordinator::{no_op_validator, Validator};
use super::group::{GroupScript, PlaceSpec, TickPlan};
use super::watchdog::{recv_deadline, Phase, WatchdogConfig};
use super::world::{ShutdownReport, World, WorldSpec};
use super::GroupIdx;
use crate::orderbook::Side;
use std::sync::mpsc;
use std::time::Duration;

/// 空闲脚本：每轮空计划（无操作、无续行）。
fn idle_script() -> GroupScript {
    Box::new(|_tick, _round| TickPlan::default())
}

/// 基础世界：给定组/股数与脚本工厂，账户现金 1_000_000 分、每账户各股 1000 股。
/// 全天连续竞价形态（auction_ticks=0，测试专用简化），股 @1000 分。
pub(crate) fn basic_spec(
    n_groups: usize,
    n_stocks: usize,
    accounts_per_group: usize,
    script_for: Box<dyn Fn(GroupIdx) -> GroupScript>,
) -> WorldSpec {
    let shares = vec![vec![vec![1_000u32; n_stocks]; accounts_per_group]; n_groups];
    WorldSpec {
        initial_desk_orders: Vec::new(),
        n_groups,
        accounts_per_group,
        initial_cash: 1_000_000,
        initial_shares: shares,
        initial_invested: Vec::new(),
        initial_recovered: Vec::new(),
        initial_ever_held: Vec::new(),
        root_projection: None,
        civil_start_date: crate::calendar::CivilDate::from_ymd(2026, 1, 5).expect("测试日期合法"),
        script_for,
        chunk: 8,
        watchdog: WatchdogConfig {
            normal: Duration::from_secs(5),
            long: Duration::from_secs(10),
        },
        rounds_cap: 4,
        stocks: (0..n_stocks)
            .map(|_| super::world::DeskStockSpec::main_board(1000))
            .collect(),
        config: std::sync::Arc::new(crate::config::GameConfig::proposed_defaults()),
        ticks_per_day: 30,
        auction_ticks: 0,
        closing_auction_ticks: 3,
        order_ttl_ticks: 1_000_000,
        history_len: 16,
        initial_committed_tick: 0,
        belief_seeds: Vec::new(),
        library: None,
        decision: None,
        rights_computer: None,
        initial_cash_override: None,
        plan_factory: None,
    }
}

fn buy(local: u32, stock: u16, price: i64, qty: u32) -> PlaceSpec {
    PlaceSpec {
        local,
        stock,
        side: Side::Buy,
        price,
        qty,
    }
}

fn sell(local: u32, stock: u16, price: i64, qty: u32) -> PlaceSpec {
    PlaceSpec {
        local,
        stock,
        side: Side::Sell,
        price,
        qty,
    }
}

// ── 1. 收束计数与安装 ──

#[test]
fn convergence_counts_g_plus_n_and_installs_each_tick() {
    let mut world =
        World::spawn(basic_spec(3, 2, 2, Box::new(|_| idle_script()))).expect("世界构造");
    let committed = world.run_ticks(4).expect("4 个 tick 全部收敛");
    assert_eq!(committed, 4);
    assert_eq!(world.global.committed_tick, 4);
    assert_eq!(world.report_log.len(), 4, "每个 tick 恰一条安装记录");
    for (i, rec) in world.report_log.iter().enumerate() {
        assert_eq!(rec.tick, (i + 1) as u64);
        assert_eq!(rec.group_hashes.len(), 3, "每组一条哈希");
        assert_eq!(rec.desk_hashes.len(), 2, "每交易部一条哈希");
    }
    let report = world.shutdown().expect("关停对账");
    assert_eq!(report.group_finals.len(), 3);
    assert_eq!(report.desk_finals.len(), 2);
    // 空闲脚本：期末名册 = 初始（现金/持仓/在途全原样）。
    for f in &report.group_finals {
        for acct in &f.accounts {
            assert_eq!(acct.cash, 1_000_000);
            assert!(acct.shares.iter().all(|&s| s == 1_000));
            assert!(acct.foreign_active.iter().all(|&x| x == 0));
        }
    }
    for f in &report.desk_finals {
        assert!(f.resting.is_empty());
        assert_eq!(f.skipped_orders, 0);
    }
    for (who, r) in &report.worker_results {
        if let Err(e) = r {
            panic!("空闲世界 worker {who} 不应失败：{e:?}");
        }
    }
}

// ── 2. 资源流动与 Drain 实体退回 ──

#[test]
fn entity_flow_holds_purse_at_desk_and_drain_refunds() {
    // tick1 买 100 股 @1000（purse 100_000 扣出随单转移）；tick2 撤单回流。
    let script = |g: GroupIdx| -> GroupScript {
        Box::new(move |tick, _round| {
            let _ = g;
            let mut plan = TickPlan::default();
            if tick == 1 {
                plan.places.push(buy(0, 0, 1000, 100));
            }
            plan
        })
    };
    let mut world = World::spawn(basic_spec(1, 2, 2, Box::new(script))).expect("世界构造");
    world.run_ticks(1).expect("tick1 收敛");
    // tick1 已提交：现金被扣（挂单在途，实体在交易部）。
    // 关停 Drain：强撤挂单 → purse 原路回流 → 期末名册现金复原。
    let report = world.shutdown().expect("关停");
    let acct = &report.group_finals[0].accounts[0];
    assert_eq!(acct.cash, 1_000_000, "Drain 后 purse 全额回流");
    assert_eq!(acct.foreign_active[0], 0, "Closed 落账后在途归零");
    assert!(report.desk_finals[0].resting.is_empty(), "清簿后无挂单");
}

// ── 3. 时序因果：实体批次先于下一 Start ──

#[test]
fn entity_batches_arrive_before_next_start() {
    use super::group::Observed;
    // tick1 下单（买入挂簿）；tick2 对齐撤单（实体回流）；tick3 空闲。
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| {
            let mut plan = TickPlan::default();
            match tick {
                1 => plan.places.push(buy(0, 0, 1000, 100)),
                2 => plan
                    .cancels
                    .push(super::group::CancelSpec { local: 0, stock: 0 }),
                _ => {}
            }
            plan
        })
    };
    let mut world = World::spawn(basic_spec(1, 1, 1, Box::new(script))).expect("世界构造");
    world.run_ticks(3).expect("3 tick 收敛");
    let report = world.shutdown().expect("关停");
    let observed = &report.group_finals[0].observed;
    // 全部 tick-2 批次必须先于 Start(3)（mpsc FIFO + 收束因果序）。
    let start3 = observed
        .iter()
        .position(|o| *o == Observed::Start(3))
        .expect("应有 Start(3)");
    let batches2: Vec<usize> = observed
        .iter()
        .enumerate()
        .filter(|(_, o)| **o == Observed::Batch(2))
        .map(|(i, _)| i)
        .collect();
    assert!(!batches2.is_empty(), "tick2 撤单应有实体批次回流");
    for &pos in &batches2 {
        assert!(pos < start3, "Batch(2)@{pos} 必须先于 Start(3)@{start3}");
    }
    // tick3 决策时退款已可见：现金复原（tick3 Start 落账了 tick2 的 CashRefund）。
    let acct = &report.group_finals[0].accounts[0];
    assert_eq!(acct.cash, 1_000_000, "撤单退款在下一 tick 决策前落账");
}

// ── 4. 同 tick 多轮（Continue）──

#[test]
fn multi_round_continue_terminates_by_empty_continuation() {
    // 轮 0 有续行（下单），轮 1 收口。
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, round| {
            let mut plan = TickPlan::default();
            if tick == 1 {
                if round == 0 {
                    plan.places.push(buy(0, 0, 1000, 100));
                    plan.continuation = true;
                } else if round == 1 {
                    plan.places.push(sell(1, 0, 1000, 100));
                }
            }
            plan
        })
    };
    let mut world = World::spawn(basic_spec(1, 1, 2, Box::new(script))).expect("世界构造");
    world.run_ticks(1).expect("多轮 tick 收敛");
    assert_eq!(world.report_log[0].rounds, 1, "轮 0+轮 1，最后轮号=1");
    let report = world.shutdown().expect("关停");
    // 轮 1 卖单与轮 0 买单同价交叉 ⇒ 成交（maker 价）。
    // 买方：扣 100_000 对价 + 500 最低佣金 + 1 过户费；得 100 股（T+1 冻结）。
    let acct0 = &report.group_finals[0].accounts[0];
    assert_eq!(acct0.cash, 1_000_000 - 100_501, "买腿实扣=对价+费用");
    assert_eq!(acct0.shares[0], 1_100, "买入入账");
    assert_eq!(acct0.frozen_today[0], 100, "T+1 冻结");
    // 卖方：净得 = 100_000 − (500 佣金 + 50 印花 + 1 过户)。
    let acct1 = &report.group_finals[0].accounts[1];
    assert_eq!(acct1.cash, 1_000_000 + 99_449, "卖腿净得");
    assert_eq!(acct1.shares[0], 900, "卖出扣股");
    // 守恒：费用桶=双边费用合计。
    let desk = &report.desk_finals[0];
    assert_eq!(desk.fee_sink, 501 + 551, "费用桶");
    assert_eq!(desk.market.volume_today, 100);
    assert_eq!(desk.market.trade_count, 1);
}

#[test]
fn rounds_cap_exceeding_fails_explicitly() {
    // 每轮都声称续行：超过轮数上限必须显式失败（异常形状不静默）。
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|_tick, _round| {
            let mut plan = TickPlan::default();
            plan.continuation = true;
            plan
        })
    };
    let mut spec = basic_spec(1, 1, 1, Box::new(script));
    spec.rounds_cap = 2;
    let mut world = World::spawn(spec).expect("世界构造");
    let err = world.run_ticks(1).expect_err("轮数超上限须显式失败");
    assert!(
        matches!(
            err,
            super::protocol::RuntimeError::RoundsExceeded {
                tick: 1,
                rounds_cap: 2
            }
        ),
        "应为 RoundsExceeded，收到 {err:?}"
    );
    let _ = world.shutdown();
}

// ── 5. Abort 回滚等价（含多轮 Continue 后失败）──

#[test]
fn abort_after_multi_round_rolls_back_three_layers_to_tick_start() {
    // tick1 空闲；tick2 轮 0（买入+卖出，声明续行）→ 轮 1（再买入）→ 校验失败。
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, round| {
            let mut plan = TickPlan::default();
            if tick == 2 {
                if round == 0 {
                    plan.places.push(buy(0, 0, 1000, 100));
                    plan.places.push(sell(1, 1, 1000, 50));
                    plan.continuation = true;
                } else if round == 1 {
                    plan.places.push(buy(1, 0, 1100, 200));
                }
            }
            plan
        })
    };
    let mut world = World::spawn(basic_spec(2, 2, 2, Box::new(script))).expect("世界构造");
    // 校验注入：tick2 轮 1 失败（多轮已完成后才判死）。
    let validator: Validator = Box::new(|ev| {
        if ev.tick == 2 && ev.round == 1 {
            Err("注入：tick2 轮1 校验失败".to_owned())
        } else {
            Ok(())
        }
    });
    world.set_validator(validator);
    let err = world.run_ticks(2).expect_err("tick2 应被 Abort");
    assert!(
        matches!(
            err,
            super::protocol::RuntimeError::TickValidation {
                tick: 2,
                round: 1,
                ..
            }
        ),
        "应为 TickValidation，收到 {err:?}"
    );
    // 协调者全局域：tick2 未安装。
    assert_eq!(world.global.committed_tick, 1, "失败 tick 不安装");
    assert_eq!(world.report_log.len(), 1, "只有 tick1 的安装记录");
    // tick 起始（=tick1 安装记录）哈希基线，须在 shutdown 消费 world 前取。
    let tick1 = world.report_log[0].clone();
    // 关停取 Final 对账三方状态。
    let report = world.shutdown().expect("关停对账");
    for f in &report.group_finals {
        // 批 8b 哈希惰性化：哈希基线断言退役（报告为 O(1) 诊断指纹）；
        // 回滚完整性由下方逐字段断言承载（更强）。
        let _ = &tick1.group_hashes;
        for acct in &f.accounts {
            assert_eq!(acct.cash, 1_000_000, "Abort 冲回 purse 扣除");
            assert!(
                acct.shares.iter().all(|&s| s == 1_000),
                "Abort 冲回卖出扣股"
            );
            assert!(
                acct.foreign_active.iter().all(|&x| x == 0),
                "foreign_active 复原起始值"
            );
        }
    }
    for f in &report.desk_finals {
        // 回滚等价判据：Abort 时刻哈希 == 提交态（tick1 末的行情面合法演化已包含

        assert!(f.resting.is_empty(), "Abort 后簿恢复清空");
        // 结构断言：tick2 的成交/挂单全部回滚（tick1 空闲 ⇒ 行情面停留在初值）。
        assert_eq!(f.market.volume_today, 0, "成交量回滚");
        assert_eq!(f.market.trade_count, 0, "成交笔数回滚");
        assert_eq!(f.market.last_price, 1000, "最新价回滚");
        assert_eq!(f.fee_sink, 0, "费用桶回滚");
        let _ = &tick1.desk_hashes;
    }
}

#[test]
fn failed_tick_can_be_rerun_after_abort() {
    // F §5.2 第 4 条：Abort 后全体 worker 回到上一收敛态、继续存活——可重跑同 tick。
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| {
            let mut plan = TickPlan::default();
            if tick == 2 {
                plan.places.push(buy(0, 0, 1000, 100));
            }
            plan
        })
    };
    let mut world = World::spawn(basic_spec(1, 1, 1, Box::new(script))).expect("世界构造");
    let fail_once: Validator = Box::new(|ev| {
        if ev.tick == 2 {
            Err("注入：tick2 首次失败".to_owned())
        } else {
            Ok(())
        }
    });
    world.set_validator(fail_once);
    assert!(world.run_ticks(2).is_err(), "tick2 首次应失败");
    // 换回通过校验器，重跑同一 tick。
    world.set_validator(no_op_validator());
    let committed = world.run_ticks(1).expect("Abort 后重跑 tick2 成功");
    assert_eq!(committed, 2);
    assert_eq!(world.report_log.len(), 2);
    let report = world.shutdown().expect("关停对账");
    assert_eq!(
        report.group_finals[0].accounts[0].cash, 1_000_000,
        "重跑后 Drain 回流复原"
    );
}

// ── 5b. ForceDue 事务边界（Sol 二审修复集 4）──

// Start 间窗口收到的 ForceDue 指令效果必须属于**下一 tick 的日志域**：
// tick Abort 冲回强制前值（指令失效需重发——代码承诺语义），重跑同 tick
// 不重发则 NPC 不到期。此前指令在收到时立即入账，undo 被 Start 的
// undo.clear() 清除——Abort 后 next_due 停在强制后目标（承诺违背）。
#[test]
fn sol4_force_due_abort_rolls_back_to_pre_force_and_requires_resend() {
    // 决策档案：local 0 到期=100（远期——本测试窗口内自然调度永不触发），
    // retail 到达率 0（决策无订单副作用，纯调度面断言）。
    let mut spec = basic_spec(1, 1, 1, Box::new(|_| idle_script()));
    spec.decision = Some(super::decision::DecisionSetup {
        profiles: vec![super::decision::NpcProfile {
            strategy: crate::strategy::StrategyData::retail(0.0, 100, 0.0),
            seed: 7,
            next_due: 100,
        }],
        attention_gap_ticks: 4,
    });
    let mut world = World::spawn(spec).expect("世界构造");
    // 强制 local 0 在 tick 1 到期（Start 间窗口——world 未跑任何 tick）。
    world.force_due_for_test(vec![0], 1);
    // tick 1 校验注入失败 → Abort（组回滚本 tick 日志域）。
    world.set_validator(Box::new(|ev| {
        if ev.tick == 1 {
            Err("注入：tick1 拒绝".to_owned())
        } else {
            Ok(())
        }
    }));
    assert!(world.run_ticks(1).is_err(), "注入拒绝的 tick 1 必须失败");
    // Abort 后完整状态：next_due 恢复强制前值 100（指令失效）。
    let attention = world.collect_group_attention().expect("注意力收集");
    assert_eq!(
        attention[0].1,
        vec![(0, 100)],
        "Abort 冲回强制前 next_due（指令失效需重发）"
    );
    // 重跑比较：不重发指令 → 同 tick 重跑成功但 NPC 不到期（next_due 保持
    // 100——若指令残留，重跑轮会再次决策并重排 next_due）。decide_calls_
    // total 为单调计数（Abort 不冲回），不作为事务性判据。
    world.set_validator(no_op_validator());
    world.run_ticks(1).expect("Abort 后重跑 tick1 成功");
    let attention = world.collect_group_attention().expect("再收集");
    assert_eq!(
        attention[0].1,
        vec![(0, 100)],
        "未重发的指令不生效——重跑轮 NPC 不到期"
    );
    let _ = world.shutdown().expect("关停对账");
}

// ── 5c. 续行轮受理事实完整传递（Sol 二审修复集 3）──

// 多轮 tick 的**续行轮**里受理的挂单，其 Accepted 事实必须在该轮收束后
// 即下行组（先于 Continue——身份在续行轮落账/再报价前就位）。此前只在
// 最终收束轮转发：续行轮受理的单在组内身份迟到（无 fill 即永不 adopt，
// 后续 Closed 释放走未受理路径——执行腿悬挂）。
#[test]
fn sol3_continuation_round_acceptance_reaches_group_before_continue() {
    use super::group::Observed;
    // tick1 轮 0 挂买（挂簿受理）+声明续行；轮 1 空收口。
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, round| {
            let mut plan = TickPlan::default();
            if tick == 1 && round == 0 {
                plan.places.push(buy(0, 0, 1000, 100));
                plan.continuation = true;
            }
            plan
        })
    };
    let mut world = World::spawn(basic_spec(1, 1, 1, Box::new(script))).expect("世界构造");
    world.run_ticks(1).expect("多轮 tick 收敛");
    let report = world.shutdown().expect("关停对账");
    let observed = &report.group_finals[0].observed;
    let continue_at = observed
        .iter()
        .position(|o| matches!(o, Observed::Continue(1, 1)))
        .expect("应有 Continue(1,1)（轮 0 声明续行）");
    let accepted_at = observed
        .iter()
        .position(|o| matches!(o, Observed::ChildAccepted(..)))
        .expect("轮 0 挂单受理事实应下行组（终轮转发也应有）");
    assert!(
        accepted_at < continue_at,
        "续行轮受理事实必须先于 Continue 下行（受理@{accepted_at} ≥ Continue@{continue_at}）"
    );
}

// ── 6. panic 运输（resume_unwind 原 payload）──

#[test]
fn panic_transport_resumes_original_payload_and_survivors_rollback() {
    let script = |g: GroupIdx| -> GroupScript {
        Box::new(move |tick, _round| {
            let mut plan = TickPlan::default();
            if tick == 2 && g == 0 {
                // 组 0 在 tick2 注入 panic（原 payload 字符串）。
                plan.panic_payload = Some("boom-skeleton-payload".to_owned());
            }
            if tick == 2 {
                // 全体组在 tick2 前都有扣除（幸存者须回滚）。
                plan.places.push(buy(0, 0, 1000, 100));
            }
            plan
        })
    };
    let mut world = World::spawn(basic_spec(2, 1, 1, Box::new(script))).expect("世界构造");
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| world.run_ticks(3)))
        .expect_err("panic 应穿透 run_ticks");
    let msg = payload
        .downcast_ref::<String>()
        .expect("payload 应为原 String");
    assert_eq!(msg, "boom-skeleton-payload", "resume_unwind 不吞不改写");
    // 撤退路径已收 Final：幸存者回滚到 tick 起始（tick1 空闲=初始）。
    let report = world.shutdown().expect("撤退后 Final 对账");
    assert_eq!(report.group_finals.len(), 2);
    for f in &report.group_finals {
        assert_eq!(f.accounts[0].cash, 1_000_000, "幸存者扣除已回滚");
    }
    assert!(report.desk_finals[0].resting.is_empty());
}

// ── 7. worker 失联显式失败（断连或看门狗，不静默挂死）──

#[test]
fn severed_control_channels_fail_explicitly() {
    // 切断协调者下行：worker 收不到 Start。组通道仍被交易部交叉持有 ⇒ 组靠
    // 看门狗空闲超时显式上报（断连路径由 worker 退出后的 Final 错位兜底）——
    // 两条检测路径都是显式失败，不允许静默挂死。
    let mut spec = basic_spec(2, 2, 1, Box::new(|_| idle_script()));
    spec.watchdog = WatchdogConfig {
        normal: Duration::from_millis(200),
        long: Duration::from_millis(500),
    };
    let mut world = World::spawn(spec).expect("世界构造");
    world.sever();
    let err = world.run_ticks(1).expect_err("失联须显式失败");
    match &err {
        super::protocol::RuntimeError::Watchdog { phase, .. } => {
            assert_eq!(*phase, Phase::TickConverge, "协调者侧超时携相位");
        }
        super::protocol::RuntimeError::WorkerFailed { who, error } => {
            let _ = who;
            assert!(
                matches!(
                    error.as_ref(),
                    super::protocol::RuntimeError::Watchdog { .. }
                ),
                "worker 失联上报应为看门狗超时，收到 {error:?}"
            );
        }
        super::protocol::RuntimeError::Disconnected { .. } => {}
        other => panic!("应为显式失败（Watchdog/WorkerFailed/Disconnected），收到 {other:?}"),
    }
}

// ── 8. 看门狗相位感知 ──

#[test]
fn watchdog_timeout_carries_phase_context() {
    // 组在 tick1 决策期停顿 400ms；协调者正常档 100ms → TickConverge 超时。
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| {
            let mut plan = TickPlan::default();
            if tick == 1 {
                plan.stall = Some(Duration::from_millis(400));
            }
            plan
        })
    };
    let mut spec = basic_spec(1, 1, 1, Box::new(script));
    spec.watchdog = WatchdogConfig {
        normal: Duration::from_millis(100),
        long: Duration::from_secs(10),
    };
    let mut world = World::spawn(spec).expect("世界构造");
    let err = world.run_ticks(1).expect_err("停顿须触发看门狗");
    match &err {
        super::protocol::RuntimeError::Watchdog { phase, waited, .. } => {
            assert_eq!(*phase, Phase::TickConverge, "超时须携带相位上下文");
            assert_eq!(*waited, Duration::from_millis(100));
            assert!(err.to_string().contains("TickConverge"), "消息含相位名");
        }
        other => panic!("应为 Watchdog，收到 {other:?}"),
    }
    let _ = world.shutdown();
}

#[test]
fn watchdog_phase_limits_differ_for_long_phases() {
    // 单元：Drain/FinalCollection 走长档，正常相位走紧档（不静默放大全局阈值）。
    let cfg = WatchdogConfig {
        normal: Duration::from_millis(50),
        long: Duration::from_secs(5),
    };
    assert_eq!(
        cfg.limit_for(Phase::TickConverge),
        Duration::from_millis(50)
    );
    assert_eq!(cfg.limit_for(Phase::WorkerIdle), Duration::from_millis(50));
    assert_eq!(cfg.limit_for(Phase::Drain), Duration::from_secs(5));
    assert_eq!(
        cfg.limit_for(Phase::FinalCollection),
        Duration::from_secs(5)
    );
    // 空通道超时上报携带相位（用小阈值验证，避免单测等满长档时长）。
    let (_tx, rx) = mpsc::channel::<u8>();
    let tiny = WatchdogConfig {
        normal: Duration::from_millis(20),
        long: Duration::from_millis(80),
    };
    let err = recv_deadline(&rx, Phase::Drain, &tiny).expect_err("空通道必超时");
    assert!(matches!(
        err,
        super::protocol::RuntimeError::Watchdog {
            phase: Phase::Drain,
            ..
        }
    ));
}

// ── 9. 受理拒绝：非整手买单退回 ──

#[test]
fn desk_rejects_odd_lot_buy_and_refunds_via_entities() {
    // 买入 150 股（非整手）：交易部受理拒绝 → 实体原路退回 + 显式回执计数。
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| {
            let mut plan = TickPlan::default();
            if tick == 1 {
                plan.places.push(buy(0, 0, 1000, 150));
            }
            plan
        })
    };
    let mut world = World::spawn(basic_spec(1, 1, 1, Box::new(script))).expect("世界构造");
    world
        .run_ticks(1)
        .expect("tick1 收敛（拒绝是业务结果非失败）");
    let report = world.shutdown().expect("关停");
    assert_eq!(report.desk_finals[0].skipped_orders, 1, "受理拒绝显式计数");
    assert_eq!(
        report.group_finals[0].accounts[0].cash, 1_000_000,
        "拒绝退款回流"
    );
    assert!(report.desk_finals[0].resting.is_empty());
}

// ── 10. 严格所有权 skip 计数 ──

#[test]
fn insufficient_cash_skips_explicitly_without_partial_execution() {
    // 超额买单（purse 超现金）：组端显式 skip，不部分执行。
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| {
            let mut plan = TickPlan::default();
            if tick == 1 {
                plan.places.push(buy(0, 0, 1000, 100_000));
            }
            plan
        })
    };
    let mut world = World::spawn(basic_spec(1, 1, 1, Box::new(script))).expect("世界构造");
    world.run_ticks(1).expect("skip 是业务结果非失败");
    let report = world.shutdown().expect("关停");
    assert_eq!(
        report.group_finals[0].skipped_orders, 1,
        "组端 skip 显式计数"
    );
    assert_eq!(report.group_finals[0].accounts[0].cash, 1_000_000, "无扣除");
    assert!(report.desk_finals[0].resting.is_empty(), "无单到簿");
}

// ── 11. Drain→Shutdown→Final 对账：期末在途清零 ──

#[test]
fn drain_shutdown_final_leaves_no_inflight_entities() {
    // 多组多股挂单后关停：全部实体回流、在途清零、Final 全数上交。
    let script = |g: GroupIdx| -> GroupScript {
        Box::new(move |tick, _round| {
            let mut plan = TickPlan::default();
            if tick == 1 {
                plan.places.push(buy(0, (g % 2) as u16, 1000, 100));
                plan.places.push(sell(0, ((g + 1) % 2) as u16, 1000, 100));
            }
            plan
        })
    };
    let mut world = World::spawn(basic_spec(3, 2, 1, Box::new(script))).expect("世界构造");
    world.run_ticks(2).expect("2 tick 收敛");
    let report: ShutdownReport = world.shutdown().expect("关停对账");
    assert_eq!(report.group_finals.len(), 3, "每组 Final 恰一条");
    assert_eq!(report.desk_finals.len(), 2, "每交易部 Final 恰一条");
    // 守恒断言（批 3 起撮合活跃，账户间流动）：现金总量 = 初始 − 费用桶合计；
    // 股份总量不变；在途清零。
    let mut cash_total = 0i64;
    let mut fee_total = 0i64;
    let mut shares_by_stock = vec![0u32; 2];
    for f in &report.group_finals {
        for acct in &f.accounts {
            cash_total += acct.cash;
            for (s, &qty) in acct.shares.iter().enumerate() {
                shares_by_stock[s] += qty;
            }
            assert!(
                acct.foreign_active.iter().all(|&x| x == 0),
                "组{}在途清零",
                f.group
            );
        }
    }
    for f in &report.desk_finals {
        fee_total += f.fee_sink;
        assert!(f.resting.is_empty(), "交易部{}清簿", f.stock);
        assert_eq!(f.skipped_orders, 0);
    }
    assert_eq!(
        cash_total + fee_total,
        3 * 1_000_000,
        "现金守恒（含 fee_sink）"
    );
    assert!(
        shares_by_stock.iter().all(|&s| s == 3_000),
        "股份守恒：{:?}",
        shares_by_stock
    );
}

// D0：World 线程回收——Drop 兜底关停（显式 shutdown 之外的丢弃路径
// 不得泄漏组/部线程）。
#[test]
fn d0_world_drop_shuts_down_workers() {
    fn task_count() -> usize {
        std::fs::read_dir("/proc/self/task").map(|d| d.count()).unwrap_or(0)
    }
    let before = task_count();
    let script = |_g: super::GroupIdx| -> GroupScript {
        Box::new(|_t, _r| TickPlan::default())
    };
    let world = World::spawn(basic_spec(1, 2, 1, Box::new(script))).expect("世界构造");
    // 等线程注册（≤2s）。
    let spawned = loop {
        let c = task_count();
        if c >= before + 3 {
            break c;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    drop(world);
    // 等线程退出回落（≤2s）——Drop 兜底须发 Shutdown 并 join。
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        let c = task_count();
        if c <= before {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Drop 后线程未回收：before={before} spawned={spawned} now={c}"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}
