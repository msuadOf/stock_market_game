---
name: npc-grouping-relaxed-latency
description: 用户备选方案（2026-10-14）：①NPC 按分组/分类开独立决策线程 ②NPC 决策到执行允许延迟 1~2 tick——在基础 std-thread 迁移后如果性能衰退再做实验
metadata:
  node_type: memory
  type: project
  originSessionId: f28cd04a-805e-42cf-b943-c0b6a1ae0d82
  modified: 2026-10-14T12:45:51.070Z
---

用户提出的两个备选优化（2026-10-14，在 std-thread 基础迁移之后视性能再做实验）：

**① NPC 决策分组线程**：
- NPC 按分类/分组开独立线程做批量决策（与股票线程分开）
- 解决 NPC 跨股持仓的所有权问题（决策在自己的线程，不占用股票线程）

**② 放松决策→执行延迟**：
- NPC 从做决策到决策实际落实（下单执行）可以差 1~2 个 tick
- 降低对时序和同步的要求——tick 推进更快，不用同步/阻塞等决策完成
- 语义放松需实验验证（性能衰退大时才启用）

**Why:** 基础 std-thread 迁移（一股一持久线程+内联决策）可能已够快（原型 982 t/s）；这两个方案是后备，在发现性能问题时再试。

**How to apply:**
1. 先完成基础 std-thread 迁移
2. 如果性能好（≥原型水平）→ 不需要这两个方案
3. 如果性能衰退 → 先试①（NPC 分组线程，解决跨股+增加决策并行度）
4. 如果①不够 → 再试②（放松延迟 1~2 tick，进一步解耦）
5. ②涉及语义变化，需 Sol 审查是否影响 A 股交易语义（决策时点的市场信息可能滞后 1~2 tick）
6. 关联：[[std-thread-migration]]、[[resource-entity-model]]
