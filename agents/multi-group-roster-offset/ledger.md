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
  ⚠️ **表述限定（Sol 合并门裁定）**：`offset <= account_ids.len()` 是
  **`roster_slice` 这一 helper 的调用前置条件**，**不是整个拓扑的不变量**——
  允许存在**全空组**，其 offset 本就可以超过名册长。当前有效 belief 调用路径
  未发现因此新增 panic，但不得据此断言"越界即不可解释"。
- 4787 作为 AccountBook 键仍显式加偏移（切片不替代键解析）。
- **等价性表述须改（Sol 裁定）**：
  `roster_slice(ids, offset, group_len).get(local) == ids.get(offset + local)`，
  它**不等价于**缺陷版 `ids.get(local)`（后者对尾部空槽仍可能命中组 0）。
  空槽不参与的依据：`world.rs:603` 补 `None`、`group.rs:4858` 跳过无 belief 的槽。

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
| mutation：回退 1565 | 改回裸 `fill.owner.1` | **4 passed（未覆盖）** ⇒ 故补 `fill_plan_progress_resolves_owner_within_group_roster` 直测；补后回退 ⇒ **1 failed（filled=0 静默跳过）** |
| 多拍探针 | 2 组 × 6 拍 | **passed**（4.42s，10s 门内） |
| 全量回归 | `cargo test -p engine --lib -- --test-threads=4` | 复核整改后 **2220 passed / 0 failed / 12 ignored**（整改前 2219） |

### 全量回归的两条假失败（已证伪为负载相关 flake，非本批引入）

| 运行 | 失败用例 | 单独复跑 | 结论 |
|---|---|---|---|
| 第 1 次 | `desk_semantics_tests::r09_partial_fill_cancel_releases_remainder_only` | 5/5 passed（且在未改动 main 上亦 passed） | 单组、`root_projection: None`，不触本批路径 |
| 第 2 次 | `npc_decisions_tests::external_host_consumes_npc_results_on_its_calling_thread` | 3/3 passed | 同上 |

两次失败**用例不同**且各自单独全绿 ⇒ 并行满载下的时序 flake。以
`--test-threads=4` 复跑全量：**2219 passed / 0 failed**。

### 单组不变性（生产缺省 n_groups=1）

`roster_offset==0` 且 `per_group==npcs==account_ids.len()` ⇒
`roster_slice(ids, 0, per_group)` == `&ids[..]`（全量视图），
`roster_chain_params(.., 0, local)` == 原 `.get(local)` ⇒
**单组逐位不变**（生产路径零行为变化，缺陷修复只作用于多组）。

### 独立复核发现（未实施改动的 subagent，已修）

**`roster_slice` 的 `assert!` 会在合法配置上 panic（自引入缺陷）。**

- 原实现 `assert!(roster_offset <= account_ids.len())` 把"尾组偏移越界"
  当作不可解释状态 ⇒ 显式崩溃。
- 但**组数多于名册数**是合法配置：`per_group = npcs.div_ceil(n_groups)`，
  npcs=5 / n_groups=4 ⇒ per_group=2 ⇒ 尾组 offset=6 > 5，该组全是补齐空槽
  （world.rs 明确以 None 补齐支持）。⇒ 崩在组线程内，且崩的是合法世界。
- 已复现：`组名册偏移越界：offset=4 全局名册长=3`。
- **修**：`offset >= len` ⇒ 空段（与修复前 `.get()`→None 语义一致）。
  原 `should_panic` 用例随断言退役，补 `roster_slice_empty_for_group_beyond_
  roster`（offset >/==/远超 三例）与 `roster_slice_starts_at_group_offset_
  not_zero`（起点须为本组偏移——起点退化为 0 正是漏偏移的失效形态）。

复核另确认：A 股语义未触碰（纯坐标管线，规则机器与 helper 体均未改）；
单组不变性成立且 clamp 可证为不可观测；**未发现新的漏站点**（17 处
`account_ids`/`chain_params` 逐一核对，其余 `RootProjection` 字段均为
`StockId` 键或全局标量）。

复核提出的 cosmetic 项（`run_chain_decision` 内 3 处 `roster_slice(...)`
重复表达式提为局部变量）**未做**——相邻且无歧义，避免无谓改动。

### 待办（未修）

- **5380 → 现约 `group.rs:5454`**（`#[cfg(test)]` `MutateBeliefForTest`）：
  `account_ids.iter().position()` 返回**全局**下标却当组内 `local` 用
  （约 5462 索引 `belief_states`）。同类但
  ⚠️ 行号随本批改动移动，Luna 复核指出台账仍标旧号 5380/5388，已更正（2026-10-22）
  仅测试专用通道，不并入本批（避免扩大范围）。多组下该 fixture 通道会取错位。
  **已补警示注释**：当前不显错仅因该消息硬连线到 `group_txs.first()`
  （world.rs）——只在组 0 可达，而组 0 的 `roster_offset==0` 使全局序恰等于
  局部序。原注释把错的坐标当正确描述，易误导后来者放开路由后踩静默错。

## 状态

- [x] 修复面复核（整族 10 点；A2/A3/B 六处为诊断外新增）
- [x] 红测试修复前显红（3 failed，panic 签名与生产一致）
- [x] 修复（A1–A4 + B 六处，`roster_slice` / `roster_chain_params` 收口）
- [x] 1565 影响面确认 —— **同类，同批已修**；A5（5380）测试通道登记待办
- [x] 全量回归（**2220** passed / 0 failed / 12 ignored；两条假失败已证伪为
      flake，各自单独复跑均为 1 passed）
      ⚠️ 早先登记为 2219，Luna 复核指出计数过期，已按当前 tip 更正（2026-10-22）
- [x] 多组 2/3/4 探针恢复（补 B4c 规模敏感性维度）—— 2/3/4 组收集 + 2 组×6 拍全通过
- [x] 独立 subagent 复核 diff —— **approve-with-fixes**；发现的自引入缺陷
      （`roster_slice` assert 在合法配置 panic）已修并补测；无新漏站点

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

## ⚠️ 分支状态说明（2026-10-22，合并前必读）

#117 的 **6 个实现提交现在同时存在于两条分支**：

| 分支 | 形式 |
|---|---|
| `fix/multi-group-roster-offset`（主区） | 原始提交（3056d0d7 / dcf14280 / 06fa5c2d / 1c1c4941 / ab810a2b / 26f357fa） |
| `feat/reintegration-b4c-dayend-chain`（.worktree/reint-b4c） | **cherry-pick 副本**（新 sha：…/0b1e937e） |

cherry-pick 目的：让 B4c 性能探针能在含修复的代码上跑多组维度
（该维度此前被本 P0 阻断）。已验证合并后全量 **2219 passed / 0 failed**，
两线改动面不重叠（B4c=session 日终区，本批=runtime/group.rs）。

**合并时的处理（避免重复/冲突）**：
- 若 B4c 先合入 main ⇒ 实现部分随 B4c 进入 main，`fix/...` 分支届时只需合并
  **文档部分**（`9e000da6`/`937231ff`/本台账等），实现提交应丢弃或让 merge
  自动识别为已存在。
- 若本批先合入 main ⇒ B4c rebase 后其 cherry-pick 副本会与原始提交冲突，
  需在 rebase 时跳过（它们内容相同）。
- 无论如何，**不要两处都合**，否则同一改动进两次。

**仍待整理**：`9e000da6`（计划更正）与 `937231ff`（耦合盘点更正）两个**文档**
提交误落在 `fix/multi-group-roster-offset` 修复分支上，与修复批无关。
计划：cherry-pick 到独立文档分支（建议 `docs/sol-b4d-corrections`）并从修复
分支移除，使修复分支只含修复。

---

# Luna 合并门裁决：#117（2026-10-22）

**「Luna：#117 退回；P0 1 / P1 3 / P2 1」**（原始输出 `.tmp/luna-merge-gate-117-verdict.txt`）

## 与 Sol 的关系

两位复核**卡在同一件事**（P0：缺生产调用层红证），互相印证：
- Sol：「修复实现正确，但已知静默缺陷的调用层覆盖尚未闭合，helper 单测不能替代调用点红证」
- Luna：「全程未见弱化断言，但调用层覆盖是 P0；调用点 mutation 仍可全绿」

## PASS 的项（Luna 独立验证）

- **TDD 红→绿已复现**：在归档的 `3056d0d7` 上，3 项红测因 `roots.rs:276`
  `account ID is absent` 失败；当前 tip 同名 4 项通过（4.24s）。
- **断言纪律 PASS**：全程 diff 未见删除/放宽断言、加 `#[ignore]`、以默认值掩错。
  两处临时回退验证均使对应 helper 测试失败（链参数回退取到 100 而非 200；
  名册切片回退取到 `[7,8,9]` 而非 `[8,9]`）。
- **我改的两份更正文档逐字核过原文与新文，依据成立**：
  - B4c「过渡态不成立」——依据 `force_full_accounts` 只控制读取快照/分片路径
    （`drive.rs:718` 判定、`:1265` 置位）；
  - 「回购批准与注销写专户；配股认购入口只预检并入队、日终才扣款」——
    函数级佐证 `session.rs:3089`（认购入口）/`:3138`（入队）/`:3258`（专户入账）
    /`:3325`（注销核减）。

## 问题项与处置

| 级别 | 问题 | 处置 |
|---|---|---|
| **P0** | 缺静默缺陷的生产调用层红证（现金/持仓/链参数），`write_back=false` 会跳过相关计算，现有"账户不重复"断言不覆盖这些数值 | **阻断合并**；已派 agent 补入口级红证（见下） |
| P1 | 红测基线命令完整运行 31.47s，超普通测试 10s 上限 | 待办：缩小 fixture 或归入长验收分类 |
| P1 | 台账写 2219，实际 tip 为 2220 | **已更正**（本提交） |
| P1 | 分支卫生：19 提交/8 文件（非说明的 14/3），`9e000da6`/`937231ff` 两个文档提交仍在修复分支 | 待办：合并前挪到独立文档分支 |
| P2 | 5380 待办行号随改动移动，现约 `group.rs:5454` | **已更正**（本提交） |

## 备注

Luna 指出：台账的红测与独立复核勾选有历史证据，但**不能替代尚未闭合的合并门**。
⇒ 合并前必须补齐 P0，并重跑一次双人复核。

---

# 阻断项闭合验证（2026-10-22，协调者亲自跑）

Sol 与 Luna 判定的**同一条 P0 阻断项**：「缺静默缺陷的生产调用层红证——
helper 单测不能替代调用点红证，调用点 mutation 仍可全绿」。

## 验证做法

在**生产调用点** `group.rs:2858`（`roster_chain_params` 的实参位置）
把 `self.roster_offset` 改成 `0`（即"传错参数"的典型失效形态），
**不改动任何 helper 自身**，然后跑测试。

## 结果：阻断项闭合 ✅

```
test result: FAILED. 6 passed; 4 failed
    multi_group_account_ids_do_not_collide_across_groups
    multi_group_open_derivation_does_not_panic_on_tail_group
    multi_group_open_derivation_scales_over_group_counts
    multi_group_stays_consistent_over_several_ticks   ← 多拍一致性
```
失败形态与生产一致：
`collect_open_derivations 期望 GroupOpenDerivations，收到 Failed(desk-1, ..)`

⇒ **现有测试确实能捕获"调用点传错参数"**，不是只验 helper 自身。

## 说明

- 验证后已 `git checkout` 还原，工作区干净、0 处 MUTATION 残留。
- 这**不构成**"跳过复核"的理由：Sol/Luna 的其余意见（红测 31.5s 超 10s 上限、
  分支卫生、台账计数）仍需处理后再送审。
- ⚠️ 该命令完整运行 31.50s，**超过普通测试 10 秒上限**（Luna P1 之一）
  ⇒ 合入前需缩小 fixture 或归入长验收分类。

---

# 调用层红证交付与剩余缺口（2026-10-22 12:39）

提交 `96f80511` `test(engine): 补调用层红证——组切分不得改变开仓计划`

## 交付内容

3 个 case：`multi_group_open_plans_match_single_group_at_{4,8,12}_ticks`
（`packages/engine/src/runtime/group_roster_offset_tests.rs:231/238/245`）

**走的生产入口**（不碰 helper）：
`step_frame_equivalent → GroupIn::Start → run_chain_decision(write_back=true) → collect_plans`

判据为**跨组数不变式**：账户分到组 0 还是组 1 是纯调度事实，不该改变它自己的开仓计划；单组
`roster_offset==0` 恒正确 ⇒ 单组指纹即 oracle。指纹含账户/股票/方向/目标量/已成交量/状态
（`open_position_action` 全部输出字段）。

## mutation 结果（分别验证，符合 Sol 要求）

| 回退点 | 结果 |
|---|---|
| `run_chain_decision` 的 `roster_chain_params`（group.rs:2858）→ offset=0 | **红** 3/3 |
| `chain_quote_children` 的 `roster_chain_params`（group.rs:3387）→ offset=0 | **红** 3/3 |
| `roster_slice` 五处（2917/2987/3017/4622/4859）→ offset=0 / 传全局切片 / 恒空段 | **仍绿** ⚠️ |

两处 `roster_chain_params` **分别**回退各自显红 ⇒ 满足 Sol「一处回退不能证明两点都受保护」。

失败形态是 panic：`组 1 belief 位 0 缺链参数投影`（组 1 local0 落到玩家位，链参数为 None）
——是崩溃而非数值静默。

## ⚠️ 剩余缺口（未闭合，如实登记，不得写成"全覆盖"）

`roster_slice` 那族（现金/持仓/在途读面）**没钉住**，三种回退形态都仍绿。
原因：本 fixture 里没有跨拍在途票 ⇒ `cash/held_..._with_pending` 的加回量恒为 0
⇒ 该族失效在计划指纹上不可观测。
闭合需另立一条：构造跨拍持有在途票的世界（组间现金/在途互异，加回量非 0）。

## 回归

`cargo test -p engine --lib`（128 核并行）**2223 passed / 0 failed / 12 ignored**，98.58s。
单 case 最长 ~3.2s，满足 10s 硬限（此前的 31.5s 超时问题已随本批解决）。

工作区干净，无 mutation 残留；只改测试文件一个，显式 pathspec 提交。
