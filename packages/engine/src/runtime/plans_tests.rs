//! 批 6 计划链续行测试（G 批 6 验收判据）：成交驱动的跨股多步链经 Continue
//! 轮排空、终止上报、轮数上限显式失败、多轮原子性（Abort 回放到 tick 起始）。

use super::decision::PendingOp;
use super::group::{GroupScript, PlaceSpec, TickPlan};
use super::plans::{PlanChain, PlanStepOutput};
use super::protocol::RuntimeError;
use super::skeleton_tests::basic_spec;
use super::world::World;
use super::{FillFact, GroupIdx, Owner};
use crate::orderbook::Side;

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

fn pending_buy(local: u32, stock: u16, price: i64, qty: u32) -> PendingOp {
    PendingOp {
        local,
        stock,
        side: Side::Buy,
        routed: super::protocol::RoutedPrice::Fixed(price),
        purse_reference: price,
        qty,
    }
}

/// 跨股两步链：根命令买 A（100 股）；见到 A 的成交后买 B（100 股）；完成。
struct CrossStockLadder {
    owner: Owner,
    step: u32,
}

impl PlanChain for CrossStockLadder {
    fn id(&self) -> u64 {
        1
    }
    fn next_step(&mut self, fills: &[FillFact]) -> PlanStepOutput {
        match self.step {
            0 => {
                self.step = 1;
                PlanStepOutput {
                    ops: vec![pending_buy(self.owner.1, 0, 1000, 100)],
                    has_more_steps: true,
                }
            }
            1 => {
                // 只认本账户在 A 的成交（owner/order 过滤语义由计划自持）。
                let filled_a = fills
                    .iter()
                    .any(|f| f.owner == self.owner && f.side == Side::Buy && f.order.0 == 0);
                if !filled_a {
                    // 未收到结果 ⇒ 仍需续行轮（等待回流的 FillFact）。
                    return PlanStepOutput {
                        ops: vec![],
                        has_more_steps: true,
                    };
                }
                self.step = 2;
                PlanStepOutput {
                    ops: vec![pending_buy(self.owner.1, 1, 1000, 100)],
                    has_more_steps: true,
                }
            }
            _ => PlanStepOutput {
                ops: vec![],
                has_more_steps: false,
            },
        }
    }
}

#[test]
fn cross_stock_plan_drains_via_continue_rounds() {
    // 股 A/B 各有对手卖单（脚本挂）；计划：买 A →（A 成交后）买 B → 完成。
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| {
            let mut plan = TickPlan::default();
            if tick == 1 {
                plan.places.push(sell(1, 0, 1000, 100));
                plan.places.push(sell(1, 1, 1000, 100));
            }
            plan
        })
    };
    let mut spec = basic_spec(1, 2, 2, Box::new(script));
    spec.plan_factory = Some(Box::new(|_g| {
        vec![Box::new(CrossStockLadder {
            owner: (0, 0),
            step: 0,
        })]
    }));
    let mut world = World::spawn(spec).expect("世界构造");
    world.run_ticks(1).expect("1 tick 收敛");
    // 轮 0：根命令买 A（与卖 A 成交）；轮 1：见 A 成交 ⇒ 买 B（与卖 B 成交）
    // ⇒ has_more=true 但轮 2 计划完成、无新单 ⇒ 终止。
    assert!(
        world.report_log[0].rounds >= 1,
        "至少一次 Continue 轮：{}",
        world.report_log[0].rounds
    );
    let report = world.shutdown().expect("关停");
    let a0 = &report.group_finals[0].accounts[0];
    assert_eq!(a0.shares[0], 1_100, "A 股=初始1000+计划步一100");
    assert_eq!(a0.shares[1], 1_100, "B 股=初始1000+计划步二100");
    assert!(
        a0.foreign_active.iter().all(|&x| x == 0),
        "计划排空在途清零"
    );
    // 守恒。
    let cash: i64 = report.group_finals[0].accounts.iter().map(|a| a.cash).sum();
    let fees: i64 = report.desk_finals.iter().map(|d| d.fee_sink).sum();
    assert_eq!(cash + fees, 2 * 1_000_000, "两腿成交守恒");
}

#[test]
fn completed_plan_reports_no_continuation() {
    // 单步即完成的计划：轮 0 后无续行。
    struct OneShot(Owner);
    impl PlanChain for OneShot {
        fn id(&self) -> u64 {
            2
        }
        fn next_step(&mut self, _fills: &[FillFact]) -> PlanStepOutput {
            if self.0 .1 == u32::MAX {
                return PlanStepOutput::default();
            }
            let local = std::mem::replace(&mut self.0 .1, u32::MAX);
            PlanStepOutput {
                ops: vec![pending_buy(local, 0, 1000, 100)],
                has_more_steps: false,
            }
        }
    }
    let mut spec = basic_spec(
        1,
        1,
        1,
        Box::new(|_g| Box::new(|_t, _r| TickPlan::default())),
    );
    spec.plan_factory = Some(Box::new(|_g| vec![Box::new(OneShot((0, 0)))]));
    let mut world = World::spawn(spec).expect("世界构造");
    world.run_ticks(1).expect("收敛");
    assert_eq!(world.report_log[0].rounds, 0, "无续行 ⇒ 单轮");
}

#[test]
fn never_ending_plan_hits_rounds_cap_explicitly() {
    // 永远 has_more 的计划：轮数上限显式失败（异常形状不静默）。
    struct NeverEnd;
    impl PlanChain for NeverEnd {
        fn id(&self) -> u64 {
            3
        }
        fn next_step(&mut self, _fills: &[FillFact]) -> PlanStepOutput {
            PlanStepOutput {
                ops: vec![],
                has_more_steps: true,
            }
        }
    }
    let mut spec = basic_spec(
        1,
        1,
        1,
        Box::new(|_g| Box::new(|_t, _r| TickPlan::default())),
    );
    spec.rounds_cap = 2;
    spec.plan_factory = Some(Box::new(|_g| vec![Box::new(NeverEnd)]));
    let mut world = World::spawn(spec).expect("世界构造");
    let err = world.run_ticks(1).expect_err("超轮数上限须显式失败");
    assert!(
        matches!(
            err,
            RuntimeError::RoundsExceeded {
                tick: 1,
                rounds_cap: 2
            }
        ),
        "应为 RoundsExceeded：{err:?}"
    );
    let _ = world.shutdown();
}

#[test]
fn abort_after_plan_rounds_rolls_back_to_tick_start() {
    // 跨股链推进到轮 1 后注入校验失败 ⇒ Abort ⇒ 三方回到 tick 起始。
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| {
            let mut plan = TickPlan::default();
            if tick == 1 {
                plan.places.push(sell(1, 0, 1000, 100));
                plan.places.push(sell(1, 1, 1000, 100));
            }
            plan
        })
    };
    let mut spec = basic_spec(1, 2, 2, Box::new(script));
    spec.plan_factory = Some(Box::new(|_g| {
        vec![Box::new(CrossStockLadder {
            owner: (0, 0),
            step: 0,
        })]
    }));
    let mut world = World::spawn(spec).expect("世界构造");
    let fail: super::coordinator::Validator = Box::new(|ev| {
        if ev.round >= 1 {
            Err("注入：轮1 校验失败".to_owned())
        } else {
            Ok(())
        }
    });
    world.set_validator(fail);
    let err = world.run_ticks(1).expect_err("轮1 失败");
    assert!(
        matches!(
            err,
            RuntimeError::TickValidation {
                tick: 1,
                round: 1,
                ..
            }
        ),
        "应为 TickValidation：{err:?}"
    );
    // Abort 回放：组回到 tick 起始（计划两腿扣除全部冲回；成交回款冲回）。
    let report = world.shutdown().expect("关停");
    for f in &report.group_finals {
        for acct in &f.accounts {
            assert_eq!(acct.cash, 1_000_000, "计划扣除+成交全部冲回");
            assert!(acct.shares.iter().all(|&s| s == 1_000), "买入冲回");
            assert!(acct.frozen_today.iter().all(|&x| x == 0));
            assert!(acct.foreign_active.iter().all(|&x| x == 0));
        }
        assert_eq!(f.state_hash, f.committed_hash, "组哈希复原");
    }
    for f in &report.desk_finals {
        assert_eq!(f.rollback_hash, Some(f.committed_hash), "交易部回滚完整");
        assert_eq!(f.market.volume_today, 0, "成交回滚");
        assert_eq!(f.fee_sink, 0, "费用桶回滚");
    }
}
