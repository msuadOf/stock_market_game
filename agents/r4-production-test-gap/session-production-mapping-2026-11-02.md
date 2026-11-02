# 319bcfac R4 生产入口测试与 main 的逐项映射

基线：`main` = `0f40ffbc`（本工作树另加 `packages/engine/tests/company_r4_production_gap.rs`）。
历史对象：`319bcfac:packages/engine/tests/company_r4_production.rs`（10 例）。
性质：`319bcfac` 的提交信息是“固定 R4 生产入口集成红证”，其 red4 实测为 2 pass / 8 fail，
原独立复核只批准该片作为 TDD 红证与设计交接，不批准生产接线。详见
`agents/branch-completion/r4-production/independent-review-2026-10-26.md`。

## 现行权威（本次核对）

- `docs/decisions/0016-fundamental-factor-model.md:27`：经营事实与公开信息分开，NPC 不能提前读取尚未披露的经营结果。
- `docs/decisions/0035-company-system-simple-fundamentals.md:25`：公司内部生成事实与公开材料、账户本人获知分离；玩家只能查询已公开材料。
- `agents/remaining-questions-and-features/q11-notice-cadence.md`：现行 Session 发布来自
  `DisclosureDispatch::run_day_end`，公开时点固定于日终披露相位；本人检查在同一日终事务内执行。
- `docs/simulation-calendar.md:97-99`：收盘后补齐当天经营及 18:00 披露，到次日才更新 civil date。
- 现行代码：`GameSession::end_civil_day` → `prepare_civil_day_disclosures` 是唯一公开报告路径；
  Monthly 频率把披露终点延长到当日 23:59:59，其余频率保持 18:00。盘中 `step()` 不公布报告，
  也不执行本人信息检查（`deliver_public_information` 只在日结路径调用）。

结论：`319bcfac` 的 r4_07 要求“10:30 即时公开、11:00 本人获知”，属于尚未接线的盘中多 hook
候选方案，**不属于现行 main 语义**。旧方案未被批准，不得把现行日终例冒充 r4_07 已恢复。

## 逐断言映射

| 历史用例 | 断言核心 | main 现状 | 判定 |
|---|---|---|---|
| r4_01 | Simple 前史只含 `SimpleGenerated` | `packages/engine/tests/company_source_selection.rs::selected_simple_initial_public_history_comes_from_simple_generated_reports` | 已覆盖 |
| r4_02 | Legacy 前史保留 `SimulationAccounting` | 同文件 `selected_legacy_save_contains_only_legacy_company_state_and_original_public_source` | 已覆盖 |
| r4_03 | 闭市月末日结推进所选 Simple、无 tick | `packages/engine/src/session/company_source_runtime_tests.rs::selected_simple_constructs_real_history_without_any_legacy_call_and_finalizes_once`（单组、无 tick 断言）；新增 `empty_personal_groups_settle_simple_company_at_month_end`（三组） | 已覆盖（新增补三组边界） |
| r4_04 | 季度闭市日披露终点 18:00、无 tick | 新增 `quarterly_closed_day_keeps_the_18_for_scan_endpoint` | 已覆盖（新增） |
| r4_05 | 闭市日晚间 Monthly 到 23:59:59 可见、无 tick | `company_source_runtime_tests::selected_simple_new_publication_failure_preserves_library_cursor_and_rng_then_retries`（20:00、单组）；新增 `empty_personal_groups_settle_simple_company_and_keep_the_late_monthly_window`（23:30、三组空个人） | 已覆盖（新增补三组与 23:30） |
| r4_06 | 闭市日不制造 10:30 本人观察 | `packages/engine/src/runtime/group/admin_transaction/information_tests.rs::info_12_closed_day_evening_monthly_is_acquired_at_real_end_without_market_tick`（Admin 层）；新增 `closed_day_monthly_is_published_at_schedule_second_but_observed_only_at_day_end`（Session 层、Simple、三组） | 已覆盖（新增补 Session 入口） |
| r4_07 | 交易日 10:30 即时公开、11:00 本人获知、下一 step 保留 | **未实现**：main 无盘中公开与盘中本人检查入口；`query_public_reports`/`public_report_by_id` 仍以当日 00:00 为 `as_of` | **未覆盖 / 未核销** |
| r4_08 | 空个人组仍推进 Simple | `company_source_runtime_tests`（单组）；新增 `empty_personal_groups_settle_simple_company_at_month_end`（三组、6/30 月末） | 已覆盖（新增补三组） |
| r4_09 | 空个人组不丢晚间 Monthly 窗口 | 同 r4_05 的既有单组例；新增三组 23:30 例 | 已覆盖（新增补三组） |
| r4_10 | 三组保留真实名册与尾组个人行 | 新增 `three_runtime_groups_keep_the_real_roster_and_tail_group_personal_rows` | 已覆盖（新增） |

## 新增测试（最小代表集）

`packages/engine/tests/company_r4_production_gap.rs`，6 个短例：

1. `quarterly_closed_day_keeps_the_18_for_scan_endpoint` — 非 Monthly 的日终终点。
2. `empty_personal_groups_settle_simple_company_at_month_end` — 三组空个人、6/30 月末 finalize、无 tick。
3. `closed_day_monthly_is_published_at_schedule_second_but_observed_only_at_day_end` — 休市 6/1，
   `published_at` = 排期 10:30，本人获知只在 23:59:59。
4. `three_runtime_groups_keep_the_real_roster_and_tail_group_personal_rows` — 8 个真实账户分 3 组，
   尾组 2 个账户，不补 padding owner。
5. `empty_personal_groups_settle_simple_company_and_keep_the_late_monthly_window` — 三组空个人、
   6/1 晚 23:30 窗口不被丢弃。
6. `custom_monthly_schedule_is_visible_only_after_day_end_and_read_at_the_disclosure_instant` —
   交易日自定义 10:30 排期、日终才可见、日终获知、下一 step 保留。**该例只固定现行日终契约，
   不是 r4_07 的替代或恢复。**

## 证据

见同目录 `evidence-manifest.json` 与 `logs/`：构建在 worktree 独立 target
（`.tmp/r4-gap-target`）内完成，进程外 300000ms deadline、`CARGO_BUILD_JOBS=64`；
每个 case 独立进程、`--test-threads=1`、进程外 10000ms deadline。最终 6/6 通过，
单例 wall 275–561ms。负控（临时去掉 Monthly 日终端点分支）使 4 个依赖该端点的例子失败，
季度端点与名册两例不受影响；负控只作敏感性检查，不宣称 TDD 业务红。负控后已 revert，
最终二进制与日志来自 revert 后的源码。

## 遗留

1. r4_07 的盘中 10:30 公开 / 11:00 本人获知未实现，保持未核销；需要盘中披露与本人检查的
   正式契约后才能接线，不能由日终例替代。
2. 契约漂移待 root 裁定：`agents/remaining-questions-and-features/q23-report-frequency.md:49`
   记载“交易日月报在私有 tick candidate 中公布……玩家报表查询使用当前观察时刻而不是当日00:00”，
   并引用 host23 的 10:30 公布证据；现行 main 无该调用点（`publish_intraday_reports` 曾在
   `ce5b3a1c` 存在，`87f6f7d9` 后不再编译），Session 查询仍用当日 00:00。本轮未据该记录实施
   盘中接线。
