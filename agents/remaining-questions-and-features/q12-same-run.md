# Q12：量价与 causal 同批运行

## 目标

默认量价与 causal 报告必须解释同一实际 `GameSession` 运行中的 `Trade` receipt；禁止仅因 seed 相同就称为同批轨迹。独立运行仍通过明确选项保留，并显式注明独立来源。

## 实现记录

- `run_combined_diagnostics` 将量价 `SeedDiagnostics` 和 causal fact collector 接入同一个 `run_one_seed` 步进循环。
- 同批结果以相同 `run_id` 关联两个报告；source metadata 包含运行日数、tick 数以及实际 `Trade` receipt 与 causal 执行事实的笔数、成交股数、成交额摘要。
- 摘要不复制全量 `causal_facts`；若两种来源的成交事实不一致，组合 API 明确返回错误。
- CLI 默认同批模式；`--independent` 显式创建独立运行，causal `run_id` 与 price-volume `run_id` 不同，并标记 seed 不等于调度轨迹。
- 增加短 fixture 测试覆盖共用 run identity 与 receipt 对账。测试采用现有诊断 fixture 的 5 个固定 seed、20 个交易日 NPC 路径，在实际非零 receipt run 上核对同 `run_id` 的两个报告，并显式要求成交笔数、股数、成交额均为正值。
- root 首轮统一编译发现本文件单测 helper 漏传新增 `run_id` 参数，已补为 fixture 的本地 run identity。该轮编译错误是调用参数问题，不是业务断言的 red 证据。
- 首次 exact case 编译后真实运行失败：两交易日 seed 7 产生 0 笔 `Trade` receipt（约 0.31 秒）。单 seed、20 日 seed 7 也在 1.22 秒内确认零成交；没有放松非空成交断言或伪造 receipt。
- 五 seed、20 日版本由 root 实际测得 exact case 6.28 秒并通过非空成交选择与对账断言。最终源码刷新编译后精确用例再次通过，实际执行 6.16 秒，外部 10000ms deadline，日志 `.tmp/checklist-wave2/q12-final-2.log`。非作者 `review_q12_same_run` 对最终完整与增量 diff 复核通过；未运行长矩阵或完整回归。

## 语义边界

诊断来自游戏引擎实际接受与撮合的事件，不读取真实市场数据，不宣称经过真实市场校准。该改动不改变 A 股撮合、交易规则、`Trade` 或存档契约。
