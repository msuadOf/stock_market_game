---
name: std-thread-migration
description: 用户决策（2026-10-14）：迁移到 std::thread 内联方案（一股一持久线程+内联决策）；代码本体无线程数管理概念；CPU 资源用 OS 级限制（如 cgroup/taskset）
metadata:
  node_type: memory
  type: project
  originSessionId: f28cd04a-805e-42cf-b943-c0b6a1ae0d82
  modified: 2026-10-14T16:40:43.028Z
---

用户拍板（2026-10-14）：从 rayon 迁移到纯 std::thread 并发模型，使用内联方案。

**核心设计（用户原话要旨）：**
- 一股一个持久主线程（直接创建，不管理）
- tick 时主线程内联做 NPC 决策（5 股负载最优=不开子线程；更多股时按需）
- 统一 join → 多线程撮合 → 收束
- **代码本体不需要有线程数管理概念**——没有 worker 数、没有池宽、没有 RAYON_NUM_THREADS
- CPU 资源限制用 **OS 级能力**（如 cgroup cpu quota / taskset affinity），不用代码内 flag

**Why:** std-thread 原型实测 982 t/s（rayon 359 t/s 的 2.7×）；模型清晰无框架开销；用户否决了池宽封顶和 rayon 补丁，直接换到更简单的方案。

**最终方案（2026-10-15 用户确认）：资源实体所有权转移模型**
- NPC 按分组开线程（一组一线程），持有组内 NPC 的现金/持仓实体
- 决策时从持有账户把现金实体扣出，所有权 move 给交易部线程（一股一线程）
- 交易部独占撮合（零锁），成交后实体互换/退回
- 原型实测：**962 t/s**（10 组调优，s5 负载），完整跨股语义，零锁零共享
- 纯机制税仅 ~8%（打包成批次消息+流水线重叠）

**How to apply:**
1. 迁移目标=替换 ComputeContext/rayon 为 std::thread + channel（所有权转移协议）
2. 代码里不出现 worker count / thread pool width 概念
3. 资源限制走部署层（OS 级），不写进引擎代码
4. NPC 按分组分线程，交易部按股分线程，实体在两者间流动
5. 关联：[[resource-entity-model]]（完整实现）、[[npc-grouping-relaxed-latency]]（备选方案）
