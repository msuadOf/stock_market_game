---
name: collaboration-mode
description: 用户要求的协作分工模式——主 agent 只对话协调，开发工作全部交给 subagent，subagent 允许使用二级 agent
metadata:
  node_type: memory
  type: feedback
  originSessionId: f28cd04a-805e-42cf-b943-c0b6a1ae0d82
  modified: 2026-10-06T06:50:25.500Z
---

主 agent 保留用于和用户对话，具体开发工作用 subagent 执行；subagent 允许再派生二级 agent（例如 CLAUDE.md 要求的"未实施改动的独立复核"正好用二级 fresh agent 做）。

**Why:** 用户在 2026-10-06 性能优化任务中明确提出"你主agent留着和我对话用，你用subagent进行开发，subagent允许使用二级agent"。

**How to apply:** 在此项目的复杂任务中默认采用此分工：主 agent 保留与用户讨论、明确目标和取舍；负责实施的 agent 统一接口与验收，将互不冲突的文件分片或研究问题交给二级 agent。用户于 2026-10-07 再次明确这是后续复杂工作的长期授权，不必逐次询问是否允许二级派生。只用工具实际支持派生的 agent 类型；fork 或运行环境不允许时，由主会话补派，不假称权限已开启，不绕过工具限制。低价值但耗时/耗 token 的探索、定位、盘点和机械工作，用户于 2026-10-08 最终选择默认用一次性原生 Haiku，不调用 Codex Luna；架构与否定复核仍用 Astra，详见 [[subagent-model-routing]]。分片明确文件范围，统一编译验收，独立复核由未实施者执行；其他工作区的在途修改不可触碰。相关：[[engine-perf-1000x]] [[worktree-exclusivity]]
