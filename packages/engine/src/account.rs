//! 统一账户（ADR-0005 §2）：NPC 与玩家是同一种 Account，区别只在 strategy（NPC=算法、玩家=None）。
//!
//! 设计见 docs/superpowers/specs/2026-06-29-account-design.md。
//! 只做单账户账务：现金、持仓、成本价（净投入/持仓）、T+1 锁定、资金/持仓校验。
//! 不碰撮合(orderbook)/行情(market)/策略实现(strategy，仅持 trait 引用)。

use crate::config::{ConfigError, GameConfig};
use crate::money::{Money, MoneyError};
use crate::orderbook::{AccountId, Side};
use crate::strategy::Strategy;
use std::collections::BTreeMap;
use thiserror::Error;

/// 股票代码占位 newtype（待 market 模块统一；本模块不依赖 market）。
#[derive(
    Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, serde::Deserialize,
)]
pub struct StockCode(pub String);

/// 账户种类：NPC 三类 + 玩家。
#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub enum AccountKind {
    /// 散户 NPC。
    Retail,
    /// 机构 NPC。
    Inst,
    /// 热钱 NPC。
    Hot,
    /// 玩家（真人，strategy=None）。
    Player,
}

/// 账户操作失败。绝不静默吞掉（铁律二），错误携带上下文。
#[derive(Debug, Error)]
pub enum AccountError {
    /// 买入资金不足：成交额+佣金 > 现金。
    #[error("insufficient cash: needed {needed:?}, have {have:?}")]
    InsufficientCash { needed: Money, have: Money },
    /// 卖出超卖：qty > 可卖持仓（持仓 − T+1锁定）。
    #[error("insufficient shares for {code:?}: needed {needed}, sellable {have}")]
    InsufficientShares { code: StockCode, needed: u32, have: u32 },
    /// 卖出/查询时无持仓。
    #[error("no position for {0:?}")]
    NoPosition(StockCode),
    /// 透传 money 错误（溢出/非法）。
    #[error(transparent)]
    MoneyErr(#[from] MoneyError),
    /// 透传 config 错误（费用计算）。
    #[error(transparent)]
    ConfigErr(#[from] ConfigError),
}

/// 统一账户（ADR-0005 §2）：NPC 与玩家同构，区别仅在 `strategy`（NPC=算法、玩家=None）。
///
/// 权威账务状态：现金 `cash`、持仓 `positions`；可选持策略 `strategy`。
/// 持 `Box<dyn Strategy>` → 非 Copy/Clone：账户是有身份的可变状态，按引用传递。
pub struct Account {
    /// 账户唯一 id（来自 orderbook.AccountId，撮合/结算跨模块统一引用）。
    pub id: AccountId,
    /// 账户种类：NPC 三类 + 玩家。
    pub kind: AccountKind,
    /// 现金（分）。全程 Money/i64，绝不存 f64（money 模块铁律）。
    pub cash: Money,
    /// 持仓表：股票代码 → Position。BTreeMap 保有序，便于聚合/快照。
    pub positions: BTreeMap<StockCode, Position>,
    /// 策略：NPC 注入算法，玩家为 None（UI 动作直接产 Intent）。
    pub strategy: Option<Box<dyn Strategy>>,
}

impl Account {
    /// 构造：默认 `strategy=None`（玩家视角）。NPC 用 [`Self::set_strategy`] 注入。
    pub fn new(id: AccountId, kind: AccountKind, cash: Money) -> Self {
        Account {
            id,
            kind,
            cash,
            positions: BTreeMap::new(),
            strategy: None,
        }
    }

    /// 注入策略（NPC 账户）。玩家不调用。
    pub fn set_strategy(&mut self, s: Box<dyn Strategy>) {
        self.strategy = Some(s);
    }

    /// 是否持有策略（NPC=true，玩家=false）。
    pub fn has_strategy(&self) -> bool {
        self.strategy.is_some()
    }

    /// 可卖股数（持仓 − T+1 锁定）；无持仓返回 0。
    pub fn sellable_qty(&self, code: &StockCode) -> u32 {
        self.positions.get(code).map(|p| p.sellable()).unwrap_or(0)
    }

    /// 派生只读成本价（分/股）；无持仓返回 None。
    pub fn cost_price(&self, code: &StockCode) -> Option<Money> {
        self.positions.get(code).and_then(|p| p.cost_price())
    }

    /// 买入结算：扣现金(成交额 + 佣金)、加持仓、累加 invested。
    ///
    /// - `cost = price.mul_shares(qty)`：成交额（分），透传 money 溢出错误（铁律二，绝不静默吞）。
    /// - `commission = config.commission(cost)?`：费用经 [`GameConfig`] 计算（max(费率, 下限)）。
    /// - `total = cost + commission`：现金需支付总额；`total > cash` → [`AccountError::InsufficientCash`]，
    ///   且**不修改任何状态**（无半成交、不透支）。
    /// - 持仓累加：`invested_cents += cost.cents()`（**仅成交额，佣金不计入成本价**，spec §3）、
    ///   `qty += qty`；`t1_enabled=true` 时买入股数进 `t1_locked`（T+1 当日不可卖）。
    ///
    /// 全程 `Money`/i64 整数分，无 f64；费用与成本分离（费用不进 invested）。
    pub fn apply_buy(
        &mut self,
        config: &GameConfig,
        code: StockCode,
        price: Money,
        qty: u32,
        t1_enabled: bool,
    ) -> Result<(), AccountError> {
        let cost = price.mul_shares(qty).map_err(AccountError::from)?;
        let commission = config.commission(cost)?;
        let total = cost.add(commission)?;
        if total > self.cash {
            return Err(AccountError::InsufficientCash {
                needed: total,
                have: self.cash,
            });
        }
        self.cash = self.cash.sub(total)?;
        let pos = self.positions.entry(code).or_insert(Position {
            qty: 0,
            t1_locked: 0,
            invested_cents: 0,
            recovered_cents: 0,
        });
        pos.invested_cents += cost.cents();
        pos.qty += qty;
        if t1_enabled {
            pos.t1_locked += qty;
        }
        Ok(())
    }

    /// 卖出结算：校验可卖、加现金(成交额 − 佣金 − 印花税)、累加 recovered、减持仓（清仓则删除）。
    ///
    /// - `sellable = self.sellable_qty(&code)`：可卖 = 持仓 − T+1 锁定；无持仓返回 0。
    /// - `qty > sellable` → [`AccountError::InsufficientShares`]（含无持仓场景：sellable=0，
    ///   qty>0 必触发 `{have:0}`），且**不修改任何状态**（无半成交、不超卖，铁律二）。
    /// - `proceeds = price.mul_shares(qty)`：成交额（分），透传 money 溢出错误。
    /// - `commission = config.commission(proceeds)?`、`stamp = config.stamp_tax(proceeds)?`：费用经
    ///   [`GameConfig`] 计算（佣金取下限 max，印花无下限）。
    /// - `net = proceeds.sub(commission)?.sub(stamp)?`：净入账（成交额扣两项费用）。
    /// - `cash = cash.add(net)?`：现金按净额增加；recovered 只累加 **proceeds.cents()（成交额，
    ///   费用不进成本，spec §3）**；qty -= qty；qty==0 → 删除持仓（清仓，下次买入新建）。
    ///
    /// 全程 `Money`/i64 整数分，无 f64；费用与成本分离（费用不进 recovered）。
    pub fn apply_sell(
        &mut self,
        config: &GameConfig,
        code: StockCode,
        price: Money,
        qty: u32,
    ) -> Result<(), AccountError> {
        let sellable = self.sellable_qty(&code);
        if qty > sellable {
            return Err(AccountError::InsufficientShares {
                code: code.clone(),
                needed: qty,
                have: sellable,
            });
        }
        let proceeds = price.mul_shares(qty).map_err(AccountError::from)?;
        let commission = config.commission(proceeds)?;
        let stamp = config.stamp_tax(proceeds)?;
        let net = proceeds.sub(commission)?.sub(stamp)?;
        self.cash = self.cash.add(net)?;
        let pos = self
            .positions
            .get_mut(&code)
            .expect("sellable>0 => position exists");
        pos.recovered_cents += proceeds.cents();
        pos.qty -= qty;
        // 清仓：删除持仓（invested/recovered 归零，下次买入新建）。
        if pos.qty == 0 {
            self.positions.remove(&code);
        }
        Ok(())
    }

    /// 统一结算入口：按 `side` 分派买/卖。`t1_enabled` 仅对买入有效。
    pub fn apply_trade(
        &mut self,
        config: &GameConfig,
        side: Side,
        code: StockCode,
        price: Money,
        qty: u32,
        t1_enabled: bool,
    ) -> Result<(), AccountError> {
        match side {
            Side::Buy => self.apply_buy(config, code, price, qty, t1_enabled),
            Side::Sell => self.apply_sell(config, code, price, qty),
        }
    }

    /// 持仓市值 = 现价 × 总持仓(qty)。无持仓返回 None。
    pub fn market_value(&self, code: &StockCode, price: Money) -> Option<Money> {
        self.positions
            .get(code)
            .map(|p| price.mul_shares(p.qty).unwrap_or(Money::ZERO))
    }

    /// 未实现盈亏 = (现价 − 成本价) × 总持仓。无持仓返回 None。
    pub fn unrealized_pnl(&self, code: &StockCode, price: Money) -> Option<Money> {
        let p = self.positions.get(code)?;
        let cost = p.cost_price()?;
        let per_share = price.sub(cost).ok()?;
        Some(per_share.mul_shares(p.qty).unwrap_or(Money::ZERO))
    }
}

/// 单只股票的持仓。权威状态全整数分（invested/recovered 累加器），无 f64。
///
/// 成本价 = (invested − recovered) / qty（净投入/持仓模型，ADR-0005/spec §2）：
/// 卖出收回超过投入时分子为负 → 成本价为负（允许，反映已实现盈利超过成本）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Position {
    /// 总持仓股数（含 t1_locked）。
    pub qty: u32,
    /// 当日买入锁定（T+1 日终解锁；T+0 时始终 0）。
    pub t1_locked: u32,
    /// 总买入成交额（分）。仅成交额，不含费用。
    pub invested_cents: i64,
    /// 总卖出成交额（分）。仅成交额，不含费用。
    pub recovered_cents: i64,
}

impl Position {
    /// 可卖股数 = 总持仓 − T+1 锁定。
    pub fn sellable(&self) -> u32 {
        self.qty - self.t1_locked
    }

    /// 派生只读成本价（分/股）= (invested − recovered) / qty。
    /// qty == 0 → None。用纯整数银行家舍入（half-to-even），绝不引入 f64。
    pub fn cost_price(&self) -> Option<Money> {
        if self.qty == 0 {
            return None;
        }
        let n = self.invested_cents - self.recovered_cents;
        let c = round_half_to_even_i64(n, self.qty);
        Some(Money::from_cents(c))
    }
}

/// 纯整数银行家舍入（round-half-to-even）的 n / d，支持负分子。
/// 用 div_euclidean/rem_euclidean 保证负数对称；半值（2*rem == d）时取偶数商。
fn round_half_to_even_i64(n: i64, d: u32) -> i64 {
    let d = d as i64;
    let floor = n.div_euclid(d); // 向下取整商（负数也正确）
    let rem = n.rem_euclid(d); // 非负余数 [0, d)
    match (2 * rem).cmp(&d) {
        std::cmp::Ordering::Less => floor,
        std::cmp::Ordering::Greater => floor + 1,
        std::cmp::Ordering::Equal => {
            // 恰好半：取偶数。floor 与 floor+1 二选一，谁偶取谁。
            if floor % 2 == 0 {
                floor
            } else {
                floor + 1
            }
        }
    }
}
