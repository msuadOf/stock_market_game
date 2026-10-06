---
name: no-determinism
description: 引擎并发运行不需要确定性/可复现性——不为确定性付性能开销；16 线程下两次运行结果不完全一致不算 bug
metadata:
  node_type: memory
  type: feedback
  originSessionId: f28cd04a-805e-42cf-b943-c0b6a1ae0d82
  modified: 2026-10-06T11:32:09.189Z
---

项目大量使用并发，用户明确表示对确定性和可复现性没有要求；强制确定性会带来额外并发开销，不值得。

**Why:** 用户 2026-10-06 明确表态。此前探索波发现引擎在 16 线程下同 seed 多次运行 events 数/状态 hash/live_envelopes 不完全一致（单线程完全一致），一度当作正确性问题登记——按用户定位这不是问题，不排查、不修复、不再当风险提。

**How to apply:** 性能实验的等价性验证不能用"16 线程下两次运行结果逐字节一致"当判据（天然不成立）；用单线程对照或同进程对比。不要提议确定性改造（如顺序归并、固定调度）当性能优化项。相关：[[engine-perf-1000x]]
