---
name: engine-perf-1000x
description: rust engine 并行性能优化主线——16核加速比 ~220x 推向 ≥1000x，工作区 .worktree/engine-max-performance，状态台账在 agents/engine-max-performance/current-state.md
metadata:
  node_type: memory
  type: project
  originSessionId: f28cd04a-805e-42cf-b943-c0b6a1ae0d82
  modified: 2026-10-06T11:32:20.107Z
---

主线任务：production Rust server（5 股 / 20,007 NPC / seed 42 / 16 物理核 0-15 / Rayon16 / Tokio2 / 3000 tick）加速比从 ~220x 提到 ≥1000x，上不封顶；更多核带来更多加速也算方向。不许减 NPC、跳 tick、删校验门禁、改 A 股交易语义（撮合/费用/T+1/首错顺序/失败原子性）换速度。达成 1000x 前前端/WASM/浏览器路径全部暂停。进度（2026-10-06）：E2 融合 from_post_expiry ~228x（1550d28d）；E3 capture 双排序融合 ~234x（aa81ead5）；E4 零 op 重验证撤回（+1.17% <2% 门槛，砍 560ms 验证 CPU 只换 1.17%——branch2b wall 长杆由每行物化+业务数学主导，"删冗余"类已到头）。E3 诊断纠正：read-only join 真长杆是 branch2b（≈1454µs/tick）而非 hydration。E4 发现跨会话基准漂移 +3~4%，判定必须同会话相邻配对。E5 NPC 捕获通道合并撤回（+1.66%，机制结论：wall−CPU 差是尾延迟非可调度空闲，"更聪明的调度/合并"类已到头，必须削单条 item 工作量：retail 单账户 ~140µs 分析、npc_queue 串行回调）。探索波（6 方向，产物在 .tmp/perf-expl/）关键结论：①worker 倒 U 峰在 4（诊断 4w 293x vs 16w 213x）——H1 改 RAYON_NUM_THREADS=4 外推 +15~35%，非否决项；②跨 tick 投机预计算三前提实证（2084/2084 等价，外推 +16%）+ commit 旧状态异步 drop（+9%），诊断 patch 在 .tmp/perf-expl/tick-overlap/；③dirty-key splice（写集驱动行级增量物化）外推 +8~14%，须先过 P0 判据（脏键率>10% 即弃）；④allocator 方向排除（jemalloc 默认近最优）；⑤基准漂移 4.89% 峰谷超过 2% 门槛，E3→E4 的 +3.2% 位移是假信号机制，后续门控一律用 AIP-gate v1（anchor 首尾 + H/C 交错 4 对 + 五重判定）；⑥引擎 Rayon16 运行间非确定（用户已明确：并发下不需要确定性，不算问题不排查，等价性验证用单线程对照）；⑦default-setup.json 未入库（复现性根因）。

**Why:** 用户观察到 16 核 max 档位只有 20-80x（后经优化到 ~220x），认为应达 1000x+（好则 4000x+）。原任务由 codex 会话 01a10d87-eb38-7a13-849a-202f216e845e 执行（方案 A：纯 profiling 驱动优化，语义不变），2026-10-06 起转由 Claude subagent 接续。

**How to apply:** 一切状态以 worktree `.worktree/engine-max-performance`（分支 fix/engine-max-performance）的 `agents/engine-max-performance/current-state.md` 为权威台账，接续前先读它。正式基准只用 production-server-probe 普通 binary + verify-build.py + 顺序独占 benchmark；大量已否决实验清单和硬性流程约束都在 current-state.md/交接记录里，勿重复已否决方向。相关：[[collaboration-mode]]
