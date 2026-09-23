# engine 初始持仓（流通盘分配）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: 用 ultracode workflow 串行 TDD（红→绿→提交），与既有模块同款，一路跑到全绿 + 独立验证门。Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 给 NPC 分配初始持仓（流通盘），修复 session「无成交、市场死」的缺口——让玩家进场时切入一个筹码已被瓜分、能成交的活市场。

**Architecture:** T1 `Account::grant_position` + `StockSpec.float_shares` + `FloatAllocation` 类型。T2 `new()` 内 Random 分配（筹码守恒、确定性、玩家0、成本=开盘价）。T3 ByKind 比例分配。T4 市场转活集成测试（分配后 step 出 Trade）+ clippy/导出/回归。

**Tech Stack:** Rust 2021，`serde`/`thiserror`（workspace 已就位）。测试 `cargo test -p engine`。

## Global Constraints

- **铁律二（防御式）：** `ByKind` 比例非法（负/非有限）→ `InvalidSetup`；分配用 checked 防溢出；**绝不超发**（Σ 持仓 == float_shares 强守恒）；`float_shares==0` 不分配（留给加载存档路径，不报错）。
- **铁律一（TDD）：** 每任务 红→绿→提交。
- **确定性**：分配用 session 种子化 RNG（SplitMix64）→ 同种子同分配。
- f64 仅用于「权重归一/比例」（股数分配，非金额）；最终量 u32。
- 复用：`engine::account::{Account, Position}`（Position{qty,t1_locked,invested_cents,recovered_cents} 全 pub）、`engine::money::Money`、`engine::session` 既有类型。
- 提交信息 Conventional Commits，scope `engine`，末尾 `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`。
- **既有 `sample_setup`（tests/session.rs）须加 `float_shares` + `float_allocation` 字段**；依赖「new() 后 0 持仓」的既有测试，其 sample_setup 的 `float_shares` 设 0（保持原语义）。

## File Structure

```
packages/engine/src/
├── account.rs   # 修改：grant_position 方法
└── session.rs   # 修改：StockSpec.float_shares、FloatAllocation、SessionSetup 字段、seed_float、new 调用
packages/engine/tests/
├── session.rs   # 修改：sample_setup 加字段 + 分配测试
└── account.rs   # 修改：grant_position 测试
```

---

## Task 1: `Account::grant_position` + `StockSpec.float_shares` + `FloatAllocation` 类型

**Files:**
- Modify: `packages/engine/src/account.rs`
- Modify: `packages/engine/src/session.rs`
- Modify: `packages/engine/tests/session.rs`
- Modify: `packages/engine/tests/account.rs`

**Interfaces:**
- Produces: `Account::grant_position(&mut self, code: StockCode, qty: u32, cost_price: Money)`；`StockSpec.float_shares: u32`；`FloatAllocation { Random, ByKind { retail, inst, hot } }`；`SessionSetup.float_allocation: FloatAllocation`。

- [ ] **Step 1: 写失败测试**

`packages/engine/tests/account.rs` 追加：
```rust
#[test]
fn grant_position_sets_cost_basis() {
    use engine::account::StockCode;
    use engine::Money;
    let mut a = engine::Account::new(engine::AccountId(1), engine::AccountKind::Retail, Money::from_cents(1_000_000));
    a.grant_position(StockCode("600101".to_string()), 1000, Money::from_cents(1000));
    let p = a.positions.get(&StockCode("600101".to_string())).unwrap();
    assert_eq!(p.qty, 1000);
    assert_eq!(p.invested_cents, 1_000_000); // 1000 × 1000
    assert_eq!(p.recovered_cents, 0);
    assert_eq!(p.t1_locked, 0);
    assert_eq!(p.cost_price().unwrap().cents(), 1000);
}
```

`packages/engine/tests/session.rs`：把 `sample_setup()` 的 `StockSpec` 加 `float_shares: 0` 字段，`SessionSetup` 加 `float_allocation: engine::FloatAllocation::Random`（**先设 0，保持既有测试「new() 后 0 持仓」语义不变**）。同时顶部 import 补 `use engine::FloatAllocation;` 若用全路径则免。

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p engine`
Expected: 编译错误（`grant_position` 无、`float_shares`/`float_allocation` 字段缺失）（RED）。

- [ ] **Step 3: 写实现**

`packages/engine/src/account.rs` 顶部确认 `use crate::money::Money;`、`StockCode` 已在作用域；`impl Account` 内追加：
```rust
    /// 设一笔持仓：qty 股、成本价 cost_price。
    /// invested = qty × cost_price、recovered = 0、t1_locked = 0（全可卖，历史持仓）。
    /// 通用方法：新游戏随机分配 + 加载存档精确仓位 都走它。
    pub fn grant_position(&mut self, code: StockCode, qty: u32, cost_price: Money) {
        let invested = (qty as i64)
            .checked_mul(cost_price.cents())
            .unwrap_or(i64::MAX);
        self.positions.insert(code, crate::account::Position {
            qty,
            t1_locked: 0,
            invested_cents: invested,
            recovered_cents: 0,
        });
    }
```
> `Position` 在同模块（account），直接 `Position { .. }` 即可（去掉 `crate::account::` 前缀）：用 `Position { qty, t1_locked: 0, invested_cents: invested, recovered_cents: 0 }`。

`packages/engine/src/session.rs`：`StockSpec` 加字段：
```rust
pub struct StockSpec {
    pub code: StockCode,
    pub initial_price: Money,
    pub limit_pct: f64,
    pub v_initial: Money,
    pub tick: Money,
    pub float_shares: u32,
}
```
新增枚举（文件内类型区）：
```rust
/// 流通盘分配方式。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum FloatAllocation {
    /// 默认：所有 NPC 随机分配（权重随机，筹码守恒）。
    Random,
    /// 三类各占比例（类内随机）。比例应 ≈ 1（运行时按「有 NPC 的种类」归一化）。
    ByKind { retail: f64, inst: f64, hot: f64 },
}
```
`SessionSetup` 加字段：
```rust
pub struct SessionSetup {
    // ... 既有字段 ...
    pub float_allocation: FloatAllocation,
}
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test -p engine`
Expected: PASS（grant_position 测试过；既有 session 测试因 sample_setup float_shares=0 仍 0 持仓，语义不变）。

- [ ] **Step 5: 提交**

```bash
git add packages/engine/src/account.rs packages/engine/src/session.rs packages/engine/tests/account.rs packages/engine/tests/session.rs
git commit -m "feat(engine): grant_position + StockSpec.float_shares + FloatAllocation 类型"
```

---

## Task 2: `seed_float` Random 分配（筹码守恒、确定性、玩家0、成本=开盘价）

**Files:**
- Modify: `packages/engine/src/session.rs`
- Modify: `packages/engine/tests/session.rs`

**Interfaces:**
- Produces：`GameSession::seed_float(&mut self)`（私有）；`new()` 在构造账户后、若 `float_shares>0` 且有 NPC 则调用。Random 路径：权重随机归一、最后一个拿余量保 Σ==float。

- [ ] **Step 1: 写失败测试**

`packages/engine/tests/session.rs` 追加（用一个 float>0 的 setup）：
```rust
fn float_setup(float: u32) -> SessionSetup {
    let mut s = sample_setup(); // sample_setup float_shares=0
    s.stocks[0].float_shares = float;
    s
}

#[test]
fn seed_float_random_conserves_chips() {
    let s = GameSession::new(float_setup(1_000_000), 42).unwrap();
    // NPC 持仓之和 == 流通盘
    let total: u32 = s.accounts.values()
        .filter(|a| a.id.0 != 0)
        .map(|a| a.positions.get(&StockCode("600101".to_string())).map(|p| p.qty).unwrap_or(0))
        .sum();
    assert_eq!(total, 1_000_000, "筹码守恒：Σ NPC 持仓 == float_shares");
}

#[test]
fn seed_float_player_has_zero() {
    let s = GameSession::new(float_setup(1_000_000), 42).unwrap();
    let player = s.account(AccountId(0)).unwrap();
    assert!(player.positions.is_empty(), "玩家新进场 0 持仓");
}

#[test]
fn seed_float_deterministic() {
    let a = GameSession::new(float_setup(1_000_000), 42).unwrap();
    let b = GameSession::new(float_setup(1_000_000), 42).unwrap();
    for id in 1..=4 {
        let qa = a.account(AccountId(id)).unwrap().positions.get(&StockCode("600101".to_string())).map(|p| p.qty).unwrap_or(0);
        let qb = b.account(AccountId(id)).unwrap().positions.get(&StockCode("600101".to_string())).map(|p| p.qty).unwrap_or(0);
        assert_eq!(qa, qb, "同种子同分配 (AccountId({}))", id);
    }
}

#[test]
fn seed_float_cost_is_initial_price() {
    let s = GameSession::new(float_setup(1_000_000), 42).unwrap();
    // 任一有持仓的 NPC：cost_price == initial_price(1000分)、t1_locked==0
    let holder = s.accounts.values().find(|a| a.id.0 != 0 && a.positions.contains_key(&StockCode("600101".to_string()))).unwrap();
    let p = holder.positions.get(&StockCode("600101".to_string())).unwrap();
    assert_eq!(p.cost_price().unwrap().cents(), 1000);
    assert_eq!(p.t1_locked, 0);
    assert_eq!(p.invested_cents, (p.qty as i64) * 1000);
}

#[test]
fn seed_float_zero_float_no_allocation() {
    let s = GameSession::new(float_setup(0), 42).unwrap(); // float_shares=0
    let total: u32 = s.accounts.values().map(|a| a.positions.values().map(|p| p.qty).sum::<u32>()).sum();
    assert_eq!(total, 0, "float_shares==0 不分配（兼容加载存档路径）");
}
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p engine`
Expected: `seed_float_*` 测试失败（new 未分配，total=0≠1_000_000）（RED）。

- [ ] **Step 3: 写实现**

`packages/engine/src/session.rs`，`impl GameSession` 内 `populate_npcs` 之后加私有方法 + 在 `new()` 末尾（`Ok(sess)` 前）调用：
```rust
        sess.populate_npcs(AccountKind::Retail);
        sess.populate_npcs(AccountKind::Inst);
        sess.populate_npcs(AccountKind::Hot);
        sess.seed_float();   // 新增：分配流通盘
        Ok(sess)
```
方法实现：
```rust
    /// 分配流通盘给 NPC（按 setup.float_allocation）。float_shares==0 或无 NPC 则跳过。
    /// 筹码守恒：Σ NPC 持仓 == float_shares。玩家不分配（新进场）。
    fn seed_float(&mut self) {
        let npc_ids: Vec<AccountId> = self.accounts.keys().copied().filter(|id| id.0 != 0).collect();
        if npc_ids.is_empty() {
            return;
        }
        for spec in &self.setup.stocks {
            let float = spec.float_shares;
            if float == 0 {
                continue;
            }
            let code = spec.code.clone();
            let price = spec.initial_price;
            let alloc = self.split_random(float, &npc_ids);
            for (id, qty) in alloc {
                if qty > 0 {
                    if let Some(acc) = self.accounts.get_mut(&id) {
                        acc.grant_position(code.clone(), qty, price);
                    }
                }
            }
        }
    }

    /// 把 float 股随机分给 ids（权重随机归一，最后一个拿余量保 Σ==float）。确定性（种子化）。
    fn split_random(&mut self, float: u32, ids: &[AccountId]) -> Vec<(AccountId, u32)> {
        let n = ids.len();
        if n == 0 || float == 0 {
            return ids.iter().map(|id| (*id, 0u32)).collect();
        }
        // 随机权重（+ epsilon 防零）。
        let weights: Vec<f64> = (0..n).map(|_| self.rng.next_f64() + 1e-9).collect();
        let total_w: f64 = weights.iter().sum();
        let mut out: Vec<(AccountId, u32)> = Vec::with_capacity(n);
        let mut remaining = float;
        for i in 0..n {
            let q = if i == n - 1 {
                remaining // 最后一个拿全部余量 → 精确守恒
            } else {
                let raw = (float as f64 * weights[i] / total_w).round() as u32;
                raw.min(remaining)
            };
            out.push((ids[i], q));
            remaining -= q;
        }
        out
    }
```
> `split_random` 借 `&mut self`（用 self.rng）；`seed_float` 内对每只股票调一次。最后 NPC 拿余量保证 Σ==float 精确。

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test -p engine`
Expected: PASS（守恒/玩家0/确定性/成本/zero-float 全过）。

- [ ] **Step 5: 提交**

```bash
git add packages/engine/src/session.rs packages/engine/tests/session.rs
git commit -m "feat(engine): seed_float Random 分配(筹码守恒/确定性/玩家0/成本开盘价)"
```

---

## Task 3: `ByKind` 比例分配 + 非法比例校验

**Files:**
- Modify: `packages/engine/src/session.rs`
- Modify: `packages/engine/tests/session.rs`

**Interfaces:**
- Produces：`seed_float` 按 `FloatAllocation::ByKind` 分（类内随机、缺类分摊、Σ守恒）；`new()` 校验 ByKind 比例非负有限 → `InvalidSetup`。

- [ ] **Step 1: 写失败测试**

`packages/engine/tests/session.rs` 追加：
```rust
#[test]
fn seed_float_bykind_ratios() {
    let mut s = sample_setup();
    s.stocks[0].float_shares = 1_000_000;
    s.float_allocation = engine::FloatAllocation::ByKind { retail: 0.2, inst: 0.5, hot: 0.3 };
    // sample_setup: retail2, inst1, hot1
    let sess = GameSession::new(s, 42).unwrap();
    let retail_total: u32 = sess.accounts.values().filter(|a| a.kind == engine::AccountKind::Retail)
        .map(|a| a.positions.get(&StockCode("600101".to_string())).map(|p| p.qty).unwrap_or(0)).sum();
    let inst_total: u32 = sess.accounts.values().filter(|a| a.kind == engine::AccountKind::Inst)
        .map(|a| a.positions.get(&StockCode("600101".to_string())).map(|p| p.qty).unwrap_or(0)).sum();
    let hot_total: u32 = sess.accounts.values().filter(|a| a.kind == engine::AccountKind::Hot)
        .map(|a| a.positions.get(&StockCode("600101".to_string())).map(|p| p.qty).unwrap_or(0)).sum();
    // 比例 2:5:3，容差（整数取整）±5%
    assert!((retail_total as f64 / 1_000_000.0 - 0.2).abs() < 0.05, "retail≈0.2 got {}", retail_total);
    assert!((inst_total as f64 / 1_000_000.0 - 0.5).abs() < 0.05, "inst≈0.5 got {}", inst_total);
    assert!((hot_total as f64 / 1_000_000.0 - 0.3).abs() < 0.05, "hot≈0.3 got {}", hot_total);
    assert_eq!(retail_total + inst_total + hot_total, 1_000_000); // 守恒
}

#[test]
fn seed_float_bykind_missing_kind_redistributes() {
    let mut s = sample_setup();
    s.stocks[0].float_shares = 1_000_000;
    s.npcs = engine::NpcSetup { retail_count: 0, inst_count: 1, hot_count: 1, cash_per_npc: Money::from_cents(10_000_000) };
    s.float_allocation = engine::FloatAllocation::ByKind { retail: 0.2, inst: 0.5, hot: 0.3 };
    // retail 0 个 → 其 0.2 分摊给 inst/hot（归一化后 inst:0.5/0.8、hot:0.3/0.8）
    let sess = GameSession::new(s, 42).unwrap();
    let total: u32 = sess.accounts.values().filter(|a| a.id.0 != 0)
        .map(|a| a.positions.get(&StockCode("600101".to_string())).map(|p| p.qty).unwrap_or(0)).sum();
    assert_eq!(total, 1_000_000, "缺类仍守恒");
}

#[test]
fn seed_float_bykind_invalid_ratio_rejected() {
    let mut s = sample_setup();
    s.stocks[0].float_shares = 1_000_000;
    s.float_allocation = engine::FloatAllocation::ByKind { retail: -0.1, inst: 0.5, hot: 0.6 };
    assert!(GameSession::new(s, 42).is_err(), "负比例 → InvalidSetup");
}
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p engine`
Expected: ByKind 测试失败（当前 seed_float 只走 Random）（RED）。

- [ ] **Step 3: 写实现**

`packages/engine/src/session.rs`：
- `new()` 开头（既有校验后）加 ByKind 比例校验：
```rust
        if let FloatAllocation::ByKind { retail, inst, hot } = &setup.float_allocation {
            for (v, name) in [(*retail, "retail"), (*inst, "inst"), (*hot, "hot")] {
                if !v.is_finite() || *v < 0.0 {
                    return Err(SessionError::InvalidSetup(format!("float_allocation ByKind {name}={v} invalid (must be finite >=0)")));
                }
            }
        }
```
- `seed_float` 改为按 allocation 分派；新增 `split_by_kind`：
```rust
    fn seed_float(&mut self) {
        let npc_ids: Vec<AccountId> = self.accounts.keys().copied().filter(|id| id.0 != 0).collect();
        if npc_ids.is_empty() { return; }
        for spec in self.setup.stocks.clone() {
            if spec.float_shares == 0 { continue; }
            let code = spec.code.clone();
            let price = spec.initial_price;
            let alloc: Vec<(AccountId, u32)> = match &self.setup.float_allocation {
                FloatAllocation::Random => self.split_random(spec.float_shares, &npc_ids),
                FloatAllocation::ByKind { retail, inst, hot } => {
                    self.split_by_kind(spec.float_shares, *retail, *inst, *hot)
                }
            };
            for (id, qty) in alloc {
                if qty > 0 {
                    if let Some(acc) = self.accounts.get_mut(&id) {
                        acc.grant_position(code.clone(), qty, price);
                    }
                }
            }
        }
    }

    /// 按种类比例分：归一化到「有 NPC 的种类」→ 类内 split_random。Σ==float。
    fn split_by_kind(&mut self, float: u32, r_retail: f64, r_inst: f64, r_hot: f64) -> Vec<(AccountId, u32)> {
        // 收集每类 NPC（升序，确定性）。
        let mut by_kind: [(AccountKind, f64, Vec<AccountId>); 3] = [
            (AccountKind::Retail, r_retail, Vec::new()),
            (AccountKind::Inst, r_inst, Vec::new()),
            (AccountKind::Hot, r_hot, Vec::new()),
        ];
        for id in self.accounts.keys().copied().filter(|id| id.0 != 0) {
            let kind = self.accounts.get(&id).map(|a| a.kind).unwrap_or(AccountKind::Retail);
            for (k, _, v) in by_kind.iter_mut() {
                if *k == kind { v.push(id); break; }
            }
        }
        // 归一化：只算有 NPC 的种类。
        let norm: f64 = by_kind.iter().filter(|(_, _, v)| !v.is_empty()).map(|(_, r, _)| *r).sum();
        let mut out: Vec<(AccountId, u32)> = Vec::new();
        let mut remaining = float;
        let nonempty: Vec<usize> = by_kind.iter().enumerate().filter(|(_, _, v)| !v.is_empty()).map(|(i, _, _)| i).collect();
        for (idx, &i) in nonempty.iter().enumerate() {
            let (kind, ratio, ids) = &by_kind[i];
            let kind_float = if idx == nonempty.len() - 1 {
                remaining // 最后一类拿余量 → 守恒
            } else if norm > 0.0 {
                ((float as f64 * *ratio / norm).round() as u32).min(remaining)
            } else { 0 };
            let parts = self.split_random(kind_float, ids);
            for (id, q) in parts { out.push((id, q)); remaining = remaining.saturating_sub(q); }
            let _ = kind;
        }
        out
    }
```
> `split_random` 内最后 NPC 拿余量已在类内守恒；`split_by_kind` 让最后一类拿整体余量 → 全局 Σ==float。`by_kind` 数组用固定 [3] 避免动态分配。

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test -p engine`
Expected: PASS（ByKind 比例/缺类守恒/非法比例 全过）。

- [ ] **Step 5: 提交**

```bash
git add packages/engine/src/session.rs packages/engine/tests/session.rs
git commit -m "feat(engine): seed_float ByKind 比例分配 + 非法比例校验"
```

---

## Task 4: 市场转活集成测试 + clippy 清零 + 全量回归

**Files:**
- Modify: `packages/engine/src/session.rs`（FloatAllocation 导出/lib.rs）
- Modify: `packages/engine/src/lib.rs`（re-export FloatAllocation）
- Modify: `packages/engine/tests/session.rs`

**Interfaces:**
- Produces：`lib.rs` re-export `FloatAllocation`；集成测试验证「分配后市场出 Trade」。

- [ ] **Step 1: 写集成测试**

`packages/engine/tests/session.rs` 追加：
```rust
#[test]
fn allocated_market_produces_trades() {
    // 分配流通盘后，NPC 有持仓可卖 → 卖盘有货 → 跑若干 step 出现成交。
    let mut s = sample_setup();
    s.stocks[0].float_shares = 10_000_000; // 大流通盘，确保 NPC 都有仓
    s.float_allocation = engine::FloatAllocation::ByKind { retail: 0.3, inst: 0.4, hot: 0.3 };
    let mut sess = GameSession::new(s, 42).unwrap();
    let mut any_trade = false;
    for _ in 0..50 {
        for e in sess.step() {
            if matches!(e, engine::Event::Trade { .. }) { any_trade = true; }
        }
    }
    assert!(any_trade, "分配流通盘后市场应出现成交（缺口已修）");
}

#[test]
fn reexport_float_allocation() {
    use engine::FloatAllocation;
    let _: FloatAllocation = FloatAllocation::Random;
    let _: FloatAllocation = FloatAllocation::ByKind { retail: 0.2, inst: 0.5, hot: 0.3 };
}
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p engine`
Expected: `reexport_float_allocation` 编译失败（FloatAllocation 未 re-export）（RED）。`allocated_market_produces_trades` 可能过/不过——若不过说明分配后仍无成交（需排查策略/撮合）。

- [ ] **Step 3: 写实现**

`packages/engine/src/lib.rs` 把 session re-export 行扩展加入 `FloatAllocation`：
```rust
pub use session::{
    GameSession, SplitMix64, Event, Snapshot, SessionError, SessionSetup,
    StockSpec, NpcSetup, RejectionReason, MarketSnap, AccountSnap, PositionSnap,
    FloatAllocation,
};
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test -p engine`
Expected: PASS。**重点确认 `allocated_market_produces_trades` 真过**（50 步内出现 Trade）。若不过：排查——NPC 策略是否产卖单（Value 在 last>target 卖、Momentum 在下跌卖、ZiNoise 随机卖）、撮合是否成。如实报告。

- [ ] **Step 5: clippy 清零**

Run: `cargo clippy -p engine --all-targets -- -D warnings`
Expected: 零告警。若有（`let _ = kind`、needless、数组借用）按建议修，重跑清零。

- [ ] **Step 6: 全量回归**

Run: `cargo test -p engine`
Expected: 全绿（133 + 新增 ≈ 145）。

Run: `cargo build -p engine`
Expected: 无 warning。

- [ ] **Step 7: 提交**

```bash
git add packages/engine/src/lib.rs packages/engine/src/session.rs packages/engine/tests/session.rs
git commit -m "feat(engine): 初始持仓市场转活集成测试 + FloatAllocation 导出 + clippy 清零"
```

---

## Self-Review（plan 作者自检）

**1. Spec 覆盖：** grant_position→T1；StockSpec.float_shares/FloatAllocation/SessionSetup→T1；seed_float Random（守恒/玩家0/确定性/成本/zero-float）→T2；ByKind（比例/缺类/非法）→T3；市场转活 Trade + 导出→T4。spec §7 测试矩阵 10 项全覆盖。

**2. 占位扫描：** 无 TBD/TODO；每步完整代码。Task 1 grant_position 的 `crate::account::Position` 前缀已注明改 `Position`（同模块）。

**3. 类型一致性：** `grant_position(code, qty, cost_price)`、`FloatAllocation{Random,ByKind{retail,inst,hot}}`、`StockSpec.float_shares`、`SessionSetup.float_allocation`、`seed_float/split_random/split_by_kind` 签名跨任务一致。

**已知风险（实现期注意）：**
- **既有 sample_setup 须加 float_shares + float_allocation 字段**（Task 1 Step1），且 float_shares 先设 0 保持既有「0 持仓」测试语义。Task 2+ 用 float_setup(float) 覆盖。
- `allocated_market_produces_trades`（Task 4）：50 步内应出 Trade。若策略参数（sample_setup 的 arrival_rate/margin 等）导致 50 步无成交，调大步数或调参——但优先如实报告。
- `split_by_kind` 的 `by_kind` 固定 [3] 数组 + `let _ = kind` 防 unused；若 clippy 报则清理。
- 筹码守恒靠「最后拿余量」：split_random 最后 NPC、split_by_kind 最后一类。Σ==float 精确。
