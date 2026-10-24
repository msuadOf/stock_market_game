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

**How to apply:** 当前协作与模型规则统一见仓库根 [ROLE.md](../ROLE.md)，按用户 2026-10-25 的更新执行。2026-10-07 的二级派生长期授权仍保留；只使用工具实际支持的 agent 类型，运行环境不允许时由主会话补派，不假称权限或绕过工具限制。分片须明确文件范围，统一编译验收，独立复核由未实施者执行；其他工作区的在途修改不可触碰。

**历史注记（不作为当前路由）:** 用户于 2026-10-08 曾选择一次性原生 Haiku 做低价值探索与机械工作、不调用 Codex Luna，架构与否定复核使用 Astra；该旧模型路由已由上述当前规范取代。相关：[[subagent-model-routing]] [[engine-perf-1000x]] [[worktree-exclusivity]]
