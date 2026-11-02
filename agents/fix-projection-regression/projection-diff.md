# step_skeleton projection_digest 集成失败取证（2026-11-03）

## 结论

三个 `step_skeleton` 完整投影失败不是交易语义回归，也不是生产缺陷。集成分支
`965e5d84` 相对 `main@0f40ffbc` 只改变了存档中 `CalendarPolicySpec` 的两项表示：

- `civil_clock.policy.official_coverage` 从空数组变为 SSE/SZSE 2026 各一条已核验
  官方休市覆盖；
- `civil_clock.policy_digest` 从不存在变为按完整 policy spec 计算的
  `e39b4ae1f109313d`。

60 个 tick 的 `events` 与 live `snapshot` 原字节完全相同。存档其余所有字段也
相同。三个新锚是预期表示变化的重新锚定，旧锚及历史说明全部保留。

## 复现与比较方法

- main 基线：`0f40ffbc2dfd11db3e8ac72faa23ac3643b699b9`
- 集成基线：`965e5d848ea017d8ed0b29a9963b83af58fe2b44`
- 独立 worktree：`/tmp/projection-main-0f40ffbc`、`/tmp/fix-projection-965e5d84`
- 两边使用同一 fixture：`auction_ticks = 0/3/6`、`seed = 42`、20 tick/日，
  每拍执行真实 `step()` 与 `save()`。
- 两边分别用独立 `CARGO_TARGET_DIR` 构建探针；探针源码保存在
  `agents/fix-projection-regression/zz_projection_probe.rs`，不进入 Cargo 测试集。
- 比较粒度：每个 tick 的 `events`、`snapshot`、`save` 分别落盘；先做严格 JSON
  路径比较，再用原始紧凑字节做摘要与内容比较。

## 实测摘要

| auction_ticks | main 锚（main 实测通过） | 集成 actual（失败日志一致） | 删除两项新表示后的集成摘要 |
|---:|---:|---:|---:|
| 0 | 14515314912140190070 | 7902364060655783278 | 14515314912140190070 |
| 3 | 1528640566612864764 | 13359828032896992400 | 1528640566612864764 |
| 6 | 571153393436025933 | 4756588382933877897 | 571153393436025933 |

逐字段结果：

- `events`：60/60 tick 逐字节相同。
- `snapshot`：60/60 tick 逐字节相同。
- `save`：60/60 tick 仅有以下差异：
  - `civil_clock.policy.official_coverage`（main `[]`，集成两条 2026 覆盖）；
  - `civil_clock.policy_digest`（main 缺字段，集成 `e39b4ae1f109313d`）。

将集成紧凑 tuple 中的 `official_coverage` 数组替换为 `[]`、删除
`,"policy_digest":"<16 hex>"` 后，三个完整投影的 FNV 摘要与 main 三锚逐位相等。
因此不存在其他字段、字段顺序或事件字节差异。

## 语义核验

- 官方覆盖来自 `4978223f fix(engine): 补齐沪深 2026 官方休市覆盖`；取证见
  `agents/calendar-2026-official-20261102/evidence.md`。SSE 原文为上证公告
  〔2025〕45号，SZSE 为深证会〔2025〕481号，均于 2025-12-22 发布；规范化正文
  SHA-256 分别为 `3ecade6151ea19fe223b747eea08212be430f69ae7a52056f1961b2087e939bc`
  与 `87ecf6071c1aa57eb6dc70327e7d763481976300079d622739b27766b789535f`。
- 保存 digest 来自 `86d9e2e1 fix(engine): 绑定存档日历政策摘要`。按生产
  `CalendarPolicy::compute_content_digest` 的相同部件顺序，从落盘 policy 独立重算：
  - main policy -> `34ff926274f28f77`（与冻结旧档白名单一致）；
  - 集成 policy -> `e39b4ae1f109313d`，与落盘 `policy_digest` 完全一致。
- `step_skeleton` 新增断言直接调用公开的 `CalendarPolicy::from_parts` 重算
  `content_digest`，并检查覆盖项为 `(Sse, 2026)`、`(Szse, 2026)` 规范顺序，
  来源标识非空、来源摘要为 64 位十六进制、区间有序。
- 2026 官方日期、来源绑定、2026 外模拟年份不变的完整取证与红绿记录见
  `agents/calendar-2026-official-20261102/evidence.md`。

## 验证

- `cargo fmt --package engine -- --check`：通过。
- `cargo test -p engine --test step_skeleton --features verification-harness --no-run`：
  构建通过（进程外 300 秒上限）。
- 已构建 test binary 直接执行，`--test-threads=7`：
  7 passed / 0 failed，2.37 秒（进程外 10 秒上限）。
- `git diff --check`：通过。

## 范围说明

本改动只更新 `packages/engine/tests/step_skeleton.rs` 的当前表示锚，并补充对新增
存档字段的语义断言；未修改生产代码、未删除或弱化任何原有断言。旧锚值、历史
取证说明和 repeatability/seed/tick-order 断言均保留。
