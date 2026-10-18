# 106 失败域迁移盘点（翻转批实测 ENGINE_STEP_RUNTIME=1）

日期：2026-10-15。来源：`ENGINE_STEP_RUNTIME=1 cargo test -p engine --lib`
全量跑（清单 `.tmp/flip-106-failures.txt`）。只盘点不实现。

## 总览

| 域 | 失败数 | runtime 侧需补的表面 | 批建议 |
|---|---|---|---|
| 决策链（decision_chain） | 28 | 见 §1 | 批 D1（最大） |
| 撮合/tick 生产契约（t3/t4/spine_v0） | 16 | 见 §2 | 批 D2 |
| candidate/committable（authoritative_tick/committable） | 20 | 见 §3 | 批 D3 |
| 观测/绑定（executor_perturbation/phase_timing） | 12 | 见 §4 | 批 D4 |
| 存档/恢复（saved_runtime/history_reads） | 8 | 见 §5 | 批 D5 |
| envelope/回执流（envelope/receipt/ingress） | 5 | 见 §6 | 并入 D3 |
| 计划链（plan_owner/plan_root） | 5 | 见 §7 | 并入 D1 |
| attention | 3 | 见 §8 | 并入 D2 |
| 生命周期/订单（lifecycle/t4 candle） | 3 | 并入 D2 | D2 |
| civil/日界 | 3 | 并入 D5 | D5 |
| quote_expiry | 2 | 并入 D2 | D2 |
| settlement | 1 | 并入 D3 | D3 |
| hash/poison 契约 | 1+3 | 并入 D4 | D4 |

## 逐域分析

### §1 决策链（28 项，批 D1——最大块）
失败形态：计划事实跨决策链运行、机构边界（冻结现金不再复用）、计划
续行重建。**根因**：旧管线的 decision_chain 每步推进 plan 状态机
（plans 域状态+计划根/续行收据）；runtime 的计划链（plans.rs PlanChain
注入式）只承载注入计划，不承载旧管线的 strategy plans 状态机。
**需补表面**：
- 组线程内 strategy plans 状态机引擎化（beliefs/plans/plan_roots/
  plan_continuations 状态从会话迁组线程或经事件投影回写会话）
- 决策链测试锚定的 `retail_traces/decision diagnostics` 产出面
- 机构边界语义（冻结现金不复用=严格所有权已天然对齐——测试口径
  需经 runtime 路径重述）
批建议：D1 分 3 子批（plans 状态机迁移/诊断面迁移/测试重述）。

### §2 tick 生产契约 t3/t4/spine_v0（16 项，批 D2）
失败形态：每 tick 恰一批生产、root 交接先于首交付、owner 市场数据
patch 安装、分钟历史前缀。**根因**：这些测试锚定旧管线的"生产/
消费恰一次"时序契约与 owner 安装面；runtime 的收束语义等价但**事件
批组装面未接**（EventFact 收束点安装进会话未做——翻转缺口的
candidate 域一部分）。
**需补表面**：收束点 EventFact 批+公共视图安装进会话（owner patch）、
retail traces 生产/清理时序、到期诊断恰一次。
批建议：D2 一批（多为同一安装面的不同断言）。

### §3 candidate/committable（20 项，批 D3——翻转核心缺口）
失败形态：shadow 克隆/commit 单点/失败不污染权威/预算原子性。
**根因**：**runtime 用 Abort/回滚替代 candidate 克隆**——语义等价但
测试锚定克隆机制本身（克隆计数/释放恰一次）。envelope/receipt/
settlement 域（5+1 项）同根因。
**需补表面**：不需要复刻克隆——需要把这些测试的断言翻译为 Abort
等价语义（回滚后状态==tick 起始已由等价测试锚定）+ envelope
ledger 行写入回写（批 8e 已有映射器骨架）。
批建议：D3 分 2 子批（测试语义翻译/envelope 回写接线）。

### §4 观测/绑定（12 项，批 D4）
失败形态：executor perturbation 维度独立、phase timing 隔离、
poison 改 session hash。**根因**：thread-local 绑定经 compute.install
传播——runtime 路径下安装面不同（驱动器线程 vs 池线程）。
**需补表面**：driver.step 内 phase timing/executor 绑定初始化
（with_phase_timing_binding 包裹）+ poison hash 契约经
poison_runtime_step 路径对齐。
批建议：D4 一批。

### §5 存档/恢复（8+3 项，批 D5）
失败形态：跨日存档恢复后的 watchlist/费用审计/认知档案/civil 夹具。
**根因**：批 5 已对齐静态校验面；剩余失败锚定**驱动后状态内容**
（费用审计行/认知档案推进）——依赖 D1/D3 的状态机+回写完成。
批建议：D5 最后做（依赖 D1-D4）。

## 执行顺序建议

D2（安装面，快赢）→ D4（绑定/观测）→ D3（candidate 语义翻译+
envelope）→ D1（决策链状态机，最大）→ D5（存档内容，收尾）。
每批照批 1-5 模式：~10-20 测试、逐字段锚定、失败先判"真实差异 vs
测试框架差异"、真实差异修 runtime 并登记。

## 备注

- 106 项全部属"测试锚定旧管线机制"型（非 runtime 撮合/决策语义错）——
  批 1-5 已验证撮合/事件/存档校验语义等价；失败集中在**编排/状态机/
  观测面**。
- 预估：D1-D5 合计工作量约等于批 1-5 的 2-3 倍（决策链状态机迁移
  是单块最大的）。


---

# 重做盘点（P1 整改后，2026-10-15 晚）

P1-8 前置校验修复改变了失败集合：**消除 7 / 新增 7 / 仍 106**
（总量巧合不变但构成不同——真实语义遗漏的修复暴露了下一层失败）。

## 消除（7 项，P1 修复直接闭合）
- failure_tests::public_step_rejects_and_poisons_a_missing_npc_strategy
  （Sol 进程级对照 case——P1-8 前置校验同判）
- t3_boundary_tests::no_due/tick_with_expiry（P1-8 生产探针对齐）
- t4::finalize_installs_owner_market_data_patch（安装面传播）
- spine_v0::v0_late_round_failure、saved_runtime::restore_keeps_cognitive、
  authoritative_tick::compute_context_shared_sessions（前置校验/安装面间接闭合）

## 新增（7 项，P1 修复暴露的下一层）
- authoritative_tick ×4（phase dispatch/discard/unified closing/evidence）
  ——candidate 投影域（D3 核心缺口，与原失败同域）
- failure_tests::typed_private_plan_failure（poison 传播细节）
- decision_chain::decision_observations_include_lunch（决策观察面，D1）
- executor_perturbation::stock_output_reordering（D4）

## 结论修正
- 原"106 项全部属测试锚定旧管线机制"**不成立**（Sol 正确）：至少
  public_step_rejects（缺策略放行）与 4 项安装/原子性是真实语义遗漏，
  已在 P1 整改批修复。
- 剩余 106 项重分类结论：域分布与原盘点基本一致（决策链 28→29、
  candidate 20→23、t3/t4 契约 16→13、观测 12→13、存档 8→7、其他同）；
  批划分 D2→D4→D3→D1→D5 仍适用，但 D2/D3 内各含若干**真实语义
  遗漏**（非纯测试翻译）——每批必须先跑进程级对照判型。

---

# 归因勘误（Sol 二审·修复集 6，2026-10-15 夜·第 7 任 agent 实测）

**上文"P1 后重做盘点·新增 7 项"的逐域归因不成立（Sol 裁定正确）。**
实测抽检 3 项（ENGINE_STEP_RUNTIME=1 单跑，取 panic 原文）：

| case | 实际失败首因 | 盘点原归因 |
|---|---|---|
| `authoritative_tick_tests::public_step_dispatches_each_market_phase_and_commits_its_evidence` | `opt-in 入口仅支持首 tick 会话…当前 tick=599`（非首 tick 门禁） | "candidate 投影域（D3 核心缺口）" |
| `decision_chain::chain_restructure_tests::decision_observations_include_lunch_without_advancing_market_minutes` | 同上，`当前 tick=8099` | "决策观察面，D1" |
| `executor_perturbation_tests::executor_perturbation_dimensions_are_independent_and_scopes_do_not_leak` | 同上，`当前 tick=1` | "D4 绑定/观测域" |

结论修正：
- P1-8 引入的**非首 tick 门禁**（`step_via_runtime` 每步重建 World）是
  多数 106 项的失败首因——多 tick 测试在第二次 `step()` 即被门禁拒绝，
  **未触达**盘点声称的 candidate/决策链/绑定域。逐域归因在门禁移除
  （常驻驱动改造，即 D 批前置）前**无法成立**，须届时按实际失败面重做。
- "消 7/增 7/总 106"的数字维持（Sol 复核属实）；但"新增 7 项=P1 修复
  暴露的下一层语义缺口"的判断不成立——至少抽检 3 项均先撞门禁。
- D 批划分（D2→D4→D3→D1→D5）在门禁移除并重测前仅为工作假设。
