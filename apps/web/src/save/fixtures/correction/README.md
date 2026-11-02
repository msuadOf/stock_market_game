# completed Correction SaveSlot fixtures

## 历史原件 `rust-completed-save.json`

`rust-completed-save.json` 是 E2 历史共同窗口的完整原始 SaveSlot，356888 bytes，SHA-256 为 `92127f8a150787c3d9ec64f44211117b1f1435027ed643bfa779d908dd35b6bd`，逐字节复制，未改字段、数字 token、键序或换行。

来源是 E2 独占树的 `agents/perf-intraday-independent/implementation/e2/correction-production-green-01/actual-completed-save.json`。真实 producer 为同一新 unit 的 `session::company_corrections_tests::correction_day_end_completes_at_real_eighteen_and_restores_exact_identity`，product-010 完整通过后才提取唯一 marker；unit SHA 为 `f897ce27f4a1e457a26a0b5012007dc6cdc093edace2dd4610445e98bace6af7`，source 为 E2 冻结 961 项。原窗口 session95868 实际 WAIT1，约214.1695s，虽然两 build 与最终23个 Rust case 通过，但 Web strict consumer 在 closing.restatements source 字段失败，**整个原窗口永久失败，未执行 Rust Web 回读**。

本原件的非空完成操作为 `session-correction-1`；公司0的 `finance.closing.restatements` 含 BusinessEventId 裸数 `900000001`。真实 Rust `RestatementRows=Vec<(ScopeId,Vec<(BusinessEventId,AccountingPeriod)>)>`、透明 u64 BusinessEventId 不带字符串 codec；Web 使用安全整数 number 共同域，超 MAX_SAFE 显式拒绝，不能宣称 Rust 完整 u64 与 JS 同域。会计期间仍为 YYYY-MM，未改变任何元/分金额。

该原件产生于 `runtime_sync_policy` 成为当前存档必填字段之前，`setup` 中没有该字段。当前 main 的 Web `parseSaveJson` 与 Rust `deserialize_saved_setup`（已有测试 `session::runtime_sync_tests::current_save_requires_explicit_runtime_sync_policy`）都会显式拒绝缺少该字段的 setup；Web 测试只固定原件 SHA 与这一拒绝行为，不再在测试内补字段后冒充当前完整 SaveSlot。

分支 `fix/web-correction-restatement-2026-10-28` 的 `real-wire-v2` 证据（提交 `11ff9010`，2026-10-28）确实用这份原件和旧 producer 二进制 `f897ce27…` 完成了当时契约下的 Web strict 消费、Rust serde 与 normal restore。它早于 2026-10-29 引入 `runtime_sync_policy` 的契约变更，因此只能作为旧版本 codec/wire 的历史证据，不能证明当前 main 的 raw Web→Rust reader/restore。

## 当前代表性短 fixture

当前契约使用 `packages/engine/tests/fixtures/correction/current-completed-save.json`：

- 143935 bytes，SHA-256 为 `88db89bb78a2599fbb9f1caf1407d06e6d0a3b2719ea35e72dbc943a49a3176c`。
- 由当前 main 的真实 `GameSession` producer 生成：同一更正受理与 18:00 日终完成流程，`runtime_sync_policy="every_tick"`，完成表非空；仅为缩短测试时间把 Simple 前史缩为 6 期、市场历史缩为 1，不拼装存档字段。
- 该档的 `civil_clock` 携带当前默认政策的 `policy_digest="e39b4ae1f109313d"`，以及 SSE/SZSE 2026 各一条 `official_coverage`；历史原件 `rust-completed-save.json` 保持原字节不变。
- `packages/engine/src/session/company_corrections_tests.rs::current_completed_correction_fixture_matches_producer_and_restores` 每次都重新生成并逐字节比较该 fixture，随后走 `decode_save_slot` 与 `GameSession::restore`。producer 或 schema 漂移时测试失败，必须重新生成 fixture，不能手改 JSON。显式重生成命令见下文。
- `apps/web/src/save/schema/company/closing.test.ts` 直接读取这份 raw 字节，不再注入 `runtime_sync_policy`；历史原件另有独立测试固定 SHA 并断言当前契约拒绝缺少该字段。
- 设置 `SMG_CURRENT_CORRECTION_WEB_ROUNDTRIP_SAVE` 指向 Web `parseSaveJson` 后的回写文件时，同一 Rust 测试还会执行完整 `SaveSlot` 反序列化、`decode_save_slot`、normal restore 与 completed status 相等，并输出 `CURRENT_CORRECTION_WEB_ROUNDTRIP_OK`。该跨语言回读是独立验收证据，不由普通单测自动宣称。

验证入口（编译与执行分开，执行受 10000ms 进程树 deadline）：

```sh
node scripts/run-long-validation.mjs 300000 -- cargo test -p engine --no-run
node scripts/run-with-deadline.mjs 10000 -- target/debug/deps/engine-<hash> session::company_corrections_tests::current_completed_correction_fixture_matches_producer_and_restores --exact --test-threads=1
node scripts/run-with-deadline.mjs 10000 -- node --test --test-timeout=10000 --test-isolation=none --test-concurrency=1 apps/web/src/save/schema/company/closing.test.ts
```

producer 或 schema 变更后，确认 diff 符合预期，再用显式环境变量重写 fixture（该模式仍会执行 read/restore 断言）：

```sh
SMG_WRITE_CURRENT_CORRECTION_FIXTURE=packages/engine/tests/fixtures/correction/current-completed-save.json \
  node scripts/run-with-deadline.mjs 10000 -- target/debug/deps/engine-<hash> session::company_corrections_tests::current_completed_correction_fixture_matches_producer_and_restores --exact --test-threads=1
```

这些 fixture 只覆盖存档 wire 与恢复边界；没有改变现行 A 股交易制度、费用、涨跌幅、T+1 或会计金额语义。
