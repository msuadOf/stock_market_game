# correction SaveSlot 证据漂移调查与修复

## 范围与基线

- 任务：修复 main 上 `apps/web/src/save/fixtures/correction/rust-completed-save.json` 与当前必需 `runtime_sync_policy` 的证据漂移，并准确区分 2026-10-28 `real-wire-v2` 旧证据。
- 基线：main `0f40ffbc2dfd11db3e8ac72faa23ac3643b699b9`。
- 隔离：worktree `.worktree/branch-audit-correction-20261102`，分支 `codex/branch-audit-correction-20261102`。未改动 `fix/multi-group-roster-offset` 及其 WIP，未 push。

## 调查结论

1. 历史原件 `rust-completed-save.json` 为 356888 bytes、SHA `92127f8a150787c3d9ec64f44211117b1f1435027ed643bfa779d908dd35b6bd`，`setup` 中确实没有 `runtime_sync_policy`。
2. 当前 main 的 Web `parseSaveJson` 与 Rust `deserialize_saved_setup` 都把 `runtime_sync_policy` 作为当前存档必填；Rust 已有 `session::runtime_sync_tests::current_save_requires_explicit_runtime_sync_policy`。
3. 旧测试在读取历史原件后插入 `"runtime_sync_policy":"every_tick"` 再解析；这只覆盖 correction codec，不能证明该文件是当前完整 SaveSlot。
4. 分支 `fix/web-correction-restatement-2026-10-28` 的 `real-wire-v2`（提交 `11ff9010`，2026-10-28）确曾对历史原件执行 Web 消费与旧 f897 binary Rust 回读。但 `runtime_sync_policy` 契约于 2026-10-29 引入（`cfd61680`/`a8b7905f`），所以该回读是旧版本证据，不能声明当前 main 闭环。
5. 本地临时调查时曾运行当前 main 的 `correction_day_end_completes_at_real_eighteen_and_restores_exact_identity` 并比对 `B4E_COMPLETED_SAVE_JSON` marker：移除 `setup.runtime_sync_policy` 后结构值与历史原件相同（356888 + 34 = 356922 bytes）。该次 marker 原始文件未归档，故只作为调查笔记，不进入正式证据链，也不把历史原件升级成当前契约证据。

## 实现

- 保留历史原件和其 SHA，不改任何字节；Web 测试新增“当前契约拒绝缺少 `runtime_sync_policy`”断言。
- 新增由当前 Rust producer 生成的短档 `packages/engine/tests/fixtures/correction/current-completed-save.json`：142501 bytes，SHA `daeee7e4de1cdc1b1ea9a192f7dc82e315a3fb383310c18451c77577a35a6de1`。
- 新增测试 `packages/engine/src/session/company_corrections_tests.rs::current_completed_correction_fixture_matches_producer_and_restores`：同一真实 `GameSession` 更正受理与 18:00 日终流程（Simple 前史 6 期、市场历史 1），逐字节比较 fixture，并执行 `decode_save_slot` 与 `GameSession::restore`。
- `apps/web/src/save/schema/company/closing.test.ts` 直接读取该 raw 档，不再注入字段。
- 跨语言回读通过新 env `SMG_CURRENT_CORRECTION_WEB_ROUNDTRIP_SAVE` 单独执行；与旧的全量 producer 测试 env 分离，避免两个测试互相误读。
- 文档已同步到 `apps/web/src/save/fixtures/correction/README.md`，明确历史证据/当前证据的边界。

## 验证证据

| 验证 | 命令要点 | 结果 | 日志 |
|------|----------|------|------|
| Rust 编译 | `run-long-validation.mjs 300000 -- cargo test -p engine --no-run` | exit 0 | `evidence/engine-build.*` |
| Rust producer/read/restore 与 Web→Rust 回读 | `run-with-deadline.mjs 10000 -- engine-3eb2a… current_completed_correction_fixture_matches_producer_and_restores --exact`，含 `SMG_CURRENT_CORRECTION_WEB_ROUNDTRIP_SAVE` | 1 pass + `CURRENT_CORRECTION_WEB_ROUNDTRIP_OK` | `evidence/rust-readback.*` |
| 旧全量 producer 测试未回归 | 同 binary exact `correction_day_end_completes_at_real_eighteen_and_restores_exact_identity` | 1 pass | `evidence/rust-old-producer.*` |
| fixture 显式重生成 | `SMG_WRITE_CURRENT_CORRECTION_FIXTURE=/tmp/regenerated-...` 指向临时文件 | 1 pass，重生成 SHA 与已入库 fixture 相同 | `evidence/rust-generator.*` |
| Rust 拒绝缺 policy | 同 binary exact `current_save_requires_explicit_runtime_sync_policy` | 1 pass | `evidence/rust-policy.*` |
| Web raw 消费 | `node --test … apps/web/src/save/schema/company/closing.test.ts`，外 deadline 10000 | 9 pass / 0 fail | `evidence/web-closing.*` |
| 相关 Web | `runtime-sync-policy.test.ts`、`company-schema.test.ts`、`state.test.ts` | 3/2/14 pass | `evidence/web-runtime-sync-policy.*` 等 |

关键哈希：

- fixture（输入）：`daeee7e4de1cdc1b1ea9a192f7dc82e315a3fb383310c18451c77577a35a6de1`
- Web 回写输出：`00e6f3506e1ef460c8e22967db4ad101ea781108fc07a9bf637445f5df6b0228`（142497 bytes）
- 历史原件：`92127f8a150787c3d9ec64f44211117b1f1435027ed643bfa779d908dd35b6bd`

## 独立复核

未实施本改动的独立 subagent `/root/fixture_fix/independent_review_2` 已只读复核完整 diff、哈希与证据，结论 **PASS，无阻断发现**：

- A 股语义未变，新 fixture 仍为 `a-share-simulation`、沪市 `600101`、`t1_enabled=true`、`lot_size=100`；更正为 `CashRevenue` 会计事实。
- 改动必要且最小，生产代码未改。
- 历史 fixture blob/SHA 未变；新 fixture 由当前 producer 序列化后逐字节绑定并执行 `decode_save_slot`/restore；Web 直接读 raw bytes，无字段注入；旧 `real-wire-v2` 已限定为 2026-10-28 旧契约证据。
- 非阻断意见“缺少直接重生成命令”已通过新增 `SMG_WRITE_CURRENT_CORRECTION_FIXTURE` 环境变量入口并实跑验证。原始 producer marker 对比未归档的限制已在调查结论中明确标注。

## 限制

- 本次没有重跑旧 `real-wire-v2`；也不改写其 2026-10-28 结论。旧证据继续只证明当时契约，当前闭环由本目录新证据证明。
- 未执行完整 Web 全量、browser 或 Rust full regression；已验证受影响 Rust 测试与相关 Web 测试。
- 新 fixture 是当前 Rust 测试 producer 输出，不是用户真实操作产生的存盘；它用于固定当前契约的 raw wire 与恢复边界，不声称用户旅程录像或真实资金事实。
- 未改变现行 A 股制度、金额、费用、T+1、涨跌幅或会计语义。
