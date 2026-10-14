//! s5 负载配置与世界构造（对齐 `agents/engine-max-performance/default-setup.json`）。

use crate::npc::Npc;
use crate::ProtoError;
use engine::money::Money;
use engine::strategy::{StrategyData, TargetPolicy};

/// 单只股票的静态规格。
#[derive(Clone, Debug)]
pub struct StockSpec {
    /// 股票代码（与引擎 StockCode 的字符串一致）。
    pub code: String,
    /// 初始价（分）。
    pub initial_price_cents: i64,
    /// 涨跌幅限制（主板 0.10，创业板 0.20）。
    pub limit_pct: f64,
    /// 流通股数。
    pub float_shares: u64,
}

/// 原型配置。数值口径全部为「分 / 股数」，与 engine Money 一致。
#[derive(Clone, Debug)]
pub struct ProtoConfig {
    pub seed: u64,
    pub stocks: Vec<StockSpec>,
    /// 每股 retail NPC 数。
    pub retail_per_stock: u32,
    /// 每股 inst NPC 数（s5=1）。
    pub inst_per_stock: u32,
    /// hot NPC 总数（s5=2，轮流放前 N 股）。
    pub hot_total: u32,
    /// 单个 retail 初始现金（分）。
    pub retail_cash_cents: i64,
    /// 单个 inst 初始现金（分）。
    pub inst_cash_cents: i64,
    /// 单个 hot 初始现金（分）。
    pub hot_cash_cents: i64,
    /// retail 参数 (arrival_rate, order_size_mean, chase_prob)。
    pub retail_params: (f64, u32, f64),
    /// inst 参数 (margin, order_size)。
    pub inst_params: (f64, u32),
    /// hot 参数 (lookback, trend_threshold, order_size)。
    pub hot_params: (usize, f64, u32),
    /// 模拟 tick 数。
    pub ticks: u64,
    /// 挂单 TTL（tick）。
    pub order_ttl_ticks: u64,
    /// float 切分比例 (retail, inst, hot)。
    pub float_split: (f64, f64, f64),
}

/// s5 负载 5 股静态表（出处 default-setup.json；float_shares 为流通股）。
fn s5_stock_table() -> Vec<StockSpec> {
    vec![
        StockSpec {
            code: "600101".into(),
            initial_price_cents: 112_000,
            limit_pct: 0.10,
            float_shares: 3_571_428_571,
        },
        StockSpec {
            code: "002156".into(),
            initial_price_cents: 273_500,
            limit_pct: 0.10,
            float_shares: 2_047_531_993,
        },
        StockSpec {
            code: "300260".into(),
            initial_price_cents: 368_000,
            limit_pct: 0.20,
            float_shares: 611_413_043,
        },
        StockSpec {
            code: "600610".into(),
            initial_price_cents: 75_500,
            limit_pct: 0.10,
            float_shares: 847_682_119,
        },
        StockSpec {
            code: "000812".into(),
            initial_price_cents: 28_500,
            limit_pct: 0.10,
            float_shares: 842_105_263,
        },
    ]
}

impl ProtoConfig {
    /// s5 负载：5 股 / 20,007 NPC（20,000 retail + 5 inst + 2 hot）/ 3000 tick。
    pub fn s5() -> Self {
        ProtoConfig {
            seed: 42,
            stocks: s5_stock_table(),
            retail_per_stock: 4_000,
            inst_per_stock: 1,
            hot_total: 2,
            // default-setup.json：retail_cash_median 20,000,000 元。
            retail_cash_cents: 2_000_000_000,
            // setup 未给 inst/hot 现金，按其单量（20 万股/10 万股）给足资金。
            inst_cash_cents: 500_000_000_000,
            hot_cash_cents: 100_000_000_000,
            retail_params: (0.3, 300, 0.4),
            inst_params: (0.02, 200_000),
            hot_params: (20, 0.03, 100_000),
            ticks: 3_000,
            order_ttl_ticks: 5,
            float_split: (0.45, 0.53, 0.02),
        }
    }

    /// 小世界 fixture（集成测试用）：2 股、每股 40 retail + 1 inst、hot 2、60 tick。
    pub fn small(seed: u64) -> Self {
        let table = s5_stock_table();
        ProtoConfig {
            seed,
            stocks: table.into_iter().take(2).collect(),
            retail_per_stock: 40,
            inst_per_stock: 1,
            hot_total: 2,
            retail_cash_cents: 2_000_000_000,
            inst_cash_cents: 500_000_000_000,
            hot_cash_cents: 100_000_000_000,
            retail_params: (0.8, 300, 0.5),
            inst_params: (0.02, 20_000),
            hot_params: (2, 0.02, 10_000),
            ticks: 60,
            order_ttl_ticks: 3,
            float_split: (0.45, 0.53, 0.02),
        }
    }

    /// 按总数构造（CLI 用）：retail = total - inst_total - hot_total。
    pub fn with_npc_total(mut self, total: u32) -> Result<Self, ProtoError> {
        let inst_total = self.inst_per_stock * self.stocks.len() as u32;
        let hot_total = self.hot_total;
        if total < inst_total + hot_total {
            return Err(ProtoError::InvalidConfig(format!(
                "npc 总数 {total} 小于 inst({inst_total})+hot({hot_total})"
            )));
        }
        let retail_total = total - inst_total - hot_total;
        self.retail_per_stock = retail_total / self.stocks.len() as u32;
        Ok(self)
    }

    pub fn with_ticks(mut self, ticks: u64) -> Self {
        self.ticks = ticks;
        self
    }

    /// 初始总现金（分）：全部 NPC 的现金之和。
    pub fn initial_total_cash_cents(&self) -> i64 {
        let stocks = self.stocks.len() as i64;
        stocks
            * (self.retail_per_stock as i64 * self.retail_cash_cents
                + self.inst_per_stock as i64 * self.inst_cash_cents)
            + self.hot_total as i64 * self.hot_cash_cents
    }
}

/// 构造一股的 NPC 名册与流通股切分。
///
/// 返回 (npcs, float_remainder)。切分按 float_split 比例 × 流通股数，
/// 每人按 100 股（lot）向下取整，零头进 remainder 桶（参与守恒断言）。
/// remainder = float_shares − Σ实际分配（避免比例和的 f64 误差导致下溢）。
pub fn build_roster(
    cfg: &ProtoConfig,
    stock_idx: usize,
    spec: &StockSpec,
    id_base: u32,
) -> Result<(Vec<Npc>, u64), ProtoError> {
    let mut npcs: Vec<Npc> = Vec::new();
    let mut allocated: u64 = 0;
    let push = |npc: Npc, npcs: &mut Vec<Npc>, allocated: &mut u64| {
        *allocated += npc.qty as u64;
        npcs.push(npc);
    };

    // retail
    let retail_pool = (spec.float_shares as f64 * cfg.float_split.0) as u64;
    let per_retail = align_lot(retail_pool / cfg.retail_per_stock as u64);
    for i in 0..cfg.retail_per_stock {
        let npc = Npc::new(
            id_base + i,
            StrategyData::retail(
                cfg.retail_params.0,
                cfg.retail_params.1,
                cfg.retail_params.2,
            ),
            cfg.retail_cash_cents,
            per_retail,
            spec.initial_price_cents,
        );
        push(npc, &mut npcs, &mut allocated);
    }
    // inst
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
        push(npc, &mut npcs, &mut allocated);
    }
    // hot：轮流放前 hot_total 只股票（本股分到时 hot_here=1）。
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
        push(npc, &mut npcs, &mut allocated);
    }
    let remainder = spec
        .float_shares
        .checked_sub(allocated)
        .ok_or_else(|| ProtoError::InvalidConfig(format!(
            "股票 {} 切分溢出：float={} allocated={}",
            spec.code, spec.float_shares, allocated
        )))?;
    Ok((npcs, remainder))
}

/// 对齐到 100 股一手。
fn align_lot(q: u64) -> u32 {
    (q / 100 * 100) as u32
}
