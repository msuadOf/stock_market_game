//! 跨股臂的世界构造：家端名册（primary + secondaries）与流通股切分。
//!
//! 分配规则（对齐按股私有臂的守恒口径）：
//! - inst 53% / hot 2% 与按股私有臂完全一致（home 股主仓）。
//! - retail 份额 45% 拆成「主仓池 (1−cross_rate)」与「跨股池 cross_rate」：
//!   主仓池切给本股家端 retail（与原臂同法）；跨股池按 lot 均分给**其它股**
//!   家端的跨股 NPC 作为次级仓。未被分配的零头入 float_remainder 桶。
//! - 跨股 NPC 选择与其次级股选择是 id/seed 的纯函数（确定性）：
//!   draw%100 < cross_pct×100 → 跨股；其中一半再拿第二个次级股。
//!   目标形态：cross_pct=30 时约 30% retail 持 2~3 只股（各半）。

use crate::npc::{Npc, XPos};
use crate::setup::ProtoConfig;
use crate::ProtoError;
use engine::money::Money;
use engine::session::SplitMix64;
use engine::strategy::{StrategyData, Rng, TargetPolicy};

/// 跨股构造的统计（报告与自检用）。
#[derive(Clone, Debug)]
pub struct CrossSetupInfo {
    /// 按次级仓数量统计的 retail 数（下标 = 次级仓数 0..=3，即持 1..4 只股；
    /// 本负载最多 2 个次级，inst/hot 不计入）。
    pub hold_counts: [u64; 4],
    /// 全体 retail 中持 ≥2 只股的比例。
    pub cross_fraction: f64,
    /// 因不足一手被丢弃的次级仓分配数（s5 规模为 0，防御性计数）。
    pub dropped_zero: u64,
}

/// 家端名册起始 id（与 run.rs 的 worker 派号口径一致：每股 retail+inst+1 槽）。
pub fn x_id_base(cfg: &ProtoConfig, stock_idx: usize) -> u32 {
    let mut base = 0u32;
    for _ in 0..stock_idx {
        base += cfg.retail_per_stock + cfg.inst_per_stock + 1;
    }
    base
}

/// 纯函数：npc_id 是否为跨股 NPC，及其次级股列表（不含 home）。
/// 由 id/seed/参数决定，所有 worker 各自重算得到一致结果。
fn secondaries_of(cfg: &ProtoConfig, cross_pct: u32, npc_id: u32, home: u8) -> Vec<u8> {
    let n_stocks = cfg.stocks.len() as u8;
    if n_stocks < 2 || cross_pct == 0 {
        return Vec::new();
    }
    let mut rng = SplitMix64::new(cfg.seed ^ 0x5EED_C1055 ^ npc_id as u64);
    if rng.next_range_u32(0, 100) >= cross_pct {
        return Vec::new();
    }
    // 第一个次级股：非 home 中均匀取（标准排除映射：k ≥ home 则 +1）。
    let first = rng.next_range_u32(0, n_stocks as u32 - 1) as u8;
    let first = if first >= home { first + 1 } else { first };
    // 一半的跨股 NPC 拿第二个次级股（≠ home、≠ first；双排除映射需要 ≥3 只股）。
    if n_stocks >= 3 && rng.next_range_u32(0, 2) == 0 {
        let (e0, e1) = (home.min(first), home.max(first));
        let mut second = rng.next_range_u32(0, n_stocks as u32 - 2) as u8;
        if second >= e0 {
            second += 1;
        }
        if second >= e1 {
            second += 1;
        }
        vec![first, second]
    } else {
        vec![first]
    }
}

/// 全局需求统计：每股被多少次级仓指向（所有 worker 一致重算）。
fn global_demand(cfg: &ProtoConfig, cross_pct: u32) -> Vec<u64> {
    let mut demand = vec![0u64; cfg.stocks.len()];
    for stock_idx in 0..cfg.stocks.len() {
        let home = stock_idx as u8;
        let id_base = x_id_base(cfg, stock_idx);
        for i in 0..cfg.retail_per_stock {
            for target in secondaries_of(cfg, cross_pct, id_base + i, home) {
                demand[target as usize] += 1;
            }
        }
    }
    demand
}

/// 对齐到 100 股一手。
fn align_lot(q: u64) -> u32 {
    (q / 100 * 100) as u32
}

/// 构造一股的家端名册（含跨股 NPC 的次级仓）。
///
/// 返回 (roster, float_remainder, info)。次级仓每份数量由全局需求决定：
/// lot = align_lot(该股跨股池 / 该股次级需求总数)，不足一手的分配被
/// 显式丢弃并计数。
pub fn build_cross_roster(
    cfg: &ProtoConfig,
    stock_idx: usize,
    cross_pct: u32,
) -> Result<(Vec<Npc>, u64, CrossSetupInfo), ProtoError> {
    let spec = &cfg.stocks[stock_idx];
    let home = stock_idx as u8;
    let id_base = x_id_base(cfg, stock_idx);

    let demand = global_demand(cfg, cross_pct);
    let cross_rate = cross_pct as f64 / 100.0;
    // 每股每份次级仓的 lot 大小。
    let mut secondary_lot = vec![0u32; cfg.stocks.len()];
    for (s, target_spec) in cfg.stocks.iter().enumerate() {
        if demand[s] == 0 {
            continue;
        }
        let cross_pool =
            (target_spec.float_shares as f64 * cfg.float_split.0 * cross_rate) as u64;
        secondary_lot[s] = align_lot(cross_pool / demand[s]);
    }

    let mut npcs: Vec<Npc> = Vec::new();
    let mut allocated: u64 = 0;
    let mut dropped_zero = 0u64;
    let mut hold_counts = [0u64; 4];

    // retail：主仓池 = float × 45% × (1 − cross_rate)。
    let primary_pool = (spec.float_shares as f64 * cfg.float_split.0 * (1.0 - cross_rate)) as u64;
    let per_retail = align_lot(primary_pool / cfg.retail_per_stock as u64);
    if crate::xdbg() {
        eprintln!("[xsetup w{stock_idx}] demand={demand:?} lots={secondary_lot:?} per_retail={per_retail}");
    }
    for i in 0..cfg.retail_per_stock {
        let id = id_base + i;
        let mut npc = Npc::new(
            id,
            StrategyData::retail(cfg.retail_params.0, cfg.retail_params.1, cfg.retail_params.2),
            cfg.retail_cash_cents,
            per_retail,
            spec.initial_price_cents,
        );
        for target in secondaries_of(cfg, cross_pct, id, home) {
            let lot = secondary_lot[target as usize];
            if lot == 0 {
                dropped_zero += 1;
                continue;
            }
            npc.secondaries.push(XPos {
                stock_idx: target,
                qty: lot,
                sellable: lot,
                reserved_shares: 0,
                frozen_today: 0,
                cost_price_cents: cfg.stocks[target as usize].initial_price_cents,
            });
        }
        hold_counts[npc.secondaries.len().min(3)] += 1;
        // 守恒口径：allocated 只计「本股的股份」——主仓属于本股流通股；
        // 次级仓属于目标股的流通股，由目标股的跨股池预算约束（见下）。
        allocated += npc.qty as u64;
        debug_assert!(npc.qty % 100 == 0, "主仓必须整手");
        npcs.push(npc);
    }
    // inst（与按股私有臂一致，53% 池）。
    let inst_pool = (spec.float_shares as f64 * cfg.float_split.1) as u64;
    let per_inst = align_lot(inst_pool / cfg.inst_per_stock as u64);
    for i in 0..cfg.inst_per_stock {
        let npc = Npc::new(
            id_base + cfg.retail_per_stock + i,
            StrategyData::inst(
                TargetPolicy::Fixed(Money::from_cents(spec.initial_price_cents)),
                cfg.inst_params.0,
                cfg.inst_params.1,
            ),
            cfg.inst_cash_cents,
            per_inst,
            spec.initial_price_cents,
        );
        allocated += npc.qty as u64;
        npcs.push(npc);
    }
    // hot（与按股私有臂一致，轮流放前 hot_total 只股票）。
    if (cfg.hot_total as usize) > stock_idx {
        let hot_pool = (spec.float_shares as f64 * cfg.float_split.2) as u64;
        let per_hot = align_lot(hot_pool);
        let npc = Npc::new(
            id_base + cfg.retail_per_stock + cfg.inst_per_stock,
            StrategyData::hot(cfg.hot_params.0, cfg.hot_params.1, cfg.hot_params.2),
            cfg.hot_cash_cents,
            per_hot,
            spec.initial_price_cents,
        );
        allocated += npc.qty as u64;
        npcs.push(npc);
    }
    // 本股流通股的完整扣减：主仓 + inst + hot（本名册分配）
    // + 本股跨股池被外部家端实际领走的次级仓（lot × 全局需求，确定性）。
    let secondary_out = secondary_lot[stock_idx] as u64 * demand[stock_idx];
    let allocated_for_stock = allocated + secondary_out;
    let remainder = spec
        .float_shares
        .checked_sub(allocated_for_stock)
        .ok_or_else(|| {
            ProtoError::InvalidConfig(format!(
                "股票 {} 跨股切分溢出：float={} allocated={}（含跨股池流出 {}）",
                spec.code, spec.float_shares, allocated_for_stock, secondary_out
            ))
        })?;
    let retail_total: u64 = cfg.retail_per_stock as u64 * cfg.stocks.len() as u64;
    let hold_2_or_3 = hold_counts[1] + hold_counts[2] + hold_counts[3];
    let info = CrossSetupInfo {
        hold_counts,
        cross_fraction: if retail_total > 0 {
            hold_2_or_3 as f64 / retail_total as f64
        } else {
            0.0
        },
        dropped_zero,
    };
    Ok((npcs, remainder, info))
}
