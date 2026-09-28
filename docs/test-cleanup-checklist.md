# 测试清理执行清单

> 性质：本轮测试冗余审查的执行记录；候选不等于已确认可删。
> 建立日期：2026-09-27。每项完成时记录实际改动、替代覆盖、验证结果和独立复核结论。

## 执行约束

- 逐项核对当前代码和调用点，再改动；删除错误测试前，确认其声称保护的当前行为已有真实覆盖。不能为消除红灯而删测试或弱化断言。
- 每项尽量形成独立批次。新增或改变行为时按 `docs/testing.md` 做红—绿—重构；纯删除也需运行受影响层的相关测试。普通测试单 case 与单命令均不得超过 10 秒，Node 测试须同时有 10000ms case timeout 与进程树 deadline。完整回归属于共享 300000ms 长验收，按现有 runner 的多进程/多线程预算运行。
- 每批完成后，由**未实施该批的 subagent**审查完整 diff：大 A 语义及依据、必要性与最小范围、边界测试和跨层契约；修复有效发现后再复核。删除测试不得改变 T+1、申报单位、价格限制、交易阶段、资金/股份单位或错误展示。纯测试清理无新增交易制度依据；一旦触及制度实现，先查官方现行规则并记录日期。
- 开工时工作树已有其他未提交改动，涉及 `apps/server/tests/api_contract.rs`、`packages/engine/tests/strategy.rs` 等候选文件。实施者须先核对归属与当前 diff；按 `docs/git/daily-workflow.md`，不得覆盖、暂存或提交他人的改动，无法区分时暂停该文件的操作。
- 负责分组：`verify_web` 负责 01/07/11；`verify_engine` 负责 02/03/05/09；`verify_infra` 负责 04/06/08；`cleanup_mobile` 负责 10。各组提供证据，由清单维护者统一更新状态。
- 用户先决定“先修改完，后面再统一编译”。现已完成本地正式 attempt4 的构建与完整回归；远端当前 head 的浏览器 E2E 仍待验证，不能据此宣称第 10 项全部完成。
- 状态：`待核对` → `进行中` → `已实施、待验证与独立复核` → `已实施、待验证` / `已验证、待独立复核` → `完成`；不成立的候选记为`保留`并说明证据。每项的方框仅在验证与独立复核均通过后勾选。

## 逐项清单

### 01. 伪宿主一致性测试

- [x] **完成（目标短测与独立复核通过）。** `apps/web/src/host/host-adapter-parity.test.ts` 的四宿主同协调器自比较测试已移除；真正的宿主协议/回滚覆盖保留。
- 验证：相关 5 个文件 0.69 秒通过；整 Web Node 56 文件、8 shard、289/289 通过（下同）。Astra 已复核 `baseline`/协议更新、序号和错误路径覆盖。

### 02. 测试辅助类的参数校验

- [x] **完成（Rust 回归与独立复核通过）。** `packages/engine/tests/strategy.rs` 中约 1273 行的参数校验只到达测试内辅助类，已删除；生产策略构造器、工厂的非法参数测试保留。
- 验证：本地正式 attempt4 的 Rust 完整回归通过；Astra 已复核现行策略输入校验。

### 03. 新旧路径自比较

- [x] **完成（Rust 回归与独立复核通过）。** `packages/engine/tests/strategy.rs` 已去掉两入口同源的自比较，以独立预期保留 `u64::MAX` 市场分钟边界，并保留同一分钟不同宿主 tick 密度测试。
- 验证：本地正式 attempt4 中 `drift_up_saturates_max_market_minute` 通过；Astra 已复核市场分钟口径。

### 04. 重复的建局和健康检查

- [x] **完成（Rust 回归与独立复核通过）。** `apps/server/tests/api_contract.rs` 的 `engine_setup_roundtrips_json` 与 `healthz_still_ok` 已删；真实 API 建局、错误响应和专门健康检查测试仍覆盖对应行为，最小 setup fixture 保留。
- 验证：本地正式 attempt4 的 server 测试纳入 Rust 完整回归并通过；Astra 已复核建局凭据和错误状态覆盖。

### 05. 基线夹具重复字段断言

- [x] **完成（example 定向测试与独立复核通过）。** `packages/engine/examples/baseline_fixture.rs` 的重复场景字段断言已删；逐字段输入漂移检测和未知场景显式拒绝保留。
- 验证：正式 attempt4 的 69 个 Rust 测试二进制未包含此 example，自带测试另以 harness 4/Rayon 4、10 秒外部门禁执行 4/4 通过（0.01 秒）；显式构建 1.27 秒。日志见 `.tmp/merge-main-local-validation/targeted/baseline-fixture-{build,green}.log`。Astra 已复核压缩 300 tick 与完整交易日的区分。

### 06. Web 测试文件数量硬编码

- [x] **完成（目标短测与独立复核通过）。** `scripts/run-web-tests.test.mjs` 的固定 `files.length >= 54` 门槛已移除；非空、排序、去重、后缀和代表性发现行为断言保留，runner 自身零测试拒绝保留。
- 验证：runner 测试与实际 Web 测试发现；独立复核不会因发现路径回归而静默少跑。记录：待填。

### 07. 已停用的同步存档仓储及旧包装兼容

- [x] **完成（目标短测与独立复核通过）。** `apps/web/src/save/save-repository.ts` 的同步旧仓储及本地存储裸 JSON 兼容已移除；生产 JSON **文件**导入保持原样。压缩入口补充 unsafe numeric seed 与 numeric `turnover_cents` 显式拒绝覆盖，旧 schema 拒绝测试保留。
- 验证：目标 4 case 0.28 秒通过；整 Web Node 289/289 通过。Astra 已复核存档错误边界与 A 股金额单位。

### 08. 禁用的 `before` 历史采集器

- [x] **完成（目标短测与再次独立复核通过）。** `scripts/simulation/baseline-run.test.mjs` 中约 1687 行起的 `captureBaseline` 测试及 `baseline-run.mjs` 对应采集实现：旧采集实现及 11 条专属测试已删除，新增一条 `before` CLI 明确拒绝测试；改动前该新测试已通过，不将其记作红灯。独立审查发现合法历史报告中的 `engine_error_events=5` 接受覆盖遗漏，现已补断言并修正注释；密封历史证据解析、来源/完整性校验已再次复核。
- 验证：simulation 脚本相关测试，必要时 K7 解析验收；独立复核历史证据没有被改写或假冒为当前基线。记录：待填。

### 09. 修复前重放哈希证明

- [x] **完成（Rust 回归与独立复核通过）。** `packages/engine/tests/extraction_replay.rs` 中“反改时间戳复原旧哈希”的测试已从当前回归移除；当时的历史证据与 learnings 记录保留，不改写旧结论。
- 验证：当前重放、保存恢复和市场时钟测试随本地正式 attempt4 Rust 完整回归通过；必跑的跨年边界 ignored case 另行实际运行 1/1 通过。Astra 已复核确定性、事件顺序和恢复语义。

### 10. 移动端源码文本匹配

- [ ] **已实施且独立复核通过；Node 全测通过，待 E2E。** `apps/web/src/mobile/mobile-layout-spec.test.ts` 原有 20 条（先前 19 条为误计）：已移除纯样式源码匹配，迁移能由 SSR/浏览器行为验证的断言；保留 5 条暂无法无侵入替代的源码守卫，分别保护 fatal、telemetry、capabilities、K reset 与 desktop 单位接线，不宣称已全部迁完。手/股单位、成交额口径和交易时间轴由新行为覆盖核对。
- 验证：定向 Node 两文件 8/8、0.87 秒；整 Web Node 289/289 通过。均满足 10000ms case 与进程树门禁；SSR 增至 3 case，补了 intraday/K 线数量。新增 E2E 待用户决定的统一编译后运行。Astra 首轮发现的隐藏 global toggle 路径与 desktop 格式接线覆盖遗漏已修复并再次复核通过。

### 11. 大型旧存档测试夹具

- [x] **完成（Node 全测与独立复核通过）。** `apps/web/src/save/mature-save-test-fixture.ts` 及使用者已改用 66,522 B 的当前 schema **parser 投影夹具**和 7,118 B 的公司片段；测试不再动态加载 8.48 MB 旧档或即时 upgrade。该投影只用于既有结构解析断言，裁剪后存在 report company、scheduler receivable 和个人报告 ID 等悬空跨域业务引用，**不保证 Rust 业务恢复成功**，也不作为可恢复整档证据。旧 `task29-save.json.gz` 是密封来源引用，原封保留在历史证据中，**不再由测试加载**，并未删除；其 SHA-256 `b6adb18db572faabd1ab24f76cd818152e65f6fb1cab46800c5ae083f9462738` 已核对。
- 验证：相关 6 个目标测试文件 0.84 秒、整 Web Node 289/289 通过；原有结构 parser 断言、错误路径、非空分支金额等覆盖保留。Astra 已复核完整代码、夹具限制注释、密封档哈希与本清单边界。

### 12. 旧版兼容层与密封语料适配栈（2026-09-28 新批次，用户确认全清）

- [ ] **已实施，待独立复核与全量门禁。** 三笔提交：
  1. 引擎测试脚手架（`test(engine)`）：删 `plan_npc_working_orders`/`plan_npc_working_orders_from_index`/`prepare_pre_open_tick` 三个仅测试可达的绕路入口，14 个对账测试与 7 处盘前调用改走生产入口（断言零改动）；两个"旧实现当标准答案"自对照测试（`npc_quote_expiry_uses_acceptance_quote_…`、`capture_advances_attention_…`）改为独立推导的显式期望（受理时盘口输入、夹具常量、队列不变量）；随之删除失去调用者的 `build_self_views_for`、`evaluate_attention_candidate{,_with_signal}` 测试脚手架；清理指向已不存在兼容桥的过时注释与命名。
  2. 引擎语料投影面（`refactor(engine)`）：删 `examples/escrow_corpus_replay.rs` 与 `verification_evidence.rs` 中唯一消费者即该 example 的语料投影族（`CorpusProjection`/`project_corpus_surface`/`project_controlled_sell_corpus`/`CorpusSurfaceInput`/`ControlledSellInput`/`SaveRestoreLiveOrderInput`/`legacy_sell_reservation` 及全部专属辅助，约 2100 行 src + 1347 行测试）；保留 phase tracking、conservation snapshot、observation、update projector 与 `validate_update_transition`；删失去调用者的 `CorpusEvidenceMismatch`/`InvalidSaveV2` 错误变体。harness capture 报告的 `corpus_projection` 键保留为固定 null 占位（无 `skip_serializing_if`，删除会改变现行 JSON 键集）。
  3. 脚本侧密封语料兼容层（`chore(test)`）：删 `escrow-corpus-adapter.mjs`、`escrow-corpus.mjs`、`escrow-corpus-exact.mjs`、`escrow-corpus.test.mjs`、`escrow-corpus.md`、`prepare-escrow-performance-baseline.mjs`、`fixtures/escrow_performance_baseline_adapter.rs`（v1 policy-id 兼容改写）；`escrow-verification-contracts.mjs` 删 `compareCorpusCase`/`verifyStressCorpus`/`verifyEvidenceBundle`/`main`/`normalizeUpdates` 及仅被它们使用的辅助，保留 current-run 校验（artifactReceipt/validateObservation/verifyDeterminismMatrix/verifyPerturbationGate/verifyConservationSnapshot）；`run-escrow-task9-matrix.mjs` 剥离不可再生的 evidence 装配模式（corpus-diff/perf-report/verification-bundle CLI 与 summary.complete_evidence），保留 `validatePerformanceReport`（校验现行 escrow-perf-report-v3，harness 仍在生成）；`prepare-escrow-performance-config.mjs` before 侧改用 `escrow_performance_endpoint`（baseline 语义 = 任意当前代码 checkout，两侧均须 v2 setup）。
- **边界声明**：`.omo/evidence/` 密封语料与历史 bundle 数据文件原封保留，但**不再有可执行代码能复验**（用户已确认接受）；存档 v1 拒绝语义仍由 `persistence/v2_tests.rs` 直接覆盖；K7 `baseline-run.mjs` 解析层不在本批范围。
- 验证：引擎定向套件全绿（reconciliation 16、pre_open 12、authoritative_tick 8、adaptive_plan_chain 28、decision_snapshot 14、attention 10、b1 19）+ `cargo test -p engine --lib` 776/776 + verification_evidence 27/27；`--features verification-harness` 与 `escrow_performance_endpoint` example 编译通过；Node 侧 contracts 9/9、matrix 12/12、source-manifest 2/2、performance-harness 12/12（需 canonical TMPDIR）、prepare-performance 1/1，均双 10 秒门禁。全量 `pnpm lint`/`pnpm test` 与独立 subagent 复核进行中。

## 单独发现，不并入本轮 11 项

`scripts/**/*.test.mjs` 未由根测试和 CI 常规入口统一覆盖。它是测试入口范围问题，需单独盘点执行时间、10 秒门禁和 CI 策略；本轮不借测试清理顺手扩大 runner 或 CI 改动。

## 批次记录

| 批次 | 条目 | 实施与替代覆盖 | 验证命令/结果 | 独立审查者、发现及复核 | 状态 |
|---|---|---|---|---|---|
| engine | 02/03/05/09 | 四个目标 case 已调整；`strategy.rs` 同期外部并发新增/格式变化不计入本批 | 早期预编译被本轮前 `ledger_validation` 语法错误阻断（`.tmp/test-cleanup-2026-09-27/validation/engine-build.log`）；后续本地正式 attempt4 Rust 完整回归通过；05 另以 10 秒外部门禁定向执行 example 自带测试 4/4 通过（0.01 秒，显式构建 1.27 秒） | Astra 已通过独立复核 | 完成 |
| infra/server | 04/06/08 | 04 删重复建局/健康检查；06 删固定数量门槛；08 删旧采集路径和 11 条专属测试、增 1 条 CLI 拒绝测试（该测试改前已绿）；按审查补合法报告 `engine_error_events=5` 接受断言 | 06/08 两个目标 Node 测试文件在双 10 秒门禁下分别 0.19/3.68 秒通过，`git diff --check` 通过；04 随本地正式 attempt4 Rust 完整回归通过。额外 `verify-k7-root.test.mjs` 为 10/11，CLI stderr 捕获异常待排查（`.tmp/test-cleanup-2026-09-27/validation/verify-k7-root-workspace-tmp.log`），正式完整回归不覆盖此脚本用例 | Astra 已通过 04/06/08 独立复核 | 11 项内已完成；额外 K7 失败单列待查 |
| mobile | 10 | 20 条旧源码用例精简至 5 条必要守卫，部分转 SSR 与新增 E2E | 定向 Node 两文件 8/8、0.87 秒；整 Web Node 289/289 通过，新增 E2E 待统一编译 | Astra 首轮发现的 E2E toggle 路径与 desktop 格式接线遗漏已修复，再次复核通过 | 待 E2E |
| web 宿主/存档 | 01/07 | 删除伪 parity、同步旧仓储与本地裸 JSON 兼容；文件导入未改；补压缩入口非法数字拒绝 | 宿主相关 5 文件 0.69 秒、存档目标 4 case 0.28 秒、整 Web Node 289/289 通过 | Astra 已通过独立复核 | 完成 |
| web 夹具 | 11 | 66,522 B 当前 schema parser 投影与 7,118 B 公司片段替换动态旧档 upgrade；投影只供结构解析，悬空业务引用意味着不保证 Rust 恢复；密封历史文件原封留档且测试不再加载 | 目标 6 文件 0.84 秒、整 Web Node 289/289 通过；原结构解析与错误断言保留 | Astra 完整代码与清单边界复核通过 | 完成 |

整 Web Node 验证日志为 `.tmp/test-cleanup-2026-09-27/validation/web-full-node.log`：56 个文件在 8 个真实 shard 中运行，128 CPU、进程预算 8；全批 749ms、最慢 shard 712ms，case 与进程树均受 10000ms 限制。该命令不包含编译或浏览器 E2E。

本地正式 attempt4 已在每阶段 300000ms 外部期限内完成：构建 30.076 秒，执行 111.906 秒；69 个 Rust 预构建测试二进制合计 1862 通过、0 失败、6 忽略，其中必跑的跨年边界 ignored case 单独执行 1/1 通过（4.374 秒）；workspace doctests 4.612 秒，Web Node 293/293 通过（0.950 秒）。构建前后源指纹同为 `2f514bd548813605d098532601d48f0ad758ec369efd1818d10909a3f4695ec7`。正式记录见 `.tmp/merge-main-local-validation/attempt4/result-summary.json`、同目录的 `build.log` 与 `execute.log`；fmt、clippy、typecheck 也已通过。此结果覆盖包含保存 seq/cursor 缺陷修复的本地正式源状态；该修复不并入本清理清单的 11 项范围。第 10 项浏览器 E2E 仍待远端当前 head 验证。
