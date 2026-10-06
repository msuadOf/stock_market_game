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

**How to apply:** 在此项目的多步开发任务中默认采用此分工：主 agent 负责状态核对、任务拆分、subagent briefing、进度汇报；不在主对话里直接跑长开发/基准流程（也避免与 subagent 的独占 benchmark 抢 CPU）。相关：[[engine-perf-1000x]]
