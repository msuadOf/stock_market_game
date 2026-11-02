# e959 回归与性能复测

日期：2026-11-02。集成源码 `e9592853162655dce27f3ea8f726fb57e952371f`。
正式 `all` 回归退出码 0：build 6092 ms、execute 156958 ms、总计 163053 ms，日志为 `/tmp/stock-final-e9592853-regression.log`。回归后 `ps` 未发现匹配进程；监督器仍存在已确认限制：脱离原进程树的子进程可能漏检，因此不能据此证明所有后代均已终止。

性能比较使用 main `45892c8edfd05fad1ee89207ac94fb863305115d` 与集成版本，依次运行 main、集成、集成、main。两端使用 release probe、`ENGINE_STEP_RUNTIME=1`、`ENGINE_SPIN_US=500`、CPU 0–15、seed 42，同一 setup（SHA-256 `58b5b191321560b37b62d4842f026cea7ab72917b918796e1579b5cfa3995472`）；EveryTick、5 股、20007 NPC，每次均完成 15301 tick、次交易日首拍及 shutdown，退出码均为 0。每次运行由外部 `timeout` 限制为 55 秒，另设 2 秒强制终止宽限。

| 顺序 | 版本 | run_wall_ns | ticks/s |
| --- | --- | ---: | ---: |
| 1 | main | 21364045078 | 716.20 |
| 2 | 集成 | 20712831197 | 738.72 |
| 3 | 集成 | 20010682738 | 764.64 |
| 4 | main | 20332772667 | 752.53 |

按 `completed_ticks / run_wall_ns` 计算，main 均值 734.37 ticks/s，集成均值 751.68 ticks/s，高约 2.36%，在允许的 20% 波动范围内。原始 JSON 位于 `/tmp/perf-retest-e959-{1-main,2-current,3-current,4-main}.json`，时间和负载记录位于 `/tmp/perf-retest-e959-meta.txt`。

main probe SHA-256 为 `ef440d4f4ac00112dbc9344abcee274c4531b677dfa6935e02f4133cb4d19390`；集成 probe 为 `b2f957aca367b64a08cd904e2ce755631115e58c1177edc45ee1214fb7973bd1`。在干净的 e959 工作树以 8 个 Cargo jobs 增量构建集成 release probe，构建前后 HEAD 一致，产物哈希与复测相同；构建受外部 300 秒期限监督，日志为 `/tmp/perf-e959-rebuild.log`。该证据依赖 Cargo 增量缓存，并非空 target 的独立可复现构建。

运行时机器有其他负载，1 分钟 load average 约 58；每个版本仅测两次。抽样观察到多个 runtime 线程活动，但未完整采样逐核利用率。这些结果不能证明长期性能或 A 股交易规则正确性。

证据来源限制：CPU affinity、环境变量、timeout 参数、Cargo jobs 和 main 二进制哈希来自本轮执行记录，以上原始 JSON 与重建日志未内嵌这些参数；回归退出码和进程检查结果来自回归执行者记录。fixture 中的规则参数仅为负载输入，不构成现行交易制度依据。
