# Q23 交易日盘中月报公开修复记录

- 基线：`main = 0f40ffbc2dfd11db3e8ac72faa23ac3643b699b9`
- 独立 worktree：`/data1/baiyifan/workplace/stock_market_game/.worktree/q23-intraday-monthly-publication`
- 分支：`codex/q23-intraday-monthly-publication`
- 独立 target：`/tmp/stock-q23-target`
- 未推送；完整回归未运行。

## 缺陷与语义边界

修复前 Simple 的 `prepare_disclosures` 只在 `end_civil_day` 调用。自定义月报即使排期在交易日 10:30，也要等自然日日结才新增公开材料；查询接口还把可见时点固定为当日 00:00。这不是“公开时点回填”，而是实际延迟。

本次只实现 Q23 已批准的游戏内月报语义：交易日盘中按排期在私有 tick candidate 公开；休市日、收盘后的材料仍由自然日日结处理。没有新增或改写 A 股法定披露制度，没有把游戏月报冒充法定报告。

## 实现

1. `session/intraday_disclosures.rs`
   - 只处理 `CompanySystem::Simple` 与 `ReportFrequency::Monthly`。
   - 在私有 candidate 上调用既有 `prepare_disclosures`，窗口上界是真实市场观察时点。
   - 无新公开时不安装候选、不推进披露游标。
   - 串行路径按实际公布时点调用既有 `deliver_public_information`，并生成真实 `CompanyDisclosurePublished`。
2. `pipeline/candidate_commit.rs`
   - 在 CommitTick 前把盘中月报安装进同一个私有 tick candidate。
   - 事件稳定键使用已分配的真实 `seq`，跨拍保持 Session 身份单调。
   - post-shadow/commit 前失败时，candidate 连同 library、disclosure cursor、个人 information/belief 一并丢弃。
3. `runtime_mirror.rs` + `runtime_progress.rs`
   - native 默认 runtime 在私有 mirror candidate 中准备同一月报候选；成功后随唯一安装点进入 Session。
   - `event_bridge` 对 `CompanyDisclosurePublished` 使用事件 seq 作为稳定局部身份，避免每拍从 0 重编号。
4. runtime INFO 链
   - `failure.rs` 在成功安装后，对本次新公开或已到期非即时 cadence 调用既有 `RuntimeDriver::deliver_information`。
   - `runtime/group.rs` 把 INFO 投递更新登记到跨 Start 的 `belief_pending_dirty`，使下一拍 Seal 分片真实上送 Session。
   - `RuntimeDriver::deliver_information` 增加“同一 library 版本但 cadence 到期”的补投门；未到期不冒充获知。
   - `decision_chain/roots.rs` 把 `Monthly` 纳入报告种类分类，避免机构观察把月报当公告分类。
5. 查询与观察钟
   - `query_public_reports` / `public_report_by_id` 使用真实观察时点，不再固定当日 00:00。
   - 最后市场 tick 后、自然日日结前，已完成当日 session 的观察时点保持 15:00，不回退 9:15。
6. 存档游标
   - Monthly Simple 允许盘中已公开游标落在“上一日终 ≤ cursor ≤ 存档 tick 观察时点”。
   - 未来游标仍以 fault2106 显式拒绝；恢复后不重复发布。

## TDD 红绿

### 初始红：10:30 实际无公开

实现前运行新测：

```text
running 1 test
test session::company_source_runtime_tests::simple_monthly_report_is_public_at_intraday_tick_and_restore_does_not_republish ... FAILED
thread ... panicked at packages/engine/src/session/company_source_runtime_tests.rs:393:10:
10:30 交易日必须有真实公开事件
test result: FAILED. 0 passed; 1 failed
```

### runtime 11:00 获知红

临时移除 INFO 投递的跨 Start pending 标记后，实际运行：

```text
$ /tmp/stock-q23-target/debug/deps/engine-3eb2aa70a85e274e \
  --exact session::company_source_runtime_tests::runtime_publishes_at_1030_and_due_subscriber_learns_at_1100 --test-threads=1
...
thread ... panicked at ...:
11:00 到期后现有 INFO 链必须真实获知月报
test result: FAILED. 0 passed; 1 failed
```

证据：`evidence/runtime-1100-red.log`

### 绿

- `evidence/runtime-1100-green.log`：10:30 公开、11:00 真实获知，1/1 通过。
- `evidence/runtime-restore-1100-green.log`：10:30 保存后恢复，11:00 获知且不重复发布。
- `evidence/info-failure-poison-green.log`：INFO 失败 terminal/poison。
- `evidence/q23-session-tests-green.log`：21 项普通 Session 短测全绿、2 项显式性能 gate ignored，5.20s。
- `evidence/default-roster-scan-perf.log`：20,000 accounts、15,300 次缓存命中 `initial_scan=1.29ms`、`cached=473µs`。
- `evidence/production-floor-cache-perf.log`：真实 GameSession 20,000 名册，`scan=1.09ms`、`15300 O(1) hits=187µs`、`invalidate+rescan=1.01ms`。
- `evidence/runtime-restore-1100-red.log` / `runtime-restore-1100-green.log`：10:30 保存后恢复、11:00 cadence 到期获知的红绿证据。
- `evidence/info-failure-poison-green.log`：INFO 安装后失败 terminal/poison 证据。
- `evidence/check-lib-green.log`：非 test `cargo check -p engine --lib` exit 0；只证明 native 生产 lib 可编译，WASM/release 未在本批运行。
- `evidence/build-final.log`：独立 target `(lib test)` 构建成功。

新增测试覆盖：

1. 10:30 公开、查询可见、存档恢复后不重复。
2. 提交前失败回滚 library/information/business hash，并用同夹具控制组证明该拍本来会公开。
3. 串行订阅机构在 10:30 真实获知并写入 `used_report_ids`。
4. native 默认 `step()` 在 10:30 公开，恢复后不重复。
5. 最后交易 tick 后、civil 日结前仍可按 ID/列表读取。
6. 存档披露游标晚于当前观察时点时以 fault2106 拒绝。
7. native runtime 10:30 发布、11:00 cadence 到期获知。
8. 10:30 保存后恢复：11:00 到期仍真实获知，且恢复续跑期间不重复发布。
9. 20:00 日终公开、保存、恢复、次日 11:00 到期获知。
10. INFO 投递在公开安装后失败：显式 poison/terminal，首错透出，不重试重复发布。
11. DayEnd / OnDemand 非 EveryTick policy 下 10:30 仍真实公开并可查询。
12. 跨批 disclosure EventStableKey 使用全局 seq，连续两批被 SessionFactCursor 接受。
13. Monthly 真实 runtime `step_frame` 投影通过 validate，TickFrame 携带公开事件。

## 相邻回归

以下命令均直接运行已构建 test binary，外部 10s deadline，显式并发：

- `session::company_source_runtime_tests`：21/21 普通项，5.20s，4 threads；2 项性能 gate 单独显式运行。
- `runtime::drive_tests`：55/55，9.10s，4 threads。
- `runtime::drive::typed_info_delivery_tests`：2/2，1.04s，2 threads。
- `runtime::event_bridge::disclosure_identity_tests`：1/1。
- `session::persistence::`：70/70，2.83s，8 threads。
- `session::runtime_sync_tests`：21/21，0.73s，4 threads。
- `session::runtime_mirror`：10/10，2.68s，4 threads。
- `session::notices::tests`：10/10，0.99s，4 threads。
- `session::protocol::company_reads::tests`：11/11，1.45s，4 threads。
- `runtime::group::admin_transaction::information_tests`：28/28，0.50s，4 threads。
- `session::protocol::civil::session::single_owner_tests`：8/8，1.93s，4 threads。
- `session::decision_chain::retail_analysis::tests`：6/6，0.31s，4 threads。
- `runtime::group::admin_transaction::information_tests`：28/28。

探索性整模块 `session::decision_chain` 运行 12.48s，超过普通测试命令 10s 上限，因此不作为门禁证据；只把其中相关子套件按 10s 内单独运行。

构建：
- 非 test 生产检查：`cargo +1.96.1 check -p engine --lib -j 8`，exit 0。
- test 构建：`CARGO_TARGET_DIR=/tmp/stock-q23-target CARGO_BUILD_JOBS=8 node scripts/run-long-validation.mjs 300000 -- cargo +1.96.1 test -p engine --lib --no-run -j 8`。首次约 2m21s，后续增量约 20–60s，均在独立 target 和 300s 外部 deadline 内。

## 正式规则登记

`docs/trading-rules.md` 已新增“游戏额外月报与盘中排期（2026-11-03，Q23）”：明确月报是游戏额外报告、不是 A 股法定定期披露；登记预设/自定义/延迟、交易日盘中 candidate、休市/收盘后日终、单月同比不年化与 Simple 限定范围。

## 限制与未声称

- 未运行完整 engine/workspace 回归、WASM 浏览器 E2E、三宿主完整验收或发布 fixture 验证。
- INFO 投递发生在 tick 安装之后；若该步骤失败，按现有 runtime terminal/poison 语义显式终止，不静默吞错。月报公开本身仍与其他公司材料在同一个私有 mirror/candidate 中准备和安装。
- 恢复档只有在库内存在本局生命周期内真实公开材料（`published_at >= start_date`）时才把当前 library 版本设为已投递基线；纯前史播种或仅推进游标的日结不冒充驱动已投递。恢复后 11:00 cadence 与日终恢复路径均已有真实短测。
- INFO 投递在 tick 安装后执行；该阶段失败按现有 runtime terminal/poison 契约显式终止，首错透出且禁止重试重复发布。已在测试中真实注入并验证。
- 性能优化：月报候选增加轻量预过滤，无到期未公开月报时不克隆全库；INFO cadence 使用最早到期时点缓存，默认 20k 名册下缓存命中每拍仅 O(1)，跨过最早时点才扫描一次。没有非即时 cadence 账户时使用远期哨兵，避免每拍重复扫描。
- 未实现或宣称额外交易制度；月报仍是游戏额外报告。
