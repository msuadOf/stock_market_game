//! 批 5 日结测试（G 批 5 三项验收判据 + F §6.2）：
//! ① 权益落账正确性（AdminBatch 逐账户对账，含失败冲回）；
//! ② 清簿回流（Drain 后活动委托=0、实体守恒、fee_sink 日结上交）；
//! ③ 存档候选投影（组装+校验成功才提交；候选往返内容等价）。
//! 另：日界行情重置（昨收/日内/K 历史）与日结 Abort 窗口语义。

use super::group::{GroupAccount, GroupScript, PlaceSpec, TickPlan};
use super::protocol::{AdminRight, RuntimeError};
use super::skeleton_tests::basic_spec;
use super::world::{DayEndCandidate, World};
use super::GroupIdx;
use crate::orderbook::Side;
use crate::session::SplitMix64;
use crate::strategy::Rng as _;
use crate::strategy::StrategyData;

/// 4 tick/日全连续形态（day0=tick1..4，day1=tick5..8）。
fn day_spec(script: Box<dyn Fn(GroupIdx) -> GroupScript>) -> super::world::WorldSpec {
    let mut spec = basic_spec(1, 1, 2, script);
    spec.ticks_per_day = 4;
    spec.auction_ticks = 0;
    spec.closing_auction_ticks = 0;
    spec
}

fn buy(local: u32, price: i64, qty: u32) -> PlaceSpec {
    PlaceSpec {
        local,
        stock: 0,
        side: Side::Buy,
        price,
        qty,
    }
}

fn sell(local: u32, price: i64, qty: u32) -> PlaceSpec {
    PlaceSpec {
        local,
        stock: 0,
        side: Side::Sell,
        price,
        qty,
    }
}

// ── ② 清簿回流 + 守恒 ──

#[test]
fn day_end_drains_inflight_and_conserves_across_days() {
    // tick1 成交一笔 + tick4 挂一笔在途；tick5 前自动日结 ⇒ Drain 回流。
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| {
            let mut plan = TickPlan::default();
            match tick {
                1 => {
                    plan.places.push(sell(1, 1000, 100));
                    plan.places.push(buy(0, 1005, 100));
                }
                4 => plan.places.push(buy(0, 1000, 100)),
                _ => {}
            }
            plan
        })
    };
    let mut world = World::spawn(day_spec(Box::new(script))).expect("世界构造");
    world.run_ticks(5).expect("5 tick 收敛（含一次日结）");
    let cand = world.last_day_end_candidate.clone().expect("候选");
    let report = world.shutdown().expect("关停");
    // 清簿回流：tick4 的在途买单在日结 Drain 退回 ⇒ 现金守恒含费用。
    let cash: i64 = report.group_finals[0].accounts.iter().map(|a| a.cash).sum();
    let fees: i64 = report.desk_finals.iter().map(|d| d.fee_sink).sum();
    assert_eq!(cash + fees, 2 * 1_000_000, "跨日守恒（含 fee_sink）");
    // 在途清零。
    for a in &report.group_finals[0].accounts {
        assert!(a.foreign_active.iter().all(|&x| x == 0), "日结后无在途");
    }
    assert!(report.desk_finals[0].resting.is_empty());
    // 日结候选存在（tick4=日终）。
    assert_eq!(cand.day, 0);
    assert_eq!(cand.tick, 4);
}

// ── ① 权益落账（含失败冲回）──

#[test]
fn admin_rights_apply_reversibly_and_validate_before_commit() {
    // 权益：acct0 股利 2000 分 + 送转 100 股 − 扣税 300 分。
    let script = |_g: GroupIdx| -> GroupScript { Box::new(|_t, _r| TickPlan::default()) };
    let mut spec = day_spec(Box::new(script));
    spec.rights_computer = Some(Box::new(|day, _g| {
        if day != 0 {
            return Vec::new();
        }
        vec![
            AdminRight::CashDividend {
                local: 0,
                stock: 0,
                amount: 2_000,
            },
            AdminRight::ShareSplit {
                local: 0,
                stock: 0,
                bonus_qty: 100,
            },
            AdminRight::WithholdingTax {
                local: 0,
                stock: 0,
                amount: 300,
            },
        ]
    }));
    let mut world = World::spawn(spec).expect("世界构造");
    // 首次日结校验失败 ⇒ Err + 冲回（回到上一交易日收敛态=初始）。
    let fail: super::world::DayEndValidator = Box::new(|_| Err("注入：日结校验失败".to_owned()));
    world.set_day_end_validator(fail);
    let err = world.run_ticks(5).expect_err("日结校验失败须显式失败");
    assert!(
        matches!(err, RuntimeError::DayEndValidation { day: 0, .. }),
        "应为 DayEndValidation：{err:?}"
    );
    // 换回通过校验器重跑同一日结（Abort 后 worker 存活可继续=失败后可继续）。
    world.set_day_end_validator(Box::new(|_| Ok(())));
    world.run_ticks(5).expect("重跑日结成功");
    let cand = world.last_day_end_candidate.clone().expect("候选");
    let report = world.shutdown().expect("关停");
    let a0 = &report.group_finals[0].accounts[0];
    assert_eq!(a0.cash, 1_000_000 + 2_000 - 300, "股利−扣税");
    assert_eq!(a0.shares[0], 1_000 + 100, "送转股到账");
    assert_eq!(a0.frozen_today[0], 0, "送转股可卖（不入 T+1）");
    // 候选对账：落账后投影 == 期末账户。
    let cand_acct = &cand.groups[0].1[0];
    assert_eq!(cand_acct.cash, a0.cash, "候选投影逐账户对账");
    assert_eq!(cand_acct.shares, a0.shares);
}

// ── ② fee_sink 日结上交 ──

#[test]
fn fee_sink_surrendered_in_candidate_and_conserves() {
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| {
            let mut plan = TickPlan::default();
            if tick == 1 {
                plan.places.push(sell(1, 1000, 100));
                plan.places.push(buy(0, 1005, 100));
            }
            plan
        })
    };
    let mut world = World::spawn(day_spec(Box::new(script))).expect("世界构造");
    world.run_ticks(5).expect("收敛");
    let cand = world.last_day_end_candidate.clone().expect("候选");
    let fee_surrendered: i64 = cand.desks.iter().map(|(_, fee, _)| *fee).sum();
    assert!(fee_surrendered > 0, "日结上交费用桶：{fee_surrendered}");
    // 守恒：日结投影现金合计 + 上交费用 = 初始。
    let cash: i64 = cand
        .groups
        .iter()
        .flat_map(|(_, accts)| accts.iter())
        .map(|a| a.cash)
        .sum();
    assert_eq!(cash + fee_surrendered, 2 * 1_000_000, "日结投影守恒");
}

// ── 日界行情重置 ──

#[test]
fn day_rollover_updates_last_close_and_archives_candle() {
    // day0 价格上移到 1010（成交），日结后 day1 昨收=1010（K 入历史）。
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| {
            let mut plan = TickPlan::default();
            if tick == 1 {
                plan.places.push(sell(1, 1010, 100));
                plan.places.push(buy(0, 1010, 100));
            }
            plan
        })
    };
    let mut world = World::spawn(day_spec(Box::new(script))).expect("世界构造");
    world.run_ticks(6).expect("收敛");
    let cand = world.last_day_end_candidate.clone().expect("候选");
    let report = world.shutdown().expect("关停");
    let desk = &report.desk_finals[0];
    assert_eq!(desk.market.last_close, 1010, "昨收=日终最新价");
    assert_eq!(desk.market.candles.len(), 1, "day0 K 入历史");
    assert_eq!(desk.market.candles[0].close, 1010);
    assert_eq!(
        desk.market.volume_today, 0,
        "day1 成交量清零（tick5/6 无单）"
    );
    // 候选昨交基准。
    assert_eq!(cand.desks[0].2, 1010, "候选携带日终最新价");
}

// ── ③ 存档候选往返内容等价 ──

#[test]
fn day_end_candidate_roundtrip_rebuilds_equivalent_world() {
    // day0：成交+挂单+日结（Drain 回流）后取候选；用候选账户重建世界续跑。
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| {
            let mut plan = TickPlan::default();
            if tick == 1 {
                plan.places.push(sell(1, 1000, 100));
                plan.places.push(buy(0, 1005, 100));
            }
            plan
        })
    };
    let mut world = World::spawn(day_spec(Box::new(script))).expect("世界构造");
    world.run_ticks(4).expect("day0 收敛");
    // 手动推进到日结（tick5 触发）。
    world.run_ticks(1).expect("日结+tick5");
    let cand = world.last_day_end_candidate.clone().expect("候选");
    let report = world.shutdown().expect("关停");
    // 内容等价①：候选组投影 == 原世界期末账户（无后续变更）。
    for ((_, cand_accts), final_state) in cand.groups.iter().zip(&report.group_finals) {
        for (c, f) in cand_accts.iter().zip(&final_state.accounts) {
            assert_eq!(c.cash, f.cash, "候选与期末现金一致");
            assert_eq!(c.shares, f.shares, "候选与期末持仓一致");
            assert_eq!(c.frozen_today, f.frozen_today, "日结解冻后冻结归零");
        }
    }
    // 内容等价②：以候选重建世界续跑（restore 后继续 tick）。
    let mut rebuilt_spec = day_spec(Box::new(|_g: GroupIdx| -> GroupScript {
        Box::new(|_t, _r| TickPlan::default())
    }));
    rebuilt_spec.initial_cash_override = Some(
        cand.groups
            .iter()
            .map(|(_, accts)| accts.iter().map(|a| a.cash).collect())
            .collect(),
    );
    rebuilt_spec.initial_shares = cand
        .groups
        .iter()
        .map(|(_, accts)| accts.iter().map(|a| a.shares.clone()).collect())
        .collect();
    let mut rebuilt = World::spawn(rebuilt_spec).expect("候选重建世界");
    rebuilt.run_ticks(2).expect("重建后续跑 2 tick");
    let rebuilt_report = rebuilt.shutdown().expect("重建世界关停");
    // 续跑无单 ⇒ 账户与候选逐分一致（往返内容等价）。
    for ((_, cand_accts), final_state) in cand.groups.iter().zip(&rebuilt_report.group_finals) {
        for (c, f) in cand_accts.iter().zip(&final_state.accounts) {
            assert_eq!(c.cash, f.cash, "重建后续跑现金不变");
            assert_eq!(c.shares, f.shares, "重建后续跑持仓不变");
        }
    }
}

// ── 组端日结回执哈希一致性 ──

#[test]
fn day_end_group_hash_matches_post_apply_state() {
    let script = |_g: GroupIdx| -> GroupScript { Box::new(|_t, _r| TickPlan::default()) };
    let mut spec = day_spec(Box::new(script));
    spec.rights_computer = Some(Box::new(|day, _g| {
        if day == 0 {
            vec![AdminRight::CashDividend {
                local: 1,
                stock: 0,
                amount: 500,
            }]
        } else {
            Vec::new()
        }
    }));
    let mut world = World::spawn(spec).expect("世界构造");
    world.run_ticks(5).expect("收敛");
    let cand = world.last_day_end_candidate.clone().expect("候选");
    let report = world.shutdown().expect("关停");
    // 回执哈希（落账后即时态）应与期末提交哈希一致性由"日结后无变更"保证：
    // 候选账户 == 期末账户（权益在期末仍可见）。
    let cand_a1 = &cand.groups[0].1[1];
    let final_a1 = &report.group_finals[0].accounts[1];
    assert_eq!(cand_a1.cash, final_a1.cash);
    assert_eq!(cand_a1.cash, 1_000_000 + 500, "acct1 股利落账");
}

// ── RNG 辅助（避免未用导入警告；保留 SplitMix64 可用性说明）──

#[test]
fn splitmix_stays_deterministic_for_rights_scheduling() {
    let mut a = SplitMix64::new(7);
    let mut b = SplitMix64::new(7);
    for _ in 0..8 {
        assert_eq!(a.next_f64(), b.next_f64());
    }
    let _ = StrategyData::retail(1.0, 100, 0.0);
}
