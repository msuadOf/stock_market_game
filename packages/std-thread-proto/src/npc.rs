//! NPC 账户（worker 私有，单线程变更，无锁）。
//!
//! 账面单位与 engine Money 一致（i64 分）。费率的银行家舍入借用
//! `Money::apply_rate`，其余加减乘用 checked 整数运算显式上抛（铁律二）。

use engine::money::Money;
use engine::strategy::StrategyData;

/// 一个 NPC 的全部状态。挂在哪个 StockWorker 名下就归谁所有。
#[derive(Clone, Debug)]
pub struct Npc {
    /// 全局唯一 id（决策种子 seed ^ tick ^ id，对齐引擎 decide_all 约定）。
    pub id: u32,
    /// 策略参数（engine 真实数据结构）。
    pub strategy: StrategyData,
    /// 现金（分）。真实余额；`reserved_cash` 是其中被挂买单占用的部分。
    pub cash: i64,
    /// 挂买单冻结的现金（分）。不变量：== Σ本 NPC 活跃买挂单 reserved_remaining。
    pub reserved_cash: i64,
    /// 总持仓（股）。
    pub qty: u32,
    /// 可卖（T+1 已解冻且未挂卖单）。
    pub sellable: u32,
    /// 挂卖单占用。
    pub reserved_shares: u32,
    /// 当日买入冻结（tick 末解冻进 sellable）。
    pub frozen_today: u32,
    /// 成本价（分）。初始按初始价，后续成交不重算（原型简化，登记于 design.md）。
    pub cost_price_cents: i64,
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
        }
    }

    /// 决策可用现金（扣除挂单冻结），对齐 engine SelfView.cash 语义。
    pub fn available_cash(&self) -> i64 {
        self.cash - self.reserved_cash
    }

    /// T+1 解冻：当日买入转入可卖。每 tick 末由 worker 调用。
    pub fn unfreeze_t1(&mut self) {
        self.sellable += self.frozen_today;
        self.frozen_today = 0;
    }

    /// 结构不变量：qty == sellable + reserved_shares + frozen_today。
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
