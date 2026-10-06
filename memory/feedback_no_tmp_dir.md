---
name: no-tmp-dir
description: 产物/工件不要放 /tmp，统一放工作区的 .tmp/ 目录（perf 主线在 .worktree/engine-max-performance/.tmp/）
metadata:
  node_type: memory
  type: feedback
  originSessionId: f28cd04a-805e-42cf-b943-c0b6a1ae0d82
  modified: 2026-10-06T09:57:45.344Z
---

Agent 产生的临时产物、诊断输出、复制的 binary 等一律放工作区 `.tmp/` 目录，不用系统 `/tmp`。

**Why:** 用户 2026-10-06 明确要求"别用/tmp，用工作区的.tmp/目录"。/tmp 重启即失、散落难追溯；工作区 .tmp/ 被 gitignore 但随工作区持久，且与既有 perf 工件（冻结 binary、日志）集中一处。

**How to apply:** 所有 subagent brief 中涉及产物路径的写 `.worktree/engine-max-performance/.tmp/<子目录>/`（或当前活跃工作区的 .tmp/）；运行 cwd 需要隔离时也用它。相关：[[engine-perf-1000x]]
