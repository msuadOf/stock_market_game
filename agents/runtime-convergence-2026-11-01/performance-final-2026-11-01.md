# Runtime 最终集成版本同条件代表性比较

日期：2026-11-01。main `45892c8edfd05fad1ee89207ac94fb863305115d` 与集成源码 `91b3ce281f454ea7cc24b3ff8136367a9aee6b2d`；构建前后工作树干净且 HEAD 一致。两端以 Rust/Cargo 1.96.1、release、各 8 个 Cargo jobs 构建；运行时设置 `ENGINE_STEP_RUNTIME=1`、`ENGINE_SPIN_US=500`、CPU 0–15、seed 42，setup SHA-256 `58b5b191321560b37b62d4842f026cea7ab72917b918796e1579b5cfa3995472`。EveryTick、5 股、20007 NPC，每次完成 15301 tick、日结和次交易日首拍，并确认 shutdown。各运行设置外部 55 秒期限；主程序及其 runtime worker 使用同一 CPU affinity。

| 顺序 | 版本 | ticks/s |
| --- | --- | ---: |
| 1 | main | 471.82 |
| 2 | 集成 | 453.47 |
| 3 | 集成 | 460.83 |
| 4 | main | 457.10 |

以 `completed_ticks / run_wall_ns` 计算，main 均值 464.46 ticks/s，集成均值 457.15 ticks/s，集成低约 1.57%，在用户允许的 20% 波动范围内。每端仅两次，机器同时存在其他工作负载，不能证明原因、长期性能或 1000x 目标。此负载不证明 A 股规则正确性。

原始 JSON：`/tmp/perf-final-{1-main,2-integrated,3-integrated,4-main}.json`；构建日志：`/tmp/perf-final-main-build.log`、`/tmp/perf-final-integrated-build.log`。二进制 SHA-256：main `ef440d4f4ac00112dbc9344abcee274c4531b677dfa6935e02f4133cb4d19390`，集成 `268b8a66ba517c38caff1b2663c903acd5cedfa7286eba4722c6bbe1e1b9a093`。版本、环境与 affinity 根据执行记录绑定，JSON 未内嵌这些信息；逐核利用率未完整采样。

独立复核已核对数值与测量限制；复核结论由主任务提供。
