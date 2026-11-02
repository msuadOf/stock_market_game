# 319bcfac R4 生产入口测试与 main 的逐项映射

基线：`main` = `0f40ffbc`（本工作树另加 `packages/engine/tests/company_r4_production_gap.rs`）。
历史对象：`319bcfac:packages/engine/tests/company_r4_production.rs`（10 例）。
性质：`319bcfac` 的提交信息是“固定 R4 生产入口集成红证”，其 red4 实测为 2 pass / 8 fail，
原独立复核只批准该片作为 TDD 红证与设计交接。详见
`agents/branch-completion/r4-production/independent-review-2026-10-26.md`。

## 已批准契约与现行实现现状

- **行为契约**：`agents/remaining-questions-and-features/q23-report-frequency.md` 是用户已批准的
  月报契约。`:43` 要求“公布时点必须实际进入公开与本人获知链，不将日内指定时点拖到日终后再伪称
  已及时公开”；`:49` 明确“交易日月报在私有 tick candidate 中公布……玩家报表查询使用当前观察
  时刻而不是当日00:00”，并规定“休市日以及收盘后的月报在自然日日结处理时按公布时点逐项送达”
  与“月度模式的日终派发端点是 23:59:59，默认季度模式保留 18:00”。
- **领域原则**：`docs/decisions/0016-fundamental-factor-model.md:27` 要求经营事实与公开信息分开、
  NPC 不得提前读取未披露结果；`0035-company-system-simple-fundamentals.md:25` 要求公开材料与
  本人获知分离，玩家只能查询已公开材料。
- **现行实现现状**：`GameSession::end_civil_day` → `prepare_civil_day_disclosures` 是当前唯一的
  公开报告路径；盘中 `step()` 不公布报告，也不执行本人信息检查。`query_public_reports` /
  `public_report_by_id` 仍以当日 00:00 为 `as_of`。因此 **交易日月报的盘中公开与盘中获知相对
  Q23 是已批准但未实现的缺口**。`q11-notice-cadence.md` 与 `docs/simulation-calendar.md` 记录的
  是这一现行实现状态，不能反过来把日终-only 当成覆盖 Q23 的权威语义。
- **历史接线方式**：`publish_intraday_reports` 曾在 `ce5b3a1c` 存在，`87f6f7d9` 之后不再出现在
  编译面。具体移除原因与恢复方式由 root 调查；行为要求本身不受接线方案影响。

## 逐断言映射

| 历史用例 | 断言核心 | main 现状 | 判定 |
|---|---|---|---|
| r4_01 | Simple 前史只含 `SimpleGenerated` | `packages/engine/tests/company_source_selection.rs::selected_simple_initial_public_history_comes_from_simple_generated_reports` | 已覆盖 |
| r4_02 | Legacy 前史保留 `SimulationAccounting` | 同文件 `selected_legacy_save_contains_only_legacy_company_state_and_original_public_source` | 已覆盖 |
| r4_03 | 闭市月末日结推进所选 Simple、无 tick | `packages/engine/src/session/company_source_runtime_tests.rs::selected_simple_constructs_real_history_without_any_legacy_call_and_finalizes_once`（单组，真实 finalize 计数与 `advanced_through`）；新增 `empty_personal_groups_settle_simple_company_at_month_end`（三组，只验证日结成功、`advanced_through` 与无 tick，不单独观测 finalize） | 已覆盖（新增补三组；finalize 由既有例覆盖） |
| r4_04 | 季度闭市日披露终点 18:00、无 tick | 新增 `quarterly_closed_day_keeps_the_18_for_scan_endpoint` | 已覆盖（新增） |
| r4_05 | 闭市日晚间 Monthly 到 23:59:59 可见、无 tick | `company_source_runtime_tests::selected_simple_new_publication_failure_preserves_library_cursor_and_rng_then_retries`（20:00、单组）；新增 `empty_personal_groups_settle_simple_company_and_keep_the_late_monthly_window`（23:30、三组空个人） | 已覆盖（新增补三组与 23:30） |
| r4_06 | 闭市日不制造 10:30 本人观察 | `packages/engine/src/runtime/group/admin_transaction/information_tests.rs::info_12_closed_day_evening_monthly_is_acquired_at_real_end_without_market_tick`（Admin 层）；新增 `closed_day_monthly_is_published_at_schedule_second_but_observed_only_at_day_end`（Session 层、Simple、三组、休市日） | 已覆盖（新增补 Session 入口） |
| r4_07 | 交易日 10:30 即时公开、11:00 本人获知、下一 step 保留 | **Q23 已要求但 main 未实现**：无盘中 tick candidate 公布调用点，个人检查也不在盘中执行 | **未覆盖 / 未核销**（由单独 Q23 实现任务处理） |
| r4_08 | 空个人组仍推进 Simple | `company_source_runtime_tests`（单组）；新增 `empty_personal_groups_settle_simple_company_at_month_end`（三组、6/30 月末推进） | 已覆盖（新增补三组） |
| r4_09 | 空个人组不丢晚间 Monthly 窗口 | 同 r4_05 的既有单组例；新增三组 23:30 例 | 已覆盖（新增补三组） |
| r4_10 | 三组保留真实名册与尾组个人行 | 新增 `three_runtime_groups_keep_the_real_roster_and_tail_group_personal_rows` | 已覆盖（新增） |

## 新增测试（最小代表集）

`packages/engine/tests/company_r4_production_gap.rs`，6 个短例：

1. `quarterly_closed_day_keeps_the_18_for_scan_endpoint` — 非 Monthly 的日终终点（Q23 保留季度 18:00）。
2. `empty_personal_groups_settle_simple_company_at_month_end` — 三组空个人、6/30 月末日结推进到 `2030-06-30`、无 tick。该例不单独观测月末 finalize；finalize 由既有 `selected_simple_constructs_real_history_without_any_legacy_call_and_finalizes_once` 的 `simple_finalize` 计数覆盖。
3. `closed_day_monthly_is_published_at_schedule_second_but_observed_only_at_day_end` — 休市 6/1，
   `published_at` = 排期 10:30，本人获知在 23:59:59；与 Q23“休市日按自然日日结送达”一致。
4. `three_runtime_groups_keep_the_real_roster_and_tail_group_personal_rows` — 8 个真实账户分 3 组，
   尾组 2 个账户，不补 padding owner。
5. `empty_personal_groups_settle_simple_company_and_keep_the_late_monthly_window` — 三组空个人、
   6/1 晚 23:30 窗口不被丢弃（Q23 月报日终端点 23:59:59）。
6. `trading_day_monthly_keeps_schedule_second_and_retains_acquired_material` — **只固定不受
   Q23 缺口影响的表征**：`published_at` 保留排期秒值、本人获知不早于排期且不晚于本次日结相位、
   获知后跨下一 step 保留。**它不断言交易日盘中不可见，也不断言获知只能在日终**，因此不会在
   盘中公开接通后判红；它不是 r4_07 的替代或恢复。

## 证据

见同目录 `evidence-manifest.json` 与 `logs/`：构建在 worktree 独立 target
（`.tmp/r4-gap-target`）内完成，进程外 300000ms deadline、`CARGO_BUILD_JOBS=64`；
每个 case 独立进程、`--test-threads=1`、进程外 10000ms deadline。最终 6/6 通过，
单例 wall 约 0.3–0.6 秒。负控（临时去掉 Monthly 日终端点分支）只作敏感性检查，
不宣称 TDD 业务红；负控后已 revert，最终二进制与日志来自 revert 后的源码。

## 遗留

1. Q23 `:43`/`:49` 已批准交易日月报盘中实际公开与本人获知；r4_07 保持未核销，
   由单独的 Q23 实现任务处理。当前 main 只有日终路径，属实现缺口，不是契约变更。
2. `publish_intraday_reports` 的移除原因（`ce5b3a1c` → `87f6f7d9`）需 root 查明；
   本轮不据历史实现自行恢复盘中接线。
