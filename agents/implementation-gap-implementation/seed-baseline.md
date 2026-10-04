# G20 / G21：新局 seed 与 baseline CLI setup 投影

## 依据与边界

- 已阅读 `AGENTS.md`、`docs/principles.md`、`docs/testing.md`、`docs/error-handling.md`、`docs/architecture.md`、`docs/open-questions.md`、ADR-0005、ADR-0019、ADR-0023，以及总账和宿主/工具复核对应 G20/G21 原文。
- ADR-0005 §4 明确新局从熵取种、测试固定注入、存档保存 RNG；ADR-0023 明确仅生成虚拟前史，运行行情来自游戏实际撮合。本批不读取真实行情，不声称真实市场校准。
- `docs/price-volume-simulation-gap-checklist.md` §7 明确 CLI 只读取 `SessionSetup`，不恢复或验证无关快照、委托、账户事实。正式 `SaveSlot` 恢复校验不变。
- 本批不改变 A 股交易规则、单位、板块差异或 T+1。CLI 仍使用 `SessionSetup.validate()` 拒绝 T+0 等非法配置，无需新增交易制度解释。

## 实现范围

- `apps/web/src/config/session-seed.ts`：通过 `crypto.getRandomValues` 读取两个 uint32，使用 bigint 组成完整 u64。允许测试注入固定熵；零 seed 合法。能力缺失/调用失败显式抛错，保留 cause，不使用墙钟、`Math.random` 或固定 seed fallback。
- lifecycle / App 接线由 `implement_ui_contracts` 所有，本 agent 不修改其文件；已交接生产新局调用 helper、固定 E2E seed、存档 seed 原样恢复的要求。
- `packages/engine/examples/price_volume_baseline.rs`：serde 仅反序列化 `SetupProjection`，未知字段跳过；要求合法 JSON、唯一且有效的 setup，再做领域校验。报告与 causal runs 都使用该 setup 创建新会话。
- `docs/price-volume-simulation-gap-checklist.md`：只修正 CLI 所需的 `simulation-diagnostics` feature 参数，不扩大文档范围。

## TDD 与短测证据

所有日志均位于 `.tmp/seed-baseline/`，未运行长模拟、真实行情校准或全量回归。

- `seed-red.log`：保留旧固定 seed 行为的 helper 测试红灯，3 个 case 因 seed 精度、重复熵读取和显错断言失败；非 import/编译失败。
- `seed-green.log`：seed 3 case + defaults 7 case，共 10/10 通过，Node 内部约 86ms。单 case 10000ms、整命令 `run-with-deadline.mjs 10000`、concurrency 4。
- `seed-green-parallel.log`：取消 `--test-isolation=none`，两个真实 Node 子进程并行，`--test-concurrency=4`；runner 以文件级汇总为 2/2 通过，约 213ms。相同 10000ms case/整树门禁。
- `seed-types.log`、`seed-lint.log`：定向 TypeScript 类型检查与 oxlint deny-warnings 通过；每条命令受 `timeout 10s` 约束。首轮定向 tsc 从根目录执行缺少 Node 类型定位、第二轮遇 TS6 要求 `--ignoreConfig`，均调整启动参数后通过，不改变代码规避检查。
- `baseline-red-build.log`：`flock /tmp/stock-market-gap-cargo.lock` 下 Cargo `-j16`、target `.tmp/gap-target`、独立 300000ms 构建 deadline，41.51s 完成。构建与测试耗时分开。
- `baseline-red.log`：预编译 binary `timeout 10s ... --test-threads=8`，4 case 中 3 通过、1 按预期失败：合法 setup 因旧 `SaveSlot` 要求 `schema_version` 被拒绝。
- 首次 `baseline-green-build.log` 被其他并行实现的临时不完整状态阻断：`SessionSetup.company_operations` 未存在和 `DecisionReason::PersonalAnalysis` 非穷尽；未修无关代码，已报告 root，等待稳定后重编。
- 两处修齐后再次重编，仍被并行暂态阻断：retail analysis 的 `failure_influence` 借用/Result 接口及 `DayEndDisclosureCtx.groups` 等 6 个编译错误。已通知 root 与对应 owner，不把编译失败报告成 CLI 绿灯；root 可在批次源码稳定后复用同一 Cargo 命令和预编译 binary 短测。
- 共享生产入口稳定后最终重编：`timeout 300s flock /tmp/stock-market-gap-cargo.lock cargo test -p engine --features simulation-diagnostics --example price_volume_baseline --no-run --target-dir .tmp/gap-target -j16`，20.47s 完成。`baseline-green-build.log` 为这次成功构建日志，前两次失败的具体诊断仍如实保留于本记录。
- 成功 binary 复制到 `.tmp/seed-baseline/price-volume-baseline-tests`，避免后续共享 target 重编覆盖。`baseline-green-list.log` 明确包含 5 个 case（不是零 case 或过滤掉新增测试）。`timeout 10s ... --test-threads=8` 运行 5/5 通过，harness 0.01s，详见 `baseline-green.log`。
- 独立 reviewer 建议的额外边界已纳入同一短 case：无关 `schema_version` / `runtime_state` 的畸形值不参与验证；`setup` 内非法 policy、超范围自然日、错误类型 `company_operations` / `groups` 显式拒绝。新的可选配置未被当作无关存档字段跳过。
- `rustfmt --check` 与所拥有文件 `git diff --check` 通过。

## 门禁状态

Rust 绿灯已取得；`review_seed_baseline` 最终独立门禁 PASS，包含最后 `groups:false` 增量、成功构建日志核对与独立运行五 case（5/5 通过，0.01s），详见 [seed-baseline-review.md](seed-baseline-review.md)。G20 lifecycle 接线由 UI owner 与 `review_ui_contracts` 独立处理。本记录不核销总账，也不宣称完整宿主验收通过。无 stage、commit、push 或总账改动。
