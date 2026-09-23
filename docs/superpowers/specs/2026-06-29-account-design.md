# 设计：engine `account` 模块 —— 统一账户（现金 + 持仓 + 可选策略）

- **日期 (Date):** 2026-06-29
- **状态 (Status):** 已批准（设计稿，待 TDD 实现）
- **决策者 (Deciders):** msuad + Claude
- **关联 (Related):** [ADR-0005](../../decisions/0005-unified-engine-three-deployments.md)（统一账户模型）、[ADR-0006](../../decisions/0006-npc-strategy-module.md)（策略 trait）、[`money` 设计](2026-06-29-money-fixed-point-design.md)、[`orderbook` 设计](2026-06-29-orderbook-design.md)、[`GameConfig` 设计](2026-06-29-gameconfig-design.md)。

---

## 1. 背景与动机

ADR-0005 §2 定调「统一账户模型」：NPC（散户/机构/游资）与玩家是**同一种 `Account` 对象**，区别只在「下单策略」（NPC=算法、玩家=UI/None）。`money`（钱）+ `orderbook`（撮合）+ `config`（费用）已就位，`account` 是把它们串起来的**账务执行单元**：接收成交（Trade）→ 扣现金/加仓位 → 算费用 → 维护持仓与成本价。

本模块只做「单账户的账务」：现金、持仓、成本价、T+1 锁定、资金/持仓校验。**不**关心：撮合本身（orderbook）、行情驱动（market）、NPC 决策（strategy，仅持有 trait 对象引用）。解耦后可独立单测。

## 2. 核心决策（含 msuad 2026-06-29 拍板的账务规则）

1. **`Account` = `cash: Money` + `positions: BTreeMap<StockCode, Position>` + `strategy: Option<Box<dyn Strategy>>` + `id`/`kind`。** 玩家 `strategy = None`（UI 动作直接产 Intent，不经策略）。
2. **资金不足拒绝下单**（msuad 定）：买入若 `成交额 + 佣金 > cash` → `Err(InsufficientCash)`，不产生成交、不透支。
3. **持仓不足拒绝超卖**（msuad 定）：卖出若 `qty > 可卖持仓（持仓 − T+1锁定）` → `Err(InsufficientShares)`，不裸空。
4. **成本价 = 净投入 / 持仓**（msuad 定，最自洽模型）：
   - 权威状态存两个 i64 累加器：`invested_cents`（总买入成交额）、`recovered_cents`（总卖出成交额），**全是整数分、无 f64、无精度损失**。
   - `cost_price`（派生只读）= `(invested − recovered) / qty`，按需计算并用**银行家舍入到分**（复用 money 约定）。
   - **卖出收回超过投入 → 分子为负 → 成本价为负**（允许，反映已实现盈利超过成本）。
   - 成本价的**显示规则**（显示层，非本模块职责，但契约需明示）：成本为负时显示 `-` 而非 `xx%`，但**成本仍按公式更新**。
5. **费用经 `GameConfig` 计算**：买入佣金 = `config.commission(成交额)`；卖出佣金同 + 印花税 = `config.stamp_tax(成交额)`（已构建）。费用计入「现金」扣减，**不计入成本价**（成本价只跟成交额）。
6. **T+0/T+1 可配**（`GameConfig` 开关）：买入的股数进 `t1_locked`，日终清算时若配置为 T+1 则解锁（转入可卖）；T+0 则买入即可卖（`t1_locked` 始终 0）。

## 3. 类型与 API

```rust
/// 账户种类（NPC 三类 + 玩家）。
#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub enum AccountKind { Retail, Inst, Hot, Player }

/// 单只股票的持仓。权威状态全整数分，无 f64。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Position {
    pub qty: u32,            // 可卖股数（持仓 - t1_locked 之外的…见下）
    pub t1_locked: u32,      // 当日买入锁定（T+1 日终解锁）
    pub invested_cents: i64, // 总买入成交额（分）
    pub recovered_cents: i64,// 总卖出成交额（分）
}
// 注：qty 含 t1_locked（总持仓）；可卖 = qty - t1_locked。

/// 统一账户。
pub struct Account {
    pub id: AccountId,
    pub kind: AccountKind,
    pub cash: Money,
    pub positions: BTreeMap<StockCode, Position>,
    pub strategy: Option<Box<dyn Strategy>>,
}
```

**`AccountId` / `StockCode`**：orderbook 已有 `AccountId(u64)`；本模块复用之。`StockCode` 用轻量 newtype（`pub struct StockCode(pub String)` 或 `u32`）——待 market 模块统一。本模块不引入对 market 的依赖。

**结算方法（消费 orderbook 的 Trade）：**
- `account.apply_trade(&GameConfig, side: Side, trade: &Trade, code: StockCode) -> Result<(), AccountError>` —— 统一入口：按 side 分派买/卖，算费用、校验、更新现金与持仓。
  - 内部：买 → `buy_settle`；卖 → `sell_settle`。
- `account.cost_price(code: &StockCode) -> Option<Money>` —— 派生只读成本价（`(invested−recovered)/qty`，银行家舍入到分）；无持仓返回 None。
- `account.market_value(code, price: Money) -> Option<Money>` —— `price × 可卖持仓`（qty；市值按总持仓算，可另开方法按 qty vs 可卖，初版用 qty）。
- `account.unrealized_pnl(code, price) -> Option<Money>` —— `(price − cost_price) × qty`。
- `account.sellable_qty(code) -> u32` —— `qty − t1_locked`。
- `account.total_assets(&self, prices: &dyn Fn(StockCode)->Option<Money>) -> Result<Money, AccountError>` —— `cash + Σ market_value`（供顶层聚合；本模块提供原子能力，具体 prices 来源由 market 注入）。

> `apply_trade` 需要 `side` 入参（**Trade 本身不带方向**，只有 maker/taker；方向由调用方/account 视角提供：对本账户而言这笔是买还是卖）。

## 4. 结算逻辑（净投入/持仓模型）

`buy_settle(config, trade, code)`：
1. `cost = trade.price × trade.qty`（Money 精确整数乘；`mul_shares`）。
2. `commission = config.commission(cost)?`。
3. `total = cost.add(commission)?`。
4. 若 `total > cash` → `Err(InsufficientCash { needed: total, have: cash })`。
5. `cash = cash.sub(total)?`。
6. 更新 Position：`invested_cents += cost.cents()`；`qty += trade.qty`；`t1_locked += trade.qty`（若 T+1）或 0（若 T+0，可卖立即可用 → t1_locked 不增）。无 Position 则新建。

`sell_settle(config, trade, code)`：
1. 校验：`trade.qty > sellable_qty(code)` → `Err(InsufficientShares { needed, have })`。
2. `proceeds = trade.price × trade.qty`（gross）。
3. `commission = config.commission(proceeds)?`；`stamp = config.stamp_tax(proceeds)?`。
4. `net = proceeds.sub(commission)?.sub(stamp)?`。
5. `cash = cash.add(net)?`。
6. 更新 Position：`recovered_cents += proceeds.cents()`；`qty -= trade.qty`；若 qty==0 → 删除该 Position（invested/recovered 归零）。

> 成本价只跟 `invested/recovered`（成交额），**费用不进成本价**（费用是成本外的现金支出）。

## 5. 错误处理（铁律二）

`AccountError`（thiserror）：
- `InsufficientCash { needed: Money, have: Money }` —— 买入资金不足。
- `InsufficientShares { code: StockCode, needed: u32, have: u32 }` —— 卖出超卖（have = 可卖）。
- `NoPosition { code: StockCode }` —— 卖出/查询时无持仓。
- `MoneyErr(MoneyError)` —— 透传 money 溢出/非法（`#[from]`）。
- `ConfigErr(ConfigError)` —— 透传 config 错误（费用计算）。
**绝不**静默透支、绝不静默超卖、绝不静默吞 money/config 错误。

## 6. 不在本模块内（明确边界）

- 撮合本身 → orderbook（account 只消费其产出的 Trade）。
- 行情驱动 / 隐藏公允价 V / 价格 → market。
- NPC 决策算法 → strategy（account 只持 `Option<Box<dyn Strategy>>` 引用，不实现策略）。
- 费率数值 → GameConfig（account 调用其 commission/stamp_tax）。
- 显示层「成本为负显示 -」→ 前端（account 提供精确 cost_price，显示规则在前端）。

## 7. 测试矩阵（TDD：先红后绿）

`packages/engine/tests/account.rs`：
1. **构造 + 基本访问**：`Account::new(kind, cash)` cash 正确、positions 空、strategy None。
2. **买入结算**（T+0 config）：买 10.00×100 → cash 扣 `1000 + 佣金(5元下限→500分)`=1500 分减；Position.invested=100000 分、qty=100、cost_price=10.00。
3. **买入资金不足**：cash=1000 分，买需 1500 → `Err(InsufficientCash{needed:1500,have:1000})`，且 cash/positions **不变**（回滚、不产生半成交）。
4. **加权买入**（成本价）：先买 10.00×100，再买 12.00×100 → invested=100000+120000=220000 分，qty=200，cost_price=220000/200=1100 分=11.00。
5. **卖出结算 + 印花税**：持 10.00×100，卖 10.00×100 → cash 加 `1000 − 佣金5 − 印花0.5`=994.50 元=99450 分（proceeds=100000 分，net=100000−500−50=99450）；recovered=100000 分；qty=0 → Position 删除。
6. **卖出超卖拒绝**：持可卖 100，卖 200 → `Err(InsufficientShares{needed:200,have:100})`，状态不变。
7. **T+1 锁定**：T+1 config 下买入 → `t1_locked += qty`，`sellable = qty − t1_locked`；买 100 后 sellable=0，卖 1 → `Err(InsufficientShares)`。
8. **成本价为负**（净投入/持仓）：买 10.00×100（invested=100000），卖 10.00×100 价 20.00（recovered=200000）→ invested−recovered=−100000，cost_price 为负（剩余 qty>0 时才有定义；此例需先买更多再部分高价卖以留持仓，构造 invested<recovered 且 qty>0）。
9. **派生计算**：cost_price / market_value / unrealized_pnl / sellable_qty 正确反映状态。
10. **money 错误透传**：极端值触发 money Overflow → `Err(AccountError::MoneyErr(..))`。
11. **serde 往返**：Account/Position/AccountKind 可序列化（存档契约）。
12. **玩家账户 strategy=None**：玩家账户构造后 strategy 为 None。

## 8. 文件布局

```
packages/engine/src/
├── lib.rs            # 追加 `pub mod account;` + re-export
└── account.rs        # Account/Position/AccountKind/AccountError + 结算逻辑
packages/engine/tests/
└── account.rs        # 集成测试
```

> `Strategy` trait 与三策略实现属 `strategy` 模块（ADR-0006），**本模块只定义 `strategy: Option<Box<dyn Strategy>>` 字段并依赖该 trait**。为避免循环依赖与未实现的 trait 阻塞，**本模块先用一个最小 `Strategy` trait 占位定义在 `strategy` 模块**（或 account 内 trait 别名）——实现期若 `strategy` 模块尚未落地，account 持 `Option<Box<dyn Strategy>>` 但暂不实例化任何策略（测试用 None / mock）。**决策：account 不阻塞于 strategy 实现；先落 strategy 的 trait 骨架（trait + Intent + MarketView 的最小定义）作为 account 的依赖前置。** 若范围过大，account 可先不含 strategy 字段、留 TODO 待 strategy 模块补——但 msuad 明确要统一账户，故**含字段、用 trait 占位**。

## 9. 验收标准（Definition of Done）

- `cargo test -p engine` 全绿（money 20 + config 28 + orderbook 15 + account 新增，应 ≥ 75）。
- `cargo clippy -p engine --all-targets -- -D warnings` 零告警。
- `cargo build -p engine` 无 warning。
- 源码审计：现金全程 Money（无 f64）、成本价累加器全 i64 整数分、资金/持仓不足显式拒绝无静默、费用不进成本价、T+1 锁定正确。
- `lib.rs` 导出 account 公共类型。
- Conventional Commits，scope `engine`。

## 10. 实现期风险与边界

- **`Strategy` trait 前置（范围决策）**：account 持 `Box<dyn Strategy>` 需 trait 已定义。**本批次范围 = `account` + `strategy` 模块的 trait 骨架**（`Strategy` trait + `Intent` + `MarketView` 的**类型定义**，不含三策略实现）。三策略（ZI/Value/Momentum）与工厂留作下一批次。理由：account 的「统一账户」语义需要 `Option<Strategy>` 字段，而 trait 骨架很小、无行为，不阻塞 account TDD。
- **成本价派生的舍入（纯整数，绝不 f64）**：`cost_price = round_half_to_even((invested − recovered) / qty)`，单位「分/股」。分子分母皆 i64，**用纯整数实现银行家舍入**，不引入 f64：
  - `n = invested − recovered`（可正可负），`q = qty`（u32，>0）。
  - 取 `floor_div = n.div_euclidean(q)`（向下取整商，对负数也正确），`rem = n.rem_euclidean(q)`（非负余数，`0..q`）。
  - 比较 `2*rem` 与 `q`：`< q` → 取 floor_div；`> q` → floor_div+1；`== q`（恰好半）→ 看 floor_div 奇偶，偶取 floor_div、奇取 floor_div+1（half-to-even）。
  - 负数方向由 `div_euclidean`/`rem_euclidean` 的对称性保证。
  - 测试覆盖：整除、非整除向偶、负分子（成本为负）三种情形。
