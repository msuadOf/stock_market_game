# build tooling 独立复核

日期：2026-10-03。复核者未参与本批实现。结论：**独立静态复核通过，无未解决有效发现；B1 已撤销为误报，B2 已补测试源码并复核关闭。Rust 运行验证仍由 root 安排。**

## 范围与证据

已审查相对 `b89afb3346743a4b4fccf26c9ac9ff108595f696` 的完整九文件 diff、相关实现与测试调用链，并阅读全文 `hosts/build/actions.json` 的六动作及 `hosts-R2-E01` extension、`hosts/build/status.md`。文件为：

- `packages/engine/examples/escrow_verification_harness/runtime.rs`
- `packages/engine/examples/escrow_verification_harness/committed.rs`
- `scripts/build-targets.mjs`
- `scripts/desktop/build-matrix.mjs`
- `scripts/desktop/build-matrix.test.mjs`
- `scripts/simulation/baseline-run.mjs`
- `scripts/simulation/run-escrow-verification-matrix.mjs`
- `scripts/simulation/run-escrow-verification-matrix.test.mjs`
- `scripts/simulation/audit-diagnostic-divergence.test.mjs`

读取依据：`AGENTS.md`、`docs/principles.md`、`docs/testing.md`、`docs/trading-rules.md`、`docs/architecture.md`、`docs/open-questions.md`、ADR-0017 与 ADR-0027。未运行 Cargo、真实构建、真实 matrix、长验收或任何 Git 写操作；未修改生产源码。首次实施状态中记录的 46 个短测通过、3 个 sandbox EPERM 和 rustfmt 通过是实施者证据，不能写作复核者重新执行或 Rust 测试通过。第二轮重新审查最终九文件完整 baseline diff，以及新增 Writer 与 witness 测试全文。

## 已撤销的误报

### B1 — 已撤销：复用 PASS 时遗漏 negative-control typed witness 判定

首次复核误认为 `scripts/simulation/run-escrow-verification-matrix.mjs:734` 的 `compare_artifacts(..., true)` 不再直接检查 negative-control witness，即会接受非法 witness 为 PASS。该结论不成立，不能计为 P1 或有效发现。

2026-10-03 父代理与实施者指出现有调用链后，复核者重新读取全文确认：`validate_reusable_pass:726` 在 artifact 比较前调用 `validateExecutorEvidence(capture, expected[index])`；该 validator 的 `:479–482` 已检查 `negativeControlDetected(capture)` 与 `disabled_merge` 一致性，并在失败时抛出 `NEGATIVE_CONTROL_NOT_DETECTED`。基线中同样已有该 guard，所以旧比较分支里的第二次 predicate 检查是冗余条件。

复核者首次遗漏完整 validator 调用链，过早将局部删除条件描述为端到端回归；首次未执行 fakeHarness 复现。非法 witness 即使重算 receipt，也会在既有 validator 处失败，先前关于接受 PASS 的描述撤销。保留本节记录纠错历史。

不应恢复重复检查或修改既有错误 code。若补重新计算 receipt 的测试，应归为验证现有门禁的 characterization，不能称为 B1 修复。

复核者独立运行新增 `rejects a forged negative-control witness even when its capture receipts are recomputed`：1/1 PASS，case 171ms，Node runner 总 212ms；使用 `run-with-deadline.mjs 10000`、`--test-timeout=10000`、`--test-concurrency=4`、`--test-isolation=none`，临时 TMPDIR 在工作区 `.tmp` 下，退出后清理。该测试将 capture.json、capture-receipt.json 和 summary entry receipt 全部重算，真实负控 artifact 差异保持不变；明确断言复用 FAIL/NEGATIVE_CONTROL_NOT_DETECTED 且零 child。产品 validator、guard 与错误 code 未为此修改。

## 有效发现

### B2 — P2，已关闭：E01 要求的 Writer 落盘、已有目录与部分失败测试未落实

位置：`packages/engine/examples/escrow_verification_harness/runtime.rs:605`、`:636`、`:805`；动作依据为 `hosts/build/actions.json` 中 E01 的 `missing_tests`。

本批把四个硬编码写点改为遍历 `CaptureArtifact` collection，但新测试只检查内存 bytes、receipt 和 report 投影。仓库 `rg` 显示 `write_bundle`/`CaptureArtifactWriter` 只有实现与真实 harness caller，没有 Writer 行为测试；Node matrix 中 fakeHarness 自己写 capture 文件，并未经过 Rust Writer。extension 明确要求落盘字节、写失败路径和新目录拒绝仍由 Writer 验证，因此本项不能算已覆盖。

修复：使用短临时 fixture 直接调用 Writer/write_bundle，核对四个文件 bytes 与 report receipt、一致的 capture receipt，检查已存在目录拒绝；在 `event-stream.json` 等指定位置建立目录阻断写入，验证显式错误、原顺序中已写文件保留与后续文件尚未产生。保持原有允许部分输出的契约，不引入 staging 或新写入 owner。即使本轮禁止 Cargo，也应补齐测试源码并诚实记录未运行，留给上层安排有界 Rust 验证。

第二轮实施者补入 `runtime.rs:927` 的 `capture_writer_persists_exact_bytes_and_receipts_and_refuses_existing_output` 与 `:964` 的 `capture_writer_keeps_prior_files_and_stops_later_files_on_partial_write_failure`。复核者逐行读取确认：短 DTO fixture 不执行市场，不声称 runtime acceptance；临时目录在注册 TMPDIR 的 `.tmp` 后代创建，Drop 清理。前者检查真实落盘 report pretty bytes、四个 artifact bytes/name/长度/hash、capture receipt，以及拒绝既有输出后 report 未变；后者在 event-stream.json 放置目录阻断，检查明确带文件名的写错误、先前 capture/receipt/state 已写、save-slot/receipts 未写。Writer 生产逻辑与顺序未改变，未新增依赖。测试源码缺口已闭合；本 reviewer 未执行这两个 Rust 用例，运行结果不能写为通过。

## 三项门禁回答

1. **A 股语义与依据**：本批仅移动 harness、构建与测试生命周期，不修改权威成交、实际受理时间优先、资源 envelope、现金/股份单位、T+1、费用或 SaveSlot 序列化。普通与 negative-control 同用 `capture_artifacts`，`after_failed_step` receipt 在 move 前计算；report 仍由 BTreeMap 投影，Writer 保留 `capture → capture receipt → authoritative state → event stream → save slot → receipts` 顺序。现行领域依据沿用 trading-rules 中 2026-09-22/25/27 记录及其官方来源，不新增交易制度，未重新在线核验全部官方条款。重新核对确认 fresh/reuse 均检查 typed rejection/rollback/control witness，B1 不构成语义漂移。
2. **必要性与最小范围**：六动作与 E01 均在分配范围内；BuildRun 真实拥有 direct/supervised 的临时目录，prepared worker 借用且不清 Cargo cache；Publisher 拥有 staging/rename。WorkspacePathPolicy 持 canonical 路径，CaptureArtifact 持 bytes/name/receipt，Writer 唯一写盘。SimulationBatchContext 组合一次运行的 deadline/permit/pool/source identity，保留有限 after 顺序和 sensitivity 分组。EscrowVerificationRun 持本次比较状态与请求身份，未新增通用继承或交易实体对象。Desktop 继续 plain route，由同源 IDs 驱动说明与实际 probe，没有 wrapper class。DiagnosticAuditRepositoryFixture 真实拥有临时 Git repo、创建即登记、失败立即清理，产品断言仍留在测试闭包。未发现越范围实现或新增依赖。
3. **边界、跨层漂移、复杂度**：B2 是动作明列的边界测试遗漏，已补源码并复核关闭。canonical `.tmp`/traversal 校验、共享 worker/permit 预算、每 child 后 source fingerprint、最终发布失败复用拒绝、dry-run 不 probe 与 unsupported 平台边界在 diff 中保留。B1 已撤销且独立短测证明现有 guard；未发现其他必须修复的复杂度问题。三个 CLI sandbox 失败和 Rust 未验证仍须保留限制记录，不能替代为通过。

## 后续

独立 diff 门禁已完成，无未解决有效发现。Rust 编译/用例和原三个 CLI sandbox 被阻测试的运行证据由 root 补齐，本报告不宣称真实构建或完整验收完成。

## 最终版本绑定

以下 SHA-256 由复核者在第二轮完整九文件 diff 读取后实际计算；基线仍为 `b89afb3346743a4b4fccf26c9ac9ff108595f696`。

| 文件 | SHA-256 |
|---|---|
| packages/engine/examples/escrow_verification_harness/runtime.rs | c82e88edca6e4e2a50be8de7e9cc2091fe7a54fff3c700903f830ddf0a26e95a |
| packages/engine/examples/escrow_verification_harness/committed.rs | 43e8e3c7b90e7d12f5d219961e35b412472bc9082612810adab091e5f3e42f44 |
| scripts/build-targets.mjs | c24c2a0b1dd32f20e144b0ad6f750909c7116c2fdcd7df939919f9b6d329607d |
| scripts/desktop/build-matrix.mjs | c8c9f5dc8b0e0362da2a58be612e3915734bd17f1eb66ecc8e1a7c94e130c0a5 |
| scripts/desktop/build-matrix.test.mjs | d7505427b688464f5803cf5b3cab3ad3f5fa256158e4750f9566e22cd9a8bb27 |
| scripts/simulation/baseline-run.mjs | 7c554df1538e797920ab2f6f537e477d9f46a63fc3aea66f88f07ed80f4c9041 |
| scripts/simulation/run-escrow-verification-matrix.mjs | 8488754fa80a5ac32496f143f361adc0f6af0ff2035ce39663011f8920701b8b |
| scripts/simulation/run-escrow-verification-matrix.test.mjs | cd8d8ecb51cab50dc7ff810a12495f4e4b49f69a7707b6f77aa162dc0f0a5340 |
| scripts/simulation/audit-diagnostic-divergence.test.mjs | 0afdaf64ac355eb46f80e2d933293ecc5a35182130b8ee518b898d77223ac37d |
