# eager-apply undo 分域实施设计（报批稿）

> 第 19 任起草 2026-10-19（A 归因刚完成，undo 六族/首版日结 Abort 教训
> 上下文最新鲜）。依据：evidence/p2/q2/a-tax-attribution.txt（A 税 −39%
> = 屏障 250μs + 组端构建 200μs，eager 升根治位）+ 批 2 证据中首版
> eager 撤回记录（sol4_day_end_abort 红）。**批准后第 20 任实施。**

## 0. 首版失败的精确定位（本设计的出发点）

首版 eager（批 2，已撤回）：Batch 到达即 apply_staging_batch **全族**——
sol4 红的实测形态：回滚后计划簿残留挂出态（active_child_order_id=
Some/remaining 有值，基线为 None）= **回滚欠冲**。机制：批次的 F5/F6
undo 记录与决策期 F5/F6 记录**交错**——F5 是整态快照恢复、F6 是键控
票据 Option 前值，**两者各自序敏感**（快照恢复会覆盖票据；同键票据
Some/None 序敏感），交错破坏"决策段内原序"。

## 1. 分域原则：按 undo 族的可交换性切

| 族 | 成员 | 回滚算子 | 可交换性 | 域 |
|---|---|---|---|---|
| F1 | Cash/Shares/Frozen | `-= delta` 逆减 | **可交换**（加减法对全应用封闭，与序无关） | **即时域** |
| F2 | CostBasis/PendingSell/BuyPurseHeld | `-= delta`（防御钳位） | **可交换** | **即时域** |
| F3 | ForeignActive | `-= delta` | **可交换** | **即时域** |
| F4 | EverHeld | `= prev` 标量前值 | 序敏感（同 tick 清仓再持有=两次翻转） | **延迟域** |
| F5 | Personal/PlanBook | 整态快照恢复（首变更保存） | 序敏感（快照覆盖一切） | **延迟域** |
| F6 | ChildInFlight | 键控 Option 前值 | 序敏感（同键 Some/None 序） | **延迟域** |
| — | NextDue | `= prev` | 序敏感（非账户域） | 不变（不属批次） |

**数学基础**：即时域三族全部是 delta 逆减——回放终态只依赖"全部
应用"，与记录序无关。因此批次 delta 记录与决策 delta 记录**任意
交错无害**。延迟域三族保持现有"决策段+Seal 段"的段内原序，不与
批次交错。

## 2. 实施方案（group.rs 最小刀界）

### 2.1 apply_staging_batch 拆两相

```
Batch 到达（等待窗）：
  ①即时相 apply_batch_resource_domain(batch)：
    - entities 三变体落账（Cash/Shares 族 undo push——F1/F3）
    - fills 的 apply_fill_cost_basis（F2 undo push）
    - receipts 的 purse 释放（F2 BuyPurseHeld undo push）
    - **顺带把 local 插入 account_dirty_set**（Seal 分片免重扫 undo）
  ②延迟项暂存 staging_deferred：plan_fills 缓冲（现状语义）、
    fills 的 pre_exit_basis 快照、apply_fill_plan_progress/ChildInFlight
    （F5/F6）、EverHeld（F4 序敏感）
Seal 到达：
  ③apply_batch_deferred_domain()：一次性应用全部延迟项（F4/F5/F6
    undo push——段内原序保持）→ 现有 SealAck 路径（分片集合=
    account_dirty_set ∪ 延迟域新增 local）
```

### 2.2 数据结构

- `account_dirty_set: BTreeSet<LocalIdx>`（即时相顺带插——SealAck
  分片直接消费，**消 O(undo_len) 重扫**，归因中组端构建 ~200μs 的
  一半在此+GroupAccount 拷贝另一半）。
- `staging_deferred: Vec<DeferredFill>`（fills 的延迟载荷：
  (stock, fill, pre_exit_basis)）——Undo 日志仍单一日志（不拆两本，
  回滚语义不变），只是**写入时点**分两相。

### 2.3 隔离论证（对首版教训的正面回答）

回滚（现有 rollback 逆序全回放）在新序下：
- 日志 = 决策段（F1-F6 原序）⊕ 交错批次 delta（F1/F2/F3——可交换，
  任意位置等价）⊕ Seal 段延迟域（F4/F5/F6 原序）。
- **F5 快照保存点**：plan_undo_saved 首变更保存——决策触计划簿变更
  先保存（现有）；延迟域批次项在 Seal 才触 PlanBook——此时
  plan_undo_saved 已 true 不再保存 → 回滚恢复到决策前拍前值 ✓（与
  首版的差别：首版批次 F5/F6 即时应用在**部分决策 undo 之前**插入
  了快照保存/F6 前值捕获，序敏感记录交错；分域后这些记录只在 Seal
  段追加，不插入决策段）。
- **Abort 时点**（等待窗中批已部分即时应用）：delta 已入日志 →
  逆序回放全冲回 ✓（可交换性保证部分应用+全回放=拍前态）；
  staging_deferred 清空（rollback 现有清 staging 的等价面扩展）。
- **日结序列**：DayEnd 的 Drain 批次走现有日结 apply（:1903 路径，
  全族应用——日结自身是同步序列不进等待窗，不受分域影响）——
  sol4 场景（日结 Abort）的日志序恢复现有形态 ✓。

### 2.4 锁步与消息面（零协议变化）

S4 不变量不动（Seal 仍在收齐 DeskReport 后广播）；分域只改**组内
处理时序**。Seal 到达时 staging_deferred 必然已含全部延迟项（批次
FIFO 先于 Seal 入队——同一 S4 论证）。

## 3. 行为合同红测试（批准后第一批写）

1. `sol4_day_end_abort_restores_execution_leg_with_tickets`：**首版
   红旗直接复用**（现有测试）——分域后必须绿（判别力已实证）。
2. `eager_partial_apply_abort_rolls_back`：等待窗注入 Abort（组在
   即时相应用 N 批后 Abort）→ tick 起终 business_state_hash 一致
   （delta 可交换性的端到端验证）。
3. `delta_undo_shuffled_replay_equivalence`（数学性质守卫）：同一
   日志的 F1/F2/F3 记录乱序回放 == 原序回放（单元级直击分域基础）。
4. `prereq1_continue_early_round_exactly_once`（现有）+ 存活契约
   （现有 seal_ack_failure 测试）——继续绿。
5. `no_early_reuse_of_tick_resources`（1c）：即时相落账后**本拍无
   后续决策**（Seal=终轮后）——与现状同（分域不改变决策时点）；
   合同断言继续绿。
6. 突变红旋钮：①延迟项即时化（退回首版形态）→ sol4 红（负控证明
   分域是判别面）；②即时相漏 undo push → 合同 2 红。

## 4. 预期数字（构成推算）

- **屏障 250μs → ~0**：组端 drain+即时相与协调者收 DeskReport 的
  等待窗重叠；Seal 到达时只剩延迟相（fills 计划推进的轻量部分）+
  Ack 发送。
- **组端构建 200μs → ~100μs**：dirty set 免重扫（~100μs 侧）+
  GroupAccount 拷贝保留（分片本身仍需 ~500B×活跃集——后续如需再
  优化可走组端账户 COW，登记不阻塞）。
- **A 预期：774 + 250 + 100 ≈ 1120-1180**（对照 preQ2 1218——
  残余 ~50μs=消息+延迟相+Ack）。
- **B（G=1）**：屏障税本就 ~7μs 级——预期持平或微升（dirty set
  的小收益）。
- 风险：即时相与组决策的**同线程竞争**（等待窗=组空闲 recv 时——
  即时相在 recv 循环的消息处理点执行，不与决策（同步段）竞争 ✓。

## 5. 分批提交计划

1. `test(engine)`：合同 1-3 + 突变旋钮（红先行——合同 1 现绿[首版
   已撤]，合同 2/3 编写即红[注入面/单元新建]）。
2. `perf(engine)`：分域实现（合同全绿+突变红①实证）——A/B 净机
   3 轮配对（本设计 §4 对表）。
3. `docs(agents)`：证据+台账（若实测显著低于推算——停下归因，参
   照 A 归因的三臂法）。

## 6. 开放点（实施中定）

- staging_deferred 的内存上界（每 tick 清——Start/Seal 双清点）。
- 即时相 receipts 释放 purse 与 fills 成交的**同批内序**（现有
  apply_staging 的实体/成交序保持——分相后即时相内部按原批内序）。
- B fixture（集合竞价申报窗挂单累积）的即时相负载形态——B 归因
  分段用 STEP_PROFILE 确认 acct_echo/envelope 段不回退。
