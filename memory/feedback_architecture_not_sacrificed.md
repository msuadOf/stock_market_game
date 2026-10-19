---
name: architecture-not-sacrificed
description: 用户原则（2026-10-20）：性能与功能推进不许牺牲正确的代码架构——该有的分层/解耦/单一事实源不能少
metadata:
  node_type: memory
  type: feedback
  originSessionId: f28cd04a-805e-42cf-b943-c0b6a1ae0d82
  modified: 2026-10-19T19:26:26.964Z
---

用户明确指示（2026-10-20）："该有的正确代码架构不能少"——在性能战役与重集成高速推进期，架构正确性是不可让步的约束。

**Why:** 当前处于多 worktree 并行+大批量合并的重集成阶段，最易出现"为了快先凑合"的架构债（如旧耦合模式随退役代码回归、双套逻辑、跳过设计门）。用户在此时点重申，是对速度压力的预防性刹车。

**How to apply:**
1. 重集成批次严格执行既定分批（B4a 纯域落地 ≠ B4b AdminRight 通道接线 ≠ B4c/d corporate_actions 重写走 Sol 设计门）——不为省批次把接线混进域落地
2. 单一事实源原则（如 B2fix 串行路径复用同一套纯函数）保持——发现第二套逻辑即违规
3. 旧代码回归时不许带回旧耦合模式（17 个已灭公司符号挂 session 状态形状的形态不得复生）
4. 性能刀模糊架构边界时（如为省拷贝破坏所有权模型），宁可走设计门重新裁决
5. 相关：[[std-thread-migration]]（架构基线）、[[resource-entity-model]]（所有权模型不可破坏）
