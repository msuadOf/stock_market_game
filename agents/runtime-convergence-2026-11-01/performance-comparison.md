# Runtime 同条件代表性性能比较

日期：2026-11-01。对比 main `45892c8edfd05fad1ee89207ac94fb863305115d` 与集成版本 `d9c93370e025dea730f22157edde77c068940140`。两端使用相同 Rust release 编译参数、`ENGINE_STEP_RUNTIME=1`、`ENGINE_SPIN_US=500`、CPU 0–15 affinity、seed 42 和相同 setup（SHA-256 `58b5b191321560b37b62d4842f026cea7ab72917b918796e1579b5cfa3995472`）。负载为 EveryTick、5 股、20007 NPC，每次完成 15301 tick、真实日结及下一交易日首拍，并确认 shutdown。

按 main、集成、集成、main 顺序各运行两次，吞吐以完成 tick 数除以包含日结的 `run_wall_ns` 计算：

| 版本 | 第一次 ticks/s | 第二次 ticks/s | 均值 ticks/s |
| --- | ---: | ---: | ---: |
| main | 1027.80 | 816.62 | 922.21 |
| 集成 | 884.98 | 927.65 | 906.31 |

集成均值比 main 低约 1.7%，处于用户允许的 20% 波动范围内，可作为代表性比较。每组仅两次且 main 波动较大，不能据此归因具体改动或断言性能改善、回归。测前未记录逐核低于 10% 的条件；未完成既定五批及 N/2N，故此结果不能证明 1000x 正式目标达成。

原始输出：`/tmp/runtime-benchmark-1-main.json`、`/tmp/runtime-benchmark-2-integrated.json`、`/tmp/runtime-benchmark-3-integrated.json`、`/tmp/runtime-benchmark-4-main.json`。二进制 SHA-256 分别为 main `ef440d4f4ac00112dbc9344abcee274c4531b677dfa6935e02f4133cb4d19390`、集成 `83019f85b6344a80c3ea2a6c606c02f212ae6f924dabed02988cbc5b0eacfb62`；临时构建目录已清理。

上述版本、编译参数、CPU affinity 和二进制 SHA 来自运行时操作记录，原始 JSON 未自带这些绑定字段；本次未保存完整 CPU 利用率采样，独立复核无法仅凭 JSON 验证这些条件。此负载比较不证明 A 股交易规则正确性。
