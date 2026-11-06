# 当前 main 性能复测

2026-11-03，当前 `main=93b36ea7da531b15ce4d599a087e1c14dff91d36`，对照历史基线 `45892c8edfd05fad1ee89207ac94fb863305115d`。当前版本在独立 target 以 `cargo build --locked --release -p engine --example runtime_sync_policy_probe --jobs 32` 构建；基线复用已有 probe，SHA-256 与先前报告一致。构建使用 300 秒进程外 deadline，退出码 0；build.log 记录了 4 条 engine 既有警告。

使用同一 setup（SHA-256 `58b5b191321560b37b62d4842f026cea7ab72917b918796e1579b5cfa3995472`），`every_tick`、seed 42、5 只股票、20007 个 NPC，CPU affinity `64-79`，`ENGINE_STEP_RUNTIME=1`、`ENGINE_SPIN_US=500`。批次为基线、当前、当前、基线的 ABBA 顺序，整批受 300 秒进程外 deadline 约束，退出码 0，外测 109.14 秒。四轮均完成 15301 tick、进入下一交易日首 tick，事件数 116323。

| 顺序 | 版本 | run_wall_ns | ticks/s |
| --- | --- | ---: | ---: |
| 1 | 基线 | 25030340573 | 611.298 |
| 2 | 当前 | 22352573163 | 684.530 |
| 3 | 当前 | 23399071004 | 653.915 |
| 4 | 基线 | 26002947569 | 588.433 |

计算方法为 `completed_ticks / run_wall_ns`，再对每侧两个吞吐样本取算术均值：基线 599.866 ticks/s，当前 669.222 ticks/s，比值 **1.11562×**，即吞吐高约 **11.56%**。每侧只有两个样本，测试时主机有其他负载；此结果只适用于该 native engine probe 和 setup，不代表 Web、存储 I/O 或全部游戏场景的加速，也不足以证明稳定的 11.56% 改善。

本次没有覆盖旧结果。原始输出和进程树采样在 `samples/`，构建与批次日志在本目录。当前 probe SHA-256 为 `49fadfb790fd8cbbb79c5f56ca680be83bee95d7e8f8fb9f76885cc0c2eda71e`，基线 probe SHA-256 为 `ef440d4f4ac00112dbc9344abcee274c4531b677dfa6935e02f4133cb4d19390`。本轮不修改 A 股业务规则或产品代码。
