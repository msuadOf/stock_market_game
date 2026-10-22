# P0 缺陷：组端坐标 vs 全局名册投影漏加 roster_offset（多组运行时 panic）

> 2026-10-22 发现于 B4c 性能探针准备过程。**main 预存缺陷，非 B4c 引入**。

## 现象

`ENGINE_STEP_RUNTIME=1` 且 `n_groups ≥ 2` 时，尾组线程（实测恒为
`rt-group-15` 等尾部组）在决策链到期拍 panic：

```
thread 'rt-group-N' panicked at packages/engine/src/session/decision_chain/roots.rs:276:55:
account ID is absent
```

单组（生产缺省 `from_session` → `n_groups=1`）永不出事 ⇒ 长期未暴露。

## 根因

`packages/engine/src/runtime/group.rs:4787`（`build_root_read_context`）
用**裸 `local`** 查全局 `projection.account_ids`，正确坐标应为
`self.roster_offset + local`。组 g≥1 把**组 0 的账户 id** 装进
`AccountBook`，而 `run_chain_decision`（正确解析 `roster_offset+local`
的全局 id）拿全局 id 去查 ⇒ `roots.rs:276` 查不到。
组 0 `roster_offset==0` 恒正确，掩盖缺陷。

**坐标不变量（group.rs:791 字段文档）**：`account_ids` 与 `chain_params`
**均为全局名册序**（drive.rs:402 两者同由全局 `ids` 映射构造），组内
`local` 访问须加 `roster_offset`。

## 排除项（诊断实测证伪的猜测）

- **padding 是红鲱鱼**：`groups=2`（20008/2 整除）、`groups=4`（5002）均 panic。
- **触发时点随机但绝对**：取决于尾组链位最早 attention `next_due`
  （session.rs:3295 逐 NPC 采样）；实测阈值与 ticks_per_day 无关。

## 完整修复面（复核扩大：整族 10 个坐标点，非仅 4787）

诊断原登记「4787 + 两处伴生 + 1565 待确认」。二次复核（双路独立扫描：
grep 全仓 + subagent 逐点核验）确认修复面更大，且**两处静默类缺陷诊断未列**。

### A. 直接读全局投影数组

| # | line（修复前） | 坐标 | 后果 | 修复 |
|---|---|---|---|---|
| A1 | 4787 `account_ids.get(local)` | 裸 local | **panic**（根因） | `roster_offset + local` |
| A2 | 2798 `chain_params.get(local)` | 裸 local | **静默**：复审用组 0 链参数 | `+ roster_offset` |
| A3 | 3323 `chain_params.get(local)` | 裸 local | **静默**：报价用组 0 链参数 | `+ roster_offset` |
| A4 | 1565 `account_ids.get(fill.owner.1)` | LocalIdx | **静默**：尾组计划推进跳过 | `+ roster_offset` |
| A5 | 5380 `position()` 当 local 用 | 全局 idx | `#[cfg(test)]` 专用 | **未修**（另立，见下） |

- **A2/A3 未被诊断列出**：`account_ids.get(` 的 grep 结构性看不见（跨行
  `.chain_params\n.get(`）；破坏为数值错而非 panic，比 A1 更隐蔽。
- **1565 判定：同类漏偏移**。`Owner=(GroupIdx, LocalIdx)`（runtime/mod.rs:21）；
  同文件 `apply_fill_cost_basis` 用 `fill.owner.1` 索引 `self.accounts`
  （组内数组）自证其为局部坐标。漏偏移时命中组 0 账户：有票错推进、无票
  则 `return Ok(())` ⇒ **尾组计划推进整段静默跳过**。
- **A5 结论：同类但非生产面**。`#[cfg(test)]` 的 `MutateBeliefForTest`
  通道，取全局 position 当 local 索引组内 `belief_states`。不属 P0，
  **不并入本批**（避免扩大范围至测试专用通道），登记待办。

### B. helper 的 6 个生产调用方（诊断未列）

`cash_with_pending_at`(体 401) / `held_qty_with_pending_at`(体 372) /
`equity_cents_with_pending` 的 `account_ids` 形参语义=**组内局部切片**，
体内 `get(local)` **正确、保持不变**（已确认）。缺陷在**调用方**——
全部传全局 `&projection.account_ids` + 裸 local：

| 调用点（修复前） | 所在函数 | `self` 可得 |
|---|---|---|
| 2856 / 2926 / 2956 | `run_chain_decision` | ✅ |
| 4564 / 4610 | `group_urgency_at_view`（自由函数） | ❌ 需传参 |
| 4799 / 4812 | `build_root_read_context`（自由函数） | ❌ 需传参 |

（三个测试调用方 6384/6435/6454 传 1 元素局部切片，本就正确。）

⇒ **只改 4787 不够**：账户 id 对了但现金/持仓/在途仍读组 0；只改
4787+1565 则链参数仍错。多组可测性要求整族收口。

## 修法（已批准：统一预切片）

新增 `roster_slice(account_ids, roster_offset, group_len)` 单一收口点，
切出本组局部切片传给三个 helper；两个自由函数加 `roster_offset` 形参
（`GroupReviewHost` 经 `self.state.roster_offset` 取得，不加字段）。
- 切片上界**截断**不 panic：尾组常带补齐空槽（`per_group=npcs.div_ceil`
  ，world.rs 尾组 resize 到 per_group ⇒ `offset+len` 可超名册长），
  截断与修复前 `.get()`→None 语义一致；偏移量本身越界才显式失败。
- 4787 作为 AccountBook 键仍显式加偏移（切片不替代键解析）。

## 红测试

`packages/engine/src/runtime/group_roster_offset_tests.rs`（新增，
`mod.rs` 注册；沿用 `bridge_tests::setup_fixture` 会话 fixture——
完整 legal 个人态，observe 链需跑完才显红，不从零造）。

触发面选 `World::collect_open_derivations()`：向**每组**广播
`EchoOpenDerivation`，组内遍历**全部** belief 位直调 `run_chain_decision`
（`write_back=false`）——**不经 `next_due` 采样**，故每拍必触发，稳定复现。

| 测试 | 钉住的形态 |
|---|---|
| `multi_group_open_derivation_does_not_panic_on_tail_group` | A1 显式 panic |
| `multi_group_account_ids_do_not_collide_across_groups` | 静默类（跨组账户不得重复） |
| `multi_group_open_derivation_scales_over_group_counts` | 2/3/4 组规模面 |

红测试 2 是**静默类专门的判别断言**——只验「不 panic」会漏掉 A2/A3/A4。

## 收口点（修复后）

- `roster_slice(account_ids, roster_offset, group_len)` —— 六个
  `cash/held/equity_with_pending` 调用方共用，切出本组段。
- `roster_chain_params(chain_params, roster_offset, local)` —— 两处
  `chain_params` 读取共用（run_chain_decision / chain_quote_children）。

两者与 `account_ids` 的 `roster_offset + local` 直取（4787 的键）共同保证：
**同类漏偏移只能在这三个点犯，且每点都有单测钉住**。

## 验证记录

| 项 | 命令 | 结果 |
|---|---|---|
| 红测试（修复前） | `cargo test -p engine --lib group_roster_offset` @ `3056d0d7` | **3 failed**（`rt-group-1` panic `roots.rs:276 account ID is absent`） |
| 转绿（修复后） | 同上 | **3 passed** |
| 补切片/链参数单测 | `cargo test -p engine --lib roster_offset` | **7 passed** |
| mutation：回退 chain_params 两处 | 改回裸 local | **4 failed**（含 `roster_chain_params_yields_this_groups_params`）——断言非恒真 |
| mutation：回退 B 类切片六处 | 改为全局切片 | **3 passed（未覆盖）** ⇒ 故补 `roster_slice` 单测 |
| 全量回归 | `cargo test -p engine --lib` | 见下 |

### 待办（未修）

- **5380**（`#[cfg(test)]` `MutateBeliefForTest`）：`account_ids.iter().position()`
  返回**全局**下标却当组内 `local` 用（5388 索引 `belief_states`）。同类但
  仅测试专用通道，不并入本批（避免扩大范围）。多组下该 fixture 通道会取错位。

## 状态

- [x] 修复面复核（整族 10 点；A2/A3/B 六处为诊断外新增）
- [x] 红测试修复前显红（3 failed，panic 签名与生产一致）
- [x] 修复（A1–A4 + B 六处，`roster_slice` / `roster_chain_params` 收口）
- [x] 1565 影响面确认 —— **同类，同批已修**；A5（5380）测试通道登记待办
- [ ] 全量回归
- [ ] 多组 2/3/4 探针恢复（补 B4c 规模敏感性维度）
- [ ] 独立 subagent 复核 diff

## 缺陷来历（2026-10-22 追查）

`roster_offset` 字段由 B4c 分支的 **`839a9381`（2026-10-17）** 引入
（"组参数化收尾——尾组零位定长与组内名册偏移"），字段文档
（`group.rs:791`）当时即写明"组内 local 访问需加偏移；单组=0"。

但该提交**只在部分读取点加了偏移**（后续又有若干处补加），全族约 10 个
坐标点未收口 ⇒ 多组路径从未真正可用，而生产缺省单组（`offset=0`）恒正确，
故长期无暴露。

⇒ **本缺陷是"遗留未收口"，不是回归**。对后人的含义：任何新增的
"全局序投影 + 组内 local"访问点，必须在**同一提交内**确认加偏移，
否则只有跑到多组配置才会暴露。

## 修复过程中的一次自引入回退（留作教训）

引入 `roster_chain_params` 收口点时，**函数体漏用 `roster_offset` 形参**
（仍 `chain_params.get(local)`）⇒ 把已修好的偏移又改回裸 local。
World 级红证 3 例**正确报红**（红→绿→红→绿），修正后 7 passed、
全量 2217 passed / 0 failed（基线 2211，+6 为新增测试）。

**教训**：新增收口点后必须重跑**已有**红证，不能只跑新增单测——
新增单测验的是收口点自身语义，验不了"调用点是否真的传对参数"。
