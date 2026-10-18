# runtime A4/A5/A6 修复交接

## 范围与约束

只实施 A4/A5/A6；不提交。不修改 `failure.rs` 的 ends_day 段或 `pipeline/day_end.rs`。
工作区原有 `world.rs`、`session.rs`、生成的 `SaveSlot.ts` 修改均保留；
并行同事另外修改了 `group.rs` 的 day 基准；本任务在其基础上只补齐 `market_minute_at` 的日内进度基准。

## 根因与改动

- A4：`StockCatalog` 按代码升序分配 `StockId`，runtime 原来按 setup 输入顺序构造交易部，
  五股的初始价、涨跌停和板块规格与股票身份错配。回执证明五笔买单没有资金不足或其他拒收；
  错配后 StockId(3) 卖档为 1120 分而买保护价 831 分，StockId(4) 卖档 755 分而保护价 314 分，
  两笔均因无法匹配而撤销。`runtime/drive.rs::from_session_grouped` 仅对克隆的 setup 股票规格排序，
  使交易部、账户矩阵和保护价数组共用 StockId 顺序；底层 WorldSpec 输入顺序契约保持原样。
- A5：组权威 PlanBook 的日结未执行 `TradingDayEnded`，session 镜像终止旧计划之后又被组端 Active
  状态覆盖，过期计划继续报价并在 child 受理时触发有效期 panic。`runtime/group.rs::handle_day_end`
  在 Drain 释放与权益处理后，复用既有事件逐计划扫尾，并将 PlanBook 纳入日结 undo；
  未放宽 `EventBeyondHorizon` 或吞掉非法事件。并行 day 修改一度使 day 与日内分钟使用不同 epoch，
  重新复现 A5 的 watchlist 时间倒退后，本任务将 `market_minute_at` 的 day_tick 同步为 `(tick-1)%tpd`；
  新红证 `11-A5-concurrent-day-regression.txt` 保留，最终 A5 再次通过。
- A6：收盘竞价 cutoff 从 ClosingAuction 残留至 Continuous/PreOpen，runtime 自产档触发
  `fault[2291] [0, 1, 0]`。`runtime/desk.rs` 在相应相位清零 cutoff，并将 cutoff、last_phase
  一起写入 MarketSnapshot 供 Abort 恢复。`tests/step_skeleton.rs` 新增无 hash 锚的完整交易日
  自产档测试；既有断言未修改。

## 依据与语义

依据 ADR-0041 的 StockId 排序约定、既有 plans 的有效期/TradingDayEnded 状态机、
ADR-0025 的跨日个人计划与日终委托生命周期边界，以及 SavedRuntimeState 的现有 cutoff 校验。
本轮修复身份投影、既有计划日结与公开面状态，不新增交易制度或调整费率、T+1、整手、涨跌幅规则。
适用本仓库 2026-10-18 当前契约；交易所规则依据沿用 `docs/trading-rules.md` 中已注明的核验边界。

## 红绿证据

目录：`.tmp/sol-stepframe-red-evidence/`。

| 项目 | 红证 | 绿证 | 补充 |
|---|---|---|---|
| A4 | `11-A4-red.txt` | `11-A4-green.txt` | `11-A4-receipts.txt` 含五单、保护价、全部回执与成交事实 |
| A5 | `11-A5-red.txt` | `11-A5-green.txt` | `11-A5-context.txt` 含受理 tick 与完整计划上下文 |
| A6 | `11-A6-red.txt` | `11-A6-green.txt` | `11-A6-after-fix-suite.txt` 保留整套结果 |

原始复现命令均设置 `ENGINE_STEP_RUNTIME=1`、`RUST_BACKTRACE=full`、16 个测试线程及 9 秒外部执行期限。
A5 红证完整捕获 worker panic 栈后达到期限，退出 124，未等候更长 watchdog；不冒充正常结束的失败测试。
构建独立使用 `cargo ... --no-run -j 16`，构建耗时不混入短测执行时间。
短测 Cargo 重跑因并行同事占用 build lock 达到期限，相应日志保留为 `*-green-cargo-lock-timeout.txt`；
`*-green.txt` 直接运行本轮已编译出的同名 test executable，实际均通过。
A6 整套修复后 4 过/3 败：fault2291 已消失，原先被它阻挡的无开盘竞价用例同样到达旧 hash 断言；
剩余三项均为固定 hash 差异，未修改断言或重锚。

## 独立复核

未参与实施的 `/root/a456_review` 静态审查完整 diff，认为 A4/A5/A6 自身无阻塞发现：
排序范围、计划事件复用、日结 undo 与 cutoff/last_phase undo 合理，无新增交易制度。
该复核未运行 cargo，不替代执行证据。建议后续补强 A5 日结终止/保留和 Abort，以及 A6 相位 Abort 重跑测试。

完整 diff 的范围外发现移交主 agent：

1. `session.rs` 并行修改使用 `filled_value / limit_price` 反推 filled_qty，忽略真实成交价可不同于限价；
   `unwrap_or(0)` 还会掩盖异常。需由该修改负责人处理，本任务不覆盖该实现。
2. `group.rs` 并行 day 修改的混用问题已核对并修正：同事统一开仓与 RootReadContext.day，
   本任务补齐 market_minute_at 的日内进度。独立 subagent 再次确认该一行修正符合冻结观察 epoch 契约；
   最终 A5 与 `civil_fixture_sell_direction_is_absent_at_production_source` 均通过。

## lib 回归

**未满足 1654 过 / 0 败要求：完整覆盖汇总为 1646 过 / 8 败 / 11 忽略。**
未为回归变绿修改、删除、弱化任何断言。

先按要求执行 `ENGINE_STEP_RUNTIME=1 cargo test -p engine --lib -j 16 -- --test-threads=16`，
构建与等待 build lock 消耗了批次期限，完整日志保留于 `11-A456-lib-runtime-cargo-timeout.txt`。
构建单独完成后直接执行同一 lib test executable。未隔离的并发轮次为 1643 过 / 11 败 / 11 忽略，
记录于 `11-A456-lib-runtime-unpartitioned.txt`；其中两项被其他用例的全局 DECIDE_PANIC_ACCOUNTS
注入污染，另一项为后续已修正的 day/分钟混用错误。

最终批次在同一个 300000ms 外部 deadline 下完整覆盖全部 1665 个登记用例：

- 主分片：`engine-e2b004eecda01d2b --test-threads=128 --skip session::pipeline::npc_decisions_quota_tests::`，
  1636 过 / 8 败 / 11 忽略 / 10 分片外用例，71.73 秒。
- 隔离分片：同一 executable 筛选 `session::pipeline::npc_decisions_quota_tests:: --test-threads=1`，
  10 过 / 0 败，0.23 秒。此套件共享进程级 panic/hold hook，因此单独进程内串行；
  主分片同时继续并行，既不丢失用例，也不以测试专用业务分支规避失败。
- 合计：1646 过 / 8 败 / 11 忽略，退出 101。

最终完整日志：`.tmp/sol-stepframe-red-evidence/11-A456-lib-runtime.txt`；
原始分片日志：`11-A456-lib-main.txt`、`11-A456-lib-quota.txt`。
实际并行采样证据 `11-A456-lib-runtime-cpu.txt`：机器可用 128 CPU，未隔离轮次首个十秒窗口约 5252% CPU，
采样时 87 个线程；该采样不冒充所有时段满核证明。最终批次同样显式设置主分片 128 个测试线程。

八项失败均经未参与实施的 subagent 独立核对：

1. `sol6th_same_tick_abort_leaks_nothing`
2. `sol7th_projection_matrix_active_sell_passive_buy_and_add_on`
3. `sol8th_retail_first_hold_experience_initializes_same_tick`
4. `prereq1_continue_early_round_fills_settle_exactly_once`
5. `solfinal1b_continue_early_round_first_buy_initializes_experience`
6. `p2_4a_sparse_account_writeback_matches_force_full_bitwise`
7. `p2_4b_envelope_incremental_matches_full_rebuild`
8. `p2_3a_retail_chain_shared_observation_golden`

前七项 fixture 沿用错误映射：StockId(0) 实际为 300001、初始价 2000 分、创业板，
却提交 1000 分委托，低于跌停 1600；StockId(1) 实际为 600000、初始价 1000 分、主板，
却提交 2000/2050 分委托，高于涨停 1100。A4 修正后正确拒收，不能重新引入错映射或做测试专用分支。
第八项固定 golden 基于旧错映射，实测 `(13, 0, 1000065569869)` 与旧锚
`(13, 100, 1000065568818)` 不同；没有更改锚，也不将静态解释冒充新 golden 的完整验收。
按用户“不改断言，过时也报告”的约束，八项明确移交主 agent 处理。

## 并行工作区边界

最终定向绿证来自 `11-A456-aligned-build.txt` 发布的 test executables，A4 5.37 秒、A5 0.99 秒、
A6 专项 0.85 秒。完整 lib 最终批次也是该构建的产物。
本任务收尾时同事又在 `drive.rs` 添加日 K archive_limit/initial_candle_history_len，
并在 `failure.rs` 添加 runtime_candle_histories；这些不是本任务实施或以上产物覆盖的修改。
不要将本报告扩张为持续变动工作区的最终集成验收，主 agent 应在并行工作汇合后统一验收。
