# correction 当前 raw fixture 的 calendar 漂移修复

## 范围与基线

- 任务：核对集成树 correction 当前 raw fixture 是否因后续 calendar `policy_digest` 变更失效；若失效，在独立 worktree 从集成 HEAD 用当前 producer 重生成，并同步必要测试与证据。
- 集成 HEAD：`3ee968e7`（`docs(engine): 登记旧档日历兼容降级边界`）。
- 独立 worktree：`.worktree/calendar-fixture-integration-20261103`，分支 `codex/calendar-fixture-integration-20261103`。
- 集成树 `.worktree/branch-convergence-final-20261102` 未改动；历史原件 `apps/web/src/save/fixtures/correction/rust-completed-save.json` 未改动。
- 未 push。

## 只读核对结论

集成 HEAD 上直接执行 producer 绑定用例，先取得红灯：

```sh
node scripts/run-with-deadline.mjs 10000 -- \
  target/debug/deps/engine-3eb2aa70a85e274e \
  session::company_corrections_tests::current_completed_correction_fixture_matches_producer_and_restores \
  --exact --test-threads=1
```

结果：`FAILED`，失败点 `assert_eq!(generated, fixture)`，信息为“当前 producer 输出漂移；必须重新生成夹具，不能手改 JSON”。

结构化比较旧 fixture 与当前 producer 输出，除两项外完全相同：

- 新增 `/civil_clock/policy_digest = "e39b4ae1f109313d"`（commit `86d9e2e1` 起新档必写）。
- `/civil_clock/policy/official_coverage` 从 0 条变为 SSE/SZSE 2026 各 1 条（commit `4978223f` 补齐官方休市覆盖）。

明细见 `evidence/fixture-delta.txt`。两条 coverage 的 `source_digest` 分别等于取证记录的 SSE/SZSE 规范化正文摘要，各含 13 个 `closed_ranges`。

## 改动

- `packages/engine/tests/fixtures/correction/current-completed-save.json`：由当前 producer 用例经 `SMG_WRITE_CURRENT_CORRECTION_FIXTURE` 重生成，未手改 JSON。新档 143935 bytes，SHA-256 `88db89bb78a2599fbb9f1caf1407d06e6d0a3b2719ea35e72dbc943a49a3176c`。
- `apps/web/src/save/schema/company/closing.test.ts`：同步 raw fixture 的绑定 SHA。
- `apps/web/src/save/fixtures/correction/README.md`：同步字节数、SHA，并注明当前档携带的 `policy_digest` 与 2026 official coverage；历史原件保持原字节。
- 本目录：重生成、跨语言回读与 Web 消费证据。

历史原件仍为 356888 bytes，SHA-256 `92127f8a150787c3d9ec64f44211117b1f1435027ed643bfa779d908dd35b6bd`；Web 测试继续断言当前契约拒绝其缺少 `runtime_sync_policy`。

## 验证

编译与执行分开。构建使用 `CARGO_BUILD_JOBS=128`、进程外 300000ms deadline：

```sh
node scripts/run-long-validation.mjs 300000 -- cargo test -p engine --lib --no-run
```

结果：`Finished test profile in 2m 03s`，生成 `target/debug/deps/engine-3eb2aa70a85e274e`。

| 验证 | 命令要点 | 结果 | 日志 |
|---|---|---|---|
| Rust producer 重生成稳定 | `SMG_WRITE_CURRENT_CORRECTION_FIXTURE=/tmp/regenerated-calendar-fixture.json` + exact 用例 | 1 pass，`cmp` 与入库档逐字节相同 | `evidence/rust-regenerate.*` |
| Rust fixture 读/恢复 | exact 用例，`run-with-deadline.mjs 10000` | 1 pass / 0 fail，0.18s | `evidence/rust-producer-readback.*`（含 `--nocapture`） |
| Web→Rust 回读 | `agents/.../web-roundtrip.mjs` 生成 Web 输出，再以 `SMG_CURRENT_CORRECTION_WEB_ROUNDTRIP_SAVE` 执行同一 exact 用例 | 1 pass，输出 `CURRENT_CORRECTION_WEB_ROUNDTRIP_OK` | `evidence/rust-producer-readback.*` |
| Web raw 消费 | `run-with-deadline.mjs 10000 -- node --test --test-timeout=10000 ... closing.test.ts` | 9 pass / 0 fail | `evidence/web-closing.*` |
| 空白检查 | `git diff --check 3ee968e7..HEAD` | exit 0 | 终端结果 |

关键哈希：

- 当前 raw fixture 输入：`88db89bb78a2599fbb9f1caf1407d06e6d0a3b2719ea35e72dbc943a49a3176c`（143935 bytes）
- Web 回写输出：`4048324c0036d2ab5a75417d64cc1a83ae7095efe858561db8eec6878c4a57fa`（143931 bytes）
- 历史原件：`92127f8a150787c3d9ec64f44211117b1f1435027ed643bfa779d908dd35b6bd`

## 语义与限制

- 本批只重生成 producer 输出并同步绑定摘要；fixture 仍是同一 `a-share-simulation` 场景（沪市 `600101`、`t1_enabled=true`、`lot_size=100`），未改变交易制度、费用、涨跌幅或会计金额语义。
- 新增的 2026 official coverage 来自已取证并复核的交易所公告；fixture 只如实携带默认政策与 content digest。
- 未运行完整 Rust/Web 回归、浏览器验收或 simulation acceptance；本次只验证受影响的 producer/read/restore 与 Web raw consumer 路径。
- 当前 fixture 是 Rust 测试 producer 输出，不是用户真实操作产生的存盘。
