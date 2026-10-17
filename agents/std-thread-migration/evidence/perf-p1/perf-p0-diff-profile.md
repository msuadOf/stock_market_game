# P0 差分 profiling：HEAD(exp/std-thread-model) vs 5412fd10（A 口径）

日期：2026-10-17。机器 128 核。A 探针=runtime_s5_probe（3000 tick、10 组×2000 NPC、5 股），
两侧均 `CARGO_PROFILE_RELEASE_DEBUG=1` release 构建（带符号）。
基线 worktree：`.worktree/perf-baseline-5412fd10`（detached，保留供 P1 复测）。

## 方法与原始数据

- 采样：`perf record -F 499/999 --call-graph dwarf,16384`（kptr_restrict 已放开为 0）
  - baseline：`.tmp/perf-records/baseline-5412fd10.perfdata`（34861 样本，丢 9.29%）
  - HEAD：`.tmp/perf-records/head-std-thread.perfdata`（53576 样本，丢 0.12%）
- 定标：`perf stat`（task-clock/上下文切换）；受理量=探针临时诊断输出（`report_log`
  lifecycle 计数，已还原）
- 占比均为 perf 周期加权（DSO/符号/comm 口径）；CPU·s=样本×周期折算+perf stat 交叉验证

## 总量对比（决定性数字）

| 指标 | 5412fd10（663 t/s） | HEAD（71-105 t/s） | 落差 |
|---|---:|---:|---|
| wall（3000 tick，无采样） | 4.52 s | 30.16 s（本轮 99.5 t/s） | 6.7× |
| task-clock 总 CPU | ≈38 CPU·s | 118.7 CPU·s | 3.1× |
| 平均并发核数 | ≈8.5 核 | **3.93 核** | 坍塌 2.2× |
| 上下文切换 | （未测，占比低） | **1385 万次 ≈ 4618 次/tick** | — |
| 内核态周期占比（DSO） | ≈11%（[unknown] 换算） | **87.1%**（kernel 81.1+vdso 3.9+libc 1.9） | 翻盘 |
| 用户态（engine 二进制）占比 | ≈86% | **13.0%** | — |
| 协调者线程 CPU 占比 | 3.3% | **26.3%（≈28 CPU·s≈满负荷单线程）** | 8× |

**wall 落差 6.7× ≈ CPU 功放大 3.1× × 并行度坍塌 2.2×。**

## 线程分布（HEAD）

协调者 26.3% + 组线程 10×(6.4~11.1%)≈73% + desk 5×≈2.7%。
baseline：协调者 3.3%、组 ≈68%、desk ≈27%（HEAD desk 掉到 2.7%=成交量少 5.7×的镜像）。

## 嫌疑点对表（Sol 预估 vs 实测）

| 嫌疑点 | Sol 档位预估 | 实测（A 口径） | 结论 |
|---|---|---|---|
| **协调者串行瓶颈：ChildAccepted 逐笔×全组广播 + 每轮消息往返**（world.rs run_ticks：受理事实逐笔 send 给全部 10 组） | "逐笔 Accepted 广播全部组——中至低；低受理量时有限"（P3） | **实测最大项**：lifecycle 474.8 条/tick×10 组≈数千 send/tick；协调者单线程 28 CPU·s≈满负荷=wall 关键路径；叶符号=futex_wake/select_idle_sibling/ttwu/mpmc send；1385 万次上下文切换 | **Sol 严重低估**。受理量并不低（474.8/tick），广播量=受理×组数 |
| **决策层计算放大：evaluate 门+MarketView 重建**（decision.rs evaluate_attention 每 due NPC 建 5 股 BTreeMap+复制历史；decide 再建一次） | A 首要嫌疑，高；估 40-65% | 实锤但量级第二：组线程用户态计算从 ≈33 CPU·s→≈15 CPU·s（绝对值反而降因成交量少），构造次数 3.4× 放大属实（baseline build_market_view 已占 22%）。在 HEAD 只占用户态 13% 里的大头，**wall 占比受协调者瓶颈压制** | 实锤、次序第二；修掉后组线程更闲，若协调者瓶颈不动则 wall 收益有限 |
| **每 tick 多轮全组 barrier 的睡醒税**（Start→收束→受理广播→Continue…） | 相位绑定/链上下文"暂列低" | **实锤且大**：87% 内核周期全在 futex/调度记账（sched_balance_newidle/dequeue_entity/psi/cpuacct 均为高频睡醒路径），4618 次切换/tick | 与广播问题同根：消息条数×组数决定睡醒次数 |
| desk 公共簿全量重建 | 中 | desk 线程只占 2.7% CPU | A 口径当前非瓶颈 |
| attention heap 无消费增长（failure.rs enqueue/clone） | B 首要结构性缺陷（O(N·T²)） | A 口径不走 session（无 heap 问题）；B 口径待 P1-1 红证定标 | 与 Sol 一致，B 专属 |
| vdso 0xa06（时钟读取，recv_deadline 等） | 未列 | 3.9% 周期 | 次要，随消息量下降 |

## 机理解释（HEAD 为何 87% 周期在内核）

每 tick：协调者广播 Start→组决策→收束→**受理事实逐笔 send×10 组**（数千次）→（continuation 轮再广播）→install。
每次 send 到空闲组线程=futex_wake syscall+挑核（select_idle_sibling）+IPI+两次调度器穿越+PSI/cpuacct 记账。
组线程处理几条消息又睡。1385 万次切换 × 每次全路径 ~7μs ≈ 97 CPU·s 内核，对上 118.7−13%用户态。
协调者作为唯一发送中枢，自己 28 CPU·s 满负荷——**它是串行关键路径**：wall ≥ 协调者 CPU 时间。

## P1 顺序修正（以实测为准，按新指令）

1. **P1-A：受理事实按组批发**（替代"逐笔×全组"）：一次 Vec send/组/轮，消除数千→数次
   send。语义等价守卫=同种子受理/成交/事件序列逐字段等价（消息仅合批，顺序与内容不变）。
   预期直接拆掉协调者瓶颈 → wall 显著下降（定量待测）。
2. **P1-B：每 snapshot 共享 MarketView/attention signal**（原 P1-2）：组线程用户态计算砍 ~3×。
   RNG 消费顺序不变（signal/market 构建不消费 RNG）——同种子决策序列逐字段等价硬验收。
3. **P1-C：B 口径 heap 无消费增长修复**（原 P1-1）：红证=heap 项数不随 tick 增长+行为等价。

（任务书原 P1 两项=B/C；差分实测把 Sol P3 的"受理广播"提为首位——按"以实测为准"指令执行，
其余纪律不变：每件先红证后修、修复一个测一个报一个。）
