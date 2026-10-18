//! NPC 账户（worker 私有，单线程变更，无锁）。
//!
//! 账面单位与 engine Money 一致（i64 分）。费率的银行家舍入借用
//! `Money::apply_rate`，其余加减乘用 checked 整数运算显式上抛（铁律二）。
//!
//! 跨股臂扩展：账户仍归「主交互股」worker 私有，但可以持有其它股票的仓位
//! （`secondaries`）。主股仓位继续用扁平字段（与按股私有臂完全同构，
//! 撮合直写）；次级仓位只被「回执相位」单线程变更（外股成交回写）。

use engine::money::Money;
use engine::strategy::StrategyData;

/// 跨股次级持仓条目（家端账户内、非主交互股）。
#[derive(Clone, Debug)]
pub struct XPos {
    /// 目标股索引（股票表下标）。
    pub stock_idx: u8,
    pub qty: u32,
    pub sellable: u32,
    pub reserved_shares: u32,
    pub frozen_today: u32,
    pub cost_price_cents: i64,
}

/// 一个 NPC 的全部状态。挂在哪个 StockWorker 名下就归谁所有（主交互股 = 归属股）。
#[derive(Clone, Debug)]
pub struct Npc {
    /// 全局唯一 id（决策种子 seed ^ tick ^ id，对齐引擎 decide_all 约定）。
    pub id: u32,
    /// 策略参数（engine 真实数据结构）。
    pub strategy: StrategyData,
    /// 现金（分）。真实余额；`reserved_cash` 是其中被挂买单占用的部分
    /// （跨股臂：本股买单与外股路由买单的 escrow 共用这一个账户级字段）。
    pub cash: i64,
    /// 挂买单冻结的现金（分）。不变量：== Σ本 NPC 全部活跃买挂单 reserved_remaining
    /// （跨股臂含外股簿上的挂单）。
    pub reserved_cash: i64,
    /// 主交互股总持仓（股）。
    pub qty: u32,
    /// 主交互股可卖（T+1 已解冻且未挂卖单）。
    pub sellable: u32,
    /// 主交互股挂卖单占用。
    pub reserved_shares: u32,
    /// 主交互股当日买入冻结（tick 末解冻进 sellable）。
    pub frozen_today: u32,
    /// 成本价（分）。初始按初始价，后续成交不重算（原型简化，登记于 design.md）。
    pub cost_price_cents: i64,
    /// 跨股次级持仓（按股私有臂恒为空 Vec，零分配零成本）。
    pub secondaries: Vec<XPos>,
}

impl Npc {
    pub fn new(
        id: u32,
        strategy: StrategyData,
        cash: i64,
        initial_qty: u32,
        cost_price_cents: i64,
    ) -> Self {
        Npc {
            id,
            strategy,
            cash,
            reserved_cash: 0,
            qty: initial_qty,
            sellable: initial_qty,
            reserved_shares: 0,
            frozen_today: 0,
            cost_price_cents,
            secondaries: Vec::new(),
        }
    }

    /// 外股交易部席位账户（跨股臂）：不是决策主体，只是 escrow 资源实体——
    /// 买单转入 `cash`（= 家端 escrow 总额），卖单转入 `shares`（= 预留股数）。
    /// 字段语义与本地账户同构，撮合代码无差别复用；strategy 永不被读取。
    pub fn desk(cash: i64, shares: u32) -> Self {
        Npc {
            id: u32::MAX,
            strategy: StrategyData::retail(0.0, 0, 0.0),
            cash,
            reserved_cash: 0,
            qty: shares,
            sellable: shares,
            reserved_shares: 0,
            frozen_today: 0,
            cost_price_cents: 0,
            secondaries: Vec::new(),
        }
    }

    /// 决策可用现金（扣除挂单冻结），对齐 engine SelfView.cash 语义。
    /// 跨股臂：账户级现金，本股与外股 escrow 共同扣减（跨股账户语义）。
    pub fn available_cash(&self) -> i64 {
        self.cash - self.reserved_cash
    }

    /// T+1 解冻：当日买入转入可卖。每 tick 末由 worker 调用（主股字段）。
    pub fn unfreeze_t1(&mut self) {
        self.sellable += self.frozen_today;
        self.frozen_today = 0;
    }

    /// T+1 解冻（跨股臂）：主股 + 全部次级仓。
    pub fn unfreeze_t1_all(&mut self) {
        self.unfreeze_t1();
        for pos in &mut self.secondaries {
            pos.sellable += pos.frozen_today;
            pos.frozen_today = 0;
        }
    }

    /// 结构不变量：qty == sellable + reserved_shares + frozen_today（主股 + 次级）。
    pub fn x_t1_ok(&self) -> bool {
        self.t1_ok()
            && self
                .secondaries
                .iter()
                .all(|p| p.qty == p.sellable + p.reserved_shares + p.frozen_today)
    }

    /// 主股结构不变量（按股私有臂口径，保持不变）。
    pub fn t1_ok(&self) -> bool {
        self.qty == self.sellable + self.reserved_shares + self.frozen_today
    }
}

/// 成交费用（分）：佣金 = max(最低佣金, 成交额×佣金率)，卖方另加印花税。
/// 与 engine GameConfig 同参；银行家舍入用 Money::apply_rate。
pub struct FeeTable {
    pub commission_rate: f64,
    pub commission_min: i64,
    pub stamp_tax_rate: f64,
}

impl FeeTable {
    pub fn from_config(config: &engine::config::GameConfig) -> Self {
        FeeTable {
            commission_rate: config.commission_rate,
            commission_min: config.commission_min.cents(),
            stamp_tax_rate: config.stamp_tax_rate,
        }
    }

    fn apply_rate(&self, value_cents: i64, rate: f64) -> Result<i64, crate::ProtoError> {
        Money::from_cents(value_cents)
            .apply_rate(rate)
            .map(|m| m.cents())
            .map_err(|e| crate::ProtoError::Money(e.to_string()))
    }

    /// 佣金（买/卖共用）。value = 成交额（分）。
    pub fn commission(&self, value_cents: i64) -> Result<i64, crate::ProtoError> {
        let rate_part = self.apply_rate(value_cents, self.commission_rate)?;
        Ok(rate_part.max(self.commission_min))
    }

    /// 印花税（仅卖）。
    pub fn stamp_tax(&self, value_cents: i64) -> Result<i64, crate::ProtoError> {
        self.apply_rate(value_cents, self.stamp_tax_rate)
    }
}
