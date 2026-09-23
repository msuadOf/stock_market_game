# Issues — escrow-parallel-engine

Problems and gotchas encountered during work on this plan.

_Auto-scaffolded by /start-work. Append new entries below - never overwrite._

## 2026-09-19 #9 验证与范围说明

- LSP 对所有变更 Rust 文件均超时；以 Rust compiler、Clippy、focused tests、库测试、preservation verifier 和 Node verifier tests 取证，未声明 LSP clean。
- 全量 `cargo test -p engine` 在 600 秒工具时限内已通过所有已完成 target，但命令在后续 target 尚未完全结束时超时；不能声称完整命令 exit 0。已单独通过 `cargo test -p engine --lib`（143/143），其余先前输出中已完成的 integration targets 均绿。
- 独立审查确认 #9 窄变更的 buyer reservation、seller share reservation、persistence 和 decision-chain 路径一致；同时指出 worktree 含既有 P0/P1/protocol 改动和一个非 #9 restore assertion 删除，均非本里程碑新增且不在此处理。

## 2026-09-19 #9 gate repair

- `cargo fmt --all -- --check`、完整 `cargo test -p engine`、strict Clippy、preservation verifier、WASM 和 diff-check 已在修复后通过；LSP 仍超时，未声明 LSP clean。

## 2026-09-19 P0/P1 未完成占位回滚

- 已删除无 envelope hydration、receipt、守恒或测试的 P0/P1 placeholder modules 与 wiring；恢复 P0/P1 compatibility tokens，并让 TickShadow bridge 再次完整执行 legacy tick（含连续竞价报价过期）。这不是 P0/P1 实现，后续必须从 ledger hydration 的独立原子里程碑重新开始。

## 2026-09-19 TickShadow / P9 验证限制

- Rust LSP 对本轮所有 changed Rust file 的诊断请求均在 30 秒超时；未声称 LSP clean，改以 compiler、143+ engine tests、Clippy、fmt 和 preservation verifier 取证。
- 首次 full engine 命令在 120 秒时卡在既有长运行 auction test 后被工具终止；600 秒重跑完成并全绿，4 项 release stress 测试按既有 `ignored` 标记未运行。
- test-only `StoredStrategy::NonAuthoritative` 不能经 sealed `StrategyState` 导出；production shadow 必须 fail closed。为保持既有 fixture 行为，`cfg(test)` 单独保留其 legacy 执行桥，不能被用作 production authority 或 save/hash 路径。

## 2026-09-19 Continuous cancellation seam verification limits

- Rust LSP timed out for `session.rs`, `session/continuous_cancellation.rs`, and `session/continuous_cancellation_tests.rs`; no LSP-clean claim. Focused default/diagnostic tests and strict Clippy passed.
- `cargo test -p engine` exceeded the 600-second tool limit after all completed targets shown in output, including the 107-test session target. It is recorded as a timeout rather than an exit-0 full-suite pass.
- Independent review found substantial unrelated pre-existing TickShadow/envelope/seller-reservation work in the dirty `session.rs` diff. This slice did not modify, revert, or claim those changes.

## 2026-09-19 Atomic P0 expiry verification limits

- Rust LSP timed out on all changed P0 Rust files. Compiler, focused default/diagnostics P0 suites, 167-test library suite, and strict Clippy passed.
- The post-change `cargo test -p engine` run exceeded the 600-second tool deadline during later targets after all displayed targets passed; it is not reported as a new exit-0 full-suite result.

## 2026-09-19 P0 stale lifecycle repair

- Temporary `dbg!` instrumentation was used once to capture the stale lifecycle state and was removed before final verification. Rust LSP continued timing out; final compiler, full engine exit-0, strict Clippy, and focused runtime tests are authoritative.

## 2026-09-19 P0 no-expiry checkpoint verification limit

- Rust LSP timed out for changed P0 Rust files. Full engine exit-0, feature-specific focused tests, strict Clippy, formatter, preservation, WASM, and diff checks passed.

## 2026-09-19 P1 allocation snapshot verification limit

- Rust LSP timed out for changed P1 Rust files. Full engine exit-0, default/diagnostic focused tests, strict Clippy, formatter, preservation, WASM, and diff checks passed. Test-only non-authoritative strategy fixtures intentionally retain their legacy bridge and do not receive a production P1 snapshot because no owned shadow authority exists.

## 2026-09-19 P1 cutoff fixture scope

- No persisted prospective soft-budget grant or pending sale-proceeds state exists in `GameSession` at P1. The corrective isolation test therefore uses real `pending_plan_events`; it does not claim to prove invariance to an un-stored soft-budget value. Rust LSP timed out; compiler/test/Clippy evidence is authoritative.

## 2026-09-20 NPC reconciliation planner verification limit

- Rust LSP timed out for every reconciliation-touched Rust path. Full engine exit-0, strict Clippy, focused and semantic tests, preservation, WASM, and diff checks passed. The planner/executor seam is verified only as a legacy compatibility extraction; real P2 consumption remains out of scope and blocked on later ordered seams.

## 2026-09-20 NPC generation extraction rollback

- The incomplete NPC generation module was removed after a real `E0599` compiler failure for its nonexistent reconciliation helper. Rust LSP timed out after recovery; cargo check, full engine exit-0, strict Clippy, formatter, preservation, WASM, and diff checks passed. No NPC-generation API remains for P2 to consume.

## 2026-09-20 NPC preparation rollback

- The compiling post-strategy helper was deliberately removed because the required dedicated TDD characterization and duplicate/skip-parent toggles were not completed. Rust LSP was not rerun after this final rollback; the compiler and full quality gates passed.

## 2026-09-20 NPC decision batch seam verification limits

- Rust LSP diagnostics timed out for `session.rs`, `npc_generation.rs`, and `npc_generation_tests.rs`; `cargo check`, focused tests, strict Clippy, and rustfmt are the available compiler-quality evidence.
- The attempted full `cargo test -p engine` exceeded the 600-second runner limit after many displayed targets passed, so it is recorded as a timeout, not an exit-0 full-suite result. Dedicated complete pre-extraction projection parity and requested temporary regression-toggle receipts were not completed in this execution.

## 2026-09-20 NPC decision batch seam verification closure

- Supersedes the preceding NPC seam full-suite/parity/toggle limitation: `cargo test -p engine` exited 0 with a 1,200-second allowance; parity and all three temporary mutant proofs are recorded in `task-3/npc-decision-seam-20260920.md`. Four existing release-only scale/stress tests remain intentionally ignored.
- LSP still timed out for `session.rs`, both direct seam files, and the parity test module. No LSP-clean claim is made; compiler, full suite, strict Clippy, formatter, preservation gates, WASM threading, and diff check passed.

## 2026-09-20 Player candidate capture verification limit

- Rust LSP diagnostics timed out for `session.rs`, `player_candidates.rs`, and `player_candidates_tests.rs`; no LSP-clean claim is made. Focused capture/parity tests, the full engine suite, strict Clippy, simulation-diagnostics check, formatter, preservation gates, WASM threading, and diff check provide the available evidence.

## 2026-09-20 Plan-chain candidate seam verification limit

- Rust LSP diagnostics timed out for the new candidate module, its tests, the plan execution adapter, and `session.rs`; no LSP-clean claim is made. Compiler, focused/full engine tests, strict Clippy, diagnostics build, formatter, preservation verifier, WASM threading, and diff check are the authoritative evidence.

## 2026-09-19 Todo 3 verifier trust-root repair

- 独立 reviewer 指出 inventory JSON 自身可变，不能作为 baseline SHA 或 Class-A expected hash 的信任根。已改为从 sealed attempt-12 `manifest.json.preserved_test_baseline_sha` 绑定 baseline，并从该 commit 运行时重算 A symbol hash；inventory 中不一致值输出 `RECLASSIFIED`。

## 2026-09-19 lexer verifier repair

- Oracle 实测旧 verifier 可将 tuple comma、identifier token boundary 和 block-comment brace 误判为机械 Result 适配，且 B 未进入 body classifier；旧 PASS 已 supersede。现使用 Rust lexical tokens 比较完整 item，只删除精确 healthy Result suffix，并加入 B future semantic scope guard。

## 2026-09-19 B verifier closure

- Class-B verifier 完成 4 个 sealed symbols/9 assertions 的 raw-anchor parity；future B current assertion 需在 `class_b_changes` 以 sealed identity/anchor 和 current lexical hash 明确声明。Oracle session `ses_f4a1bd599ffeIxOg6hPxIRaA08` final PASS。

---

## Todo 2A 修复：临时 worktree 清理

四个 Todo-2/baseline detached worktree 曾残留，其中两个含有未提交或未跟踪内容。清理前必须逐一读取状态并确认路径名称、HEAD 和内容均属于 Todo 2 临时审查或基线采集；确认没有目标采集进程后，才可使用 `git worktree remove --force` 并执行 `git worktree prune`。不在目标清单内的 `rbw-task1-clean` 保留，不能仅凭 detached 状态删除。

---

## Todo 1 verification evidence

### Planned checks (original attempt, corrected)

Command: `ls docs/decisions/0017-escrow-parallel-tick.md; grep -n "Status: accepted" docs/decisions/0017-escrow-parallel-tick.md; grep -n "决策记录" docs/decisions/0017-escrow-parallel-tick.md; grep -c "^|" docs/decisions/0017*.md; grep -c "seal_allocation_snapshot" docs/decisions/0017*.md; grep -c "business_state_hash" docs/decisions/0017*.md; grep -c "事件变体表" docs/decisions/0017*.md; grep -c "封顶" docs/decisions/0017*.md; grep -n "托管" docs/trading-rules.md; grep -n "简化" docs/trading-rules.md`

The original transcript incorrectly claimed that the localized status line matched the exact English grep. The factual result was that it did not match, so this transcript is corrected rather than reused as evidence:

```text
grep -n "Status: accepted" docs/decisions/0017-escrow-parallel-tick.md
no matching output; exit code 1
```

Command: `if ! grep -E "真实清算|交易所清算机制" docs/trading-rules.md | grep -v 简化; then printf 'failure QA PASS: no unqualified real-clearing claim\\n'; else printf 'failure QA FAIL: unqualified real-clearing claim found\\n'; exit 1; fi`

Observed output:

```text
failure QA PASS: no unqualified real-clearing claim
```

Command: `git diff --check`

Observed output: no output, exit code 0.

### Manual Markdown QA

Read artifact: [0017-escrow-parallel-tick.md](/data1/baiyifan/workplace/stock_market_game/docs/decisions/0017-escrow-parallel-tick.md)

- Phase sequence and `seal_allocation_snapshot`: lines 21-32. P0 is the only pre-cutoff release exception, sealed releases wait for the next sealed batch, and P9 makes the new snapshot visible.
- Data-flow and seller fee contract: lines 34-44. Seller charge uses `min(F_after - charged_before, gross_delta)`, with commission → stamp tax → transfer fee ordering, `deliver_cash >= 0`, `spent.cash = 0`, and zero seller cash envelope.
- Event variant table: lines 46-64. The table enumerates the current `session.rs:168-313` variants, maps each to phase rank, entity tag, tagged source, and local index derivation, and marks `SettlementError` as the #5 migration boundary.
- Poison and hashes: lines 66-70. `StepFatal`, panic boundary, `business_state_hash`, `session_state_hash`, and commit-only authority are explicit.
- Nine-item ledger: lines 72-84. #9 is line 84 and states normal-trade non-binding behavior, old/new acceptance divergence, and the simplification/non-clearing boundary.

Independent review: PASS with one medium scope observation. It identified the pre-existing C06/external-calibration section at `docs/trading-rules.md:49-54` as unrelated, but that section existed before this task and was intentionally left untouched.

## Repair entry: 2026-09-17

- Independent AdversarialVerify found that `ResourceLimit` cannot map to `Account(id)`: the current payload at `packages/engine/src/session.rs:283-290` contains only `seq`, `resource`, and `limit`. ADR-0017 now maps it to `Session`/`Session`, with local index from session emission order.
- The prior status transcript was inaccurate because `grep -n "Status: accepted"` did not match `- **状态 (Status):** accepted`. The ADR now includes an exact `Status: accepted` literal while retaining Chinese status wording. Fresh command output is recorded below.

### Repaired verification output

Command: `grep -n "Status: accepted" docs/decisions/0017-escrow-parallel-tick.md`

```text
3:- **Status: accepted**；**状态：** accepted（已接受）
```

Command: `grep -cE '^\\| [1-9] \\|' docs/decisions/0017-escrow-parallel-tick.md`

```text
9
```

Command: `grep -c "seal_allocation_snapshot" docs/decisions/0017*.md; grep -c "business_state_hash" docs/decisions/0017*.md; grep -c "事件变体表" docs/decisions/0017*.md; grep -c "封顶" docs/decisions/0017*.md`

```text
2
2
1
4
```

Command: `git diff --check -- docs/decisions/0017-escrow-parallel-tick.md docs/trading-rules.md docs/open-questions.md .omo/notepads/escrow-parallel-engine`

Observed output: no output, exit code 0.

Manual Read QA: ADR lines 21-32 contain P0-P9; lines 50-65 contain the event table and the `ResourceLimit` Session/Session rationale; lines 34-44 contain the seller cap and deterministic fee order; lines 73-85 contain all nine divergences and #9; lines 67-71 contain poison and both hashes.

### Final independent repair review

Result: semantic PASS. The reviewer confirmed the exact status literal, `ResourceLimit` Session/Session mapping and payload rationale, all nine rows, seller fee-cap semantics, and non-clearing boundary. It reported a scope FAIL for `docs/causal-diagnostics.md` and `docs/diagnostics.md`; both files were already dirty in the pre-repair worktree status and were not edited or reverted in this repair. They remain a pre-existing unrelated worktree observation, not Todo 1 changes.

## Final Session-ordinal repair: 2026-09-17

- Fixed the final AdversarialVerify blocker. `CivilDateAdvanced`, `CompanyDisclosurePublished`, and `ResourceLimit` now all document the same `(phase_rank=6, entity_tag=Session, EventSourceIndex=Session)` shared ordinal domain.
- The shared `local_event_index` is derived from canonical P7/P8 outbox insertion order, with one monotonically increasing index for all phase-6 Session-tagged events. Executor completion order cannot choose it; duplicate keys are `InvariantViolation`.
- Preserved `ResourceLimit` as `Session` because `session.rs:283-290` contains only `seq`, `resource`, and `limit`, with no account ID.

### Final repair verification output

Command: `grep -nE 'CivilDateAdvanced|CompanyDisclosurePublished|ResourceLimit|共享 Session|P7/P8 outbox|重复键' docs/decisions/0017-escrow-parallel-tick.md`

```text
57:| `CivilDateAdvanced` | 6 | `Session` | `Session` | 共享 Session 发出流序 |
58:| `CompanyDisclosurePublished` | 6 | `Session` | `Session` | 共享 Session 发出流序 |
61:| `ResourceLimit` | 6 | `Session` | `Session` | 共享 Session 发出流序 |
65:`CivilDateAdvanced`、`CompanyDisclosurePublished` 与 `ResourceLimit` 共享 `(phase_rank=6, entity_tag=Session, EventSourceIndex=Session)`，因此三者必须共用一个确定性的 Session 发出流序号域，不能分别从各自变体计数。该共享 `local_event_index` 依 canonical phase processing 后 P7/P8 outbox 的插入顺序派生；P7/P8 只按已确定的阶段与实体顺序追加 Session 事件，不由 executor 完成顺序选择。一个单调递增的 index 覆盖全部 phase-6、Session-tagged 事件；任何重复键均为 `InvariantViolation`。
66:`ResourceLimit` 的当前 payload 只有 `seq`、`resource` 和 `limit`，没有 account ID，不能派生出 `Account(id)`；因此该变体必须使用全会话唯一的 `Session` entity tag、`Session` EventSourceIndex，并使用上述共享 Session 发出流序号，不添加产品字段或虚构账户 ID。
```

Command: `grep -nE '自然日推进序|披露发布序|ResourceLimit 发出序' docs/decisions/0017-escrow-parallel-tick.md`

```text
no matching output; exit code 1
```

### Complete final Todo 1 check rerun

Command: `ls docs/decisions/0017-escrow-parallel-tick.md`

```text
docs/decisions/0017-escrow-parallel-tick.md
```

Command: `grep -n "Status: accepted" docs/decisions/0017-escrow-parallel-tick.md`

```text
3:- **Status: accepted**；**状态：** accepted（已接受）
```

Command: `grep -n "决策记录" docs/decisions/0017-escrow-parallel-tick.md`

```text
5:- **决策者 (Deciders):** msuad，依据 2026-09-17 用户决策记录
87:| 9 | 卖单费用实收封顶与零现金预留 | 用户于 2026-09-17 明示裁定：卖单 nominal 费用函数、税率、最低佣金累计口径不变，实际每腿实收总额为 `min(F_after - charged_before, 本腿成交额)`；分项按佣金、印花税、过户费顺序拆分；`deliver_cash >= 0`，卖单 `spent.cash` 和 cash escrow 恒为 0。正常交易在封顶不触发时不受影响。旧预留大于可用现金时旧引擎拒单、新引擎接受；仅旧预留为 0 的卖单适用零现金接受相等断言。差异是游戏内简化，不是交易所清算规则，也不是一般券商规则。 | 用户决策记录：`.omo/drafts/escrow-parallel-engine.md:8-16`；旧实现 `session.rs:1142-1168,2717-2748` |
```

Command: `grep -cE '^\\| [1-9] \\|' docs/decisions/0017-escrow-parallel-tick.md`

```text
9
```

Command: `grep -c "seal_allocation_snapshot" docs/decisions/0017*.md; grep -c "business_state_hash" docs/decisions/0017*.md; grep -c "事件变体表" docs/decisions/0017*.md; grep -c "封顶" docs/decisions/0017*.md; grep -n "托管" docs/trading-rules.md; grep -n "简化" docs/trading-rules.md`

```text
2
2
1
4
26:- 全部未成交委托通过游戏内托管 `envelope` 记录资源占用：买单占用成交额及买方费用预算，卖单占用可卖股份，
29:  按股票汇总的 `reserved_sell_qty`，界面显示的“可用资金/可卖”必须扣除这些占用。该托管 envelope 是游戏内
30: 简化的预留与审计模型，不是交易所真实清算机制。
34:与原逐笔费用结果一致。该规则是游戏内简化，不代表一般券商规则或交易所清算。
```

Command: `if ! grep -E "真实清算|交易所清算机制" docs/trading-rules.md | grep -v 简化; then printf 'failure QA PASS: no unqualified real-clearing claim\\n'; else printf 'failure QA FAIL: unqualified real-clearing claim found\\n'; exit 1; fi`

```text
failure QA PASS: no unqualified real-clearing claim
```

Command: `git diff --check -- docs/decisions/0017-escrow-parallel-tick.md docs/trading-rules.md docs/open-questions.md .omo/notepads/escrow-parallel-engine`

Observed output: no output, exit code 0.

Manual Read evidence: ADR lines 21-32 show P0-P9; lines 57-61 show all three affected rows using identical `6 | Session | Session` dimensions and the same shared Session ordinal phrase; lines 65-67 define one shared monotonically increasing ordinal from canonical P7/P8 outbox insertion order, executor-order exclusion, duplicate-key `InvariantViolation`, and the ResourceLimit no-account-ID rationale; lines 44 and 87 contain the seller cap; lines 50-69 contain the complete event table/prose; lines 79-89 contain all nine divergence rows.

## Todo 2 canonical-order blocker resolved: 2026-09-17

- The event-delivery ambiguity was resolved in ADR-0017 without changing product code, trading rules, open questions, or the plan checkbox. The contract now distinguishes per-tick `TickFrame` computation from transport-only `TickBatch` buffering.
- Canonical ordering uses causal emission order from sealed operation order or canonical P7/P8 outbox append order, never worker completion timing. The shared Session ordinal repair and collision-as-`InvariantViolation` remain explicit.
- The resolution does not add a Stock-first divergence. Independent cross-entity events have no business-order claim, and entity/source/local fields are tie-breakers only after the causal ordinal.

## 2026-09-17 最终协议修订

- 原先把 `OrderAccepted → AuctionTick` 或 `causal/legacy emission order` 当作权威业务顺序的表述已被最终协议取代。帧内 `Event[]` 只是事实集合，数组排列与 `seq` 都不表达业务因果或执行先后。
- 图表和权威状态不得由 `Event[]` 数组顺序重建。`TickFrame.timeseries_payload` 是该 tick 的分时线、竞价和绘图数据来源；现有前端依赖事件顺序的路径仍是后续迁移事项。

## 2026-09-17 历史条目隔离说明

- 本文件前文记录的旧因果顺序探针和旧 canonical ordering 讨论仅是历史问题记录，已由本节及 ADR-0017 最终协议取代。不得把其中的 `causal/legacy emission order`、`OrderAccepted → AuctionTick` 或因果序号解释为现行业务契约。

## Todo 2B-1 证据修复：权威存储能力擦除仍待处理

- 当前 Account.strategy 为 `Option<Box<dyn Strategy + Send + Sync>>`，session 临时决策容器也保存 `Box<dyn Strategy>`；工厂返回的封闭 `ProductionStrategy` 能力在上转型存储时被擦除。

- 此处不会重新开放伪造：普通 `dyn Strategy` 无法传入要求 `dyn ProductionStrategy` 的导出入口。但这是 **2B-2/P2 shadow hydration 的硬前置依赖**，不是已完成的账户状态导出能力。
- 在从权威账户存储提取 `StrategyState` 之前，Account/session 生产存储必须保留 `Box<dyn ProductionStrategy>` 或等价封闭 wrapper；自定义 Strategy 测试注入必须通过独立的仅测试/非权威路径保留。本轮仅修证据，不实施该迁移。

## 2B-2 verification limits

- Desktop-only lost fatal-context blocker repaired; healthy engine-event payload unchanged. No frontend rendering/retry or final Todo-7 protocol claim. Three changed desktop Rust files had LSP timeouts; compiler, strict clippy and all 16 desktop tests passed.

- Storage capability erasure is resolved by StoredStrategy. Full engine tests/clippy/three-host checks and relevant host tests pass; independent scoped re-review PASS.
- LSP timed out. New modules are below 250 pure LOC, but inherited oversized touched files were not broadly split. Extra review tests were not independently red-run; storage-only red was masked by the library target's expected missing-API errors. No full rollback or complete host-failure UX guarantee.

## 2026-09-18 Web protocol CORE verification limits

- `scripts/corepack-pnpm.sh --filter web exec tsc --noEmit` cannot run here because `.nvmrc` requires Node 24.18.0 but the usable system runtime is Node 22.22.1; system Node 25.8.2 is explicitly disallowed for pinned acceptance.
- TypeScript LSP remains unavailable because the server is not installed and installation was previously declined.
- The expanded host test sweep has one pre-existing, out-of-scope failure in `tauri-startup-contract.test.ts`: a source-text assertion expects an obsolete `GameSession::restore` spelling in a Rust desktop actor changed by other work. Web protocol files do not import or modify that actor.

## 2026-09-18 Web protocol core blocker repair limits

- 独立审查复现了三个 parser core 问题：普通对象写入可吞掉 `__proto__`，Map 的类型不同键可能在
  字符串归一化后覆盖，另有 `phase_rank` 和 report revision 宽度及 StockSpec 业务重复校验问题。
  本轮只修协议 core 和 parser 测试，未扩展到适配器/UI。
- 可用本地运行时仍是 Node v24.11.1，不是 `.nvmrc` 的 v24.18.0；Corepack/pnpm 固定门禁仍不能
  如实声明通过。TypeScript LSP 仍因先前拒绝安装而 unavailable。

## 2026-09-20 Plan-chain interpreter verification limits

- Repeated Rust LSP symbols/diagnostics requests timed out; no clean claim. One early company-scenarios combined run exceeded 120 seconds; subsequent complete engine runs exited 0 with 1,200-second allowance.
- New interpreter/quote/consumer modules remain below 250 pure LOC. Inherited decision_chain.rs remains oversized although quote responsibility was extracted. No P3-P6 work, sealed corpus changes, preservation expansion, or Todo 3 completion was performed.
- Review must use the current approved contract: ephemeral continuations must not be serialized; price-memory generation updates are permitted; seller cash reservation remains zero under ADR-0017 #9. Reviewer suggestions to reverse those requirements were retracted, not implemented.

## 2026-09-20 Todo 3 preservation correction diagnostics

- Rust LSP diagnostics for the unchanged `restore.rs` timed out after 30 seconds. TypeScript LSP is unavailable because installation was previously declined. Focused Rust compiler/test and Node verifier evidence are required; no clean-LSP claim is made.

## 2026-09-20 P2 composition execution incomplete

- No engine implementation or new P2 test was delivered in this execution. Only the existing normal-step class-order projection was rerun (one passing test); Rust LSP symbols timed out after 30 seconds. Full gates, mutations, preservation recount, and independent implementation APPROVE remain unperformed.
- The legacy self-view cash-reuse assertion at session.rs::self_view_cash_reuses_reservations_that_will_be_atomically_reconciled must not be overlooked. It pins the legacy helper, not necessarily a separate P1-backed production view. Investigation receipt: task-3/p2-composition-investigation-20260920.md. No owner-decision blocker was established.

## 2026-09-20 Todo 3 production preservation-fixture repair

- The preservation CLI test could false-PASS with `tracked_files_total=1`, `tracked_hunks_total=3`, and `generic_mechanical_hunk_count=3`: it wrote a Class-A mutation whose live-only target had not been overlaid into a detached baseline worktree. The write therefore carried only existing Result/rustfmt changes, which generic normalization correctly classified as mechanical. The repair overlays only the two actually mutated engine test fixtures, requires every replacement and real baseline diff to be nonempty, and leaves verifier implementation, inventory, restore source, sealed artifacts, and plan state untouched.
- JavaScript/TypeScript LSP remains unavailable because installation was previously declined; no clean-LSP claim is made. The Node mutation suite, clean production CLI, focused restore test, rustfmt, and diff check passed. Full receipt: `task-3/preservation-gate-production-fixture-repair-20260920.md`.

## 2026-09-20 Todo 3 subtask C verification limits

- Rust LSP diagnostics timed out for `session.rs` and the pipeline directory; no clean-LSP claim is made. `cargo clippy -p engine --lib --tests -- -D warnings` is blocked by five pre-existing dead-code diagnostics in `pipeline/envelope.rs` for conservation APIs outside this lane. Focused transition/ledger tests, full engine tests, formatter, preservation verifier, and `cargo check` are recorded in `task-3/pipeline-fee-transition-subtask-c-20260920.md`.

## 2026-09-20 Envelope conservation-row subtask A verification limit

- The focused envelope test command cannot compile after this slice because concurrent out-of-scope `pipeline/mod.rs` re-exports private `transition.rs` types (`BuyFillInput`, `FillTransition`, `SellFillInput`), producing E0365 before tests execute. The slice does not edit the forbidden module or transition lane. Rust LSP diagnostics for both changed files timed out at 30 seconds; rustfmt, diff-check, and ast-grep no-unwrap checks are the available local evidence.
- Superseded for focused tests: the concurrent owner resolved the transition visibility error without this lane editing the forbidden files, and the final envelope suite passes 9/9. LSP timeout remains the only local diagnostics limitation.

## 2026-09-20 Envelope split diagnostics limit

- The size-gate refactor has green focused envelope/receipt-key/transition/ledger and projection suites, but Rust LSP diagnostics timed out after 30 seconds for `conservation.rs`, `envelope.rs`, `mod.rs`, and `envelope_tests.rs`. Cargo compilation and focused behavior evidence remain the available type/behavior proof; Todo 3 remains open.

## 2026-09-20 Task 3 authoritative integration boundary

- The current `P2CandidateBatch` has only canonical `(key, owner, intent)` data. It cannot drive P3 because it lacks P1 allocation context, account-local remaining budgets, deterministic order-ID allocation, envelope drafts, stock shadow operations, and phase-local rejection facts. `plan_tick` still emits P3-P8 compatibility tokens and `commit_tick` still executes the legacy bridge.
- Independent review `ses_f42923a37ffeZ9zgncQ3bpd163` returned BLOCKED: connecting the ledger to that batch now would either reuse the mutating compatibility bridge or reimplement legacy routing, both contrary to ADR-0017. A coherent P2-to-P3 ownership handoff is required before P3-P6 can be integrated.
- Current full-engine quality blockers are unrelated to this component change: `cargo test -p engine` fails `session::plan_chain_candidates::source_tests::source_preserves_multi_continuation_order_and_payload_without_session_mutation` with `InvalidRouteOutcome`; strict clippy fails existing unused fields in `plan_chain_candidates.rs` and `plan_execution/commands.rs`. Rust LSP timed out.

## 2026-09-20 Current production-integration board

- Critical path: the real player path has an independently approved detached P2→P3 seam; L8 is extending the detached flow through P4→P5→P6. No detached seam is wired into `plan_tick`, and the P9 compatibility bridge remains authoritative.
- Accepted main batch: `RetailProjectionSeen` is now owned by `GameSession`, copied/committed with the tick shadow, and included in `business_state_hash`. Independent review and V1 passed; P6 remains production-blocked until SaveSlot v2 persists this cursor.
- Accepted component registration: P7 P4 fact producers passed independent repair review and are registered in shared `pipeline/mod.rs`; V1 passed 4/4 plus engine check.
- Accepted main batch: P5 session transaction advances `EnvelopeLedger::next_receipt_index` and `GameSession::next_receipt_base` atomically, rejects a split starting cursor with an exact typed diagnostic, and rolls back both on P5 failure. Independent repair review and V1 passed 19/19; frozen hashes: implementation `9686ff5a...c8af7`, tests `dcee1102...478f2`.
- Accepted main batch: P6 session transaction consumes the shadow-owned accounts, retail experience, and replay cursor together. Independent repair review APPROVE covers T+1 false/true and nonzero market-minute wiring; after V1 correctly rejected an intermediate formatting snapshot and a disk-quota retry, the final frozen hashes `0e5f817c...d786` / `cb9e58f2...fa762` passed scoped fmt, P6 12/12, and engine check.
- Hard production blockers: SaveSlot v2 must persist ledger/cursor/retail seen; the P5→P6 seam must consume the shadow-owned seen set; successful migrated tick completion must use `rebase_live_for_next_tick`. Current compatibility-only `finish_p0_tick/reset_tick_state` cannot be changed early because the legacy bridge mutates books without emitting the complete new ledger history.
- Active work: L8 owns the isolated P4→P5→P6 seam and is materializing only its two new files for review; an independent reviewer owns the frozen P6 wrapper. Next main action is to review/register the detached seam, then extend it through approved P7 fact collection without touching `plan_tick` prematurely.

### 2026-09-20 continuous P4-P7 integration update

- The detached P4→P5→P6 seam is independently approved and its shared registration V1 gate now passes: frozen hashes `b47c9660...a7be` / `79127e05...c2a`, focused 4/4, scoped edition-2021 fmt, and `cargo check -p engine --lib`. The earlier disk-quota failure is superseded by this successful rerun.
- The continuous P4-fact→P7 collector seam is independently APPROVE and registered. Frozen hashes `5a6f1d54...2407` / `0add363e...2b3`; shared V1 passes 3/3, scoped fmt, and engine check. Its test correctly treats Trade local identity as stock-scoped and does not incorrectly permit account-scoped OrderAccepted identity collisions.
- The next main-owned batch is frozen but not registered: a prospective `GameSession` candidate applies continuous P4-P7 only after receipt-cursor, stock-ownership, and P7 event collection checks succeed. Focused tests cover atomic success, P7 seq overflow rollback, split receipt cursor, and unknown stock (4/4); frozen hashes `ec6fd0d...535c` / `f3b23ee2...4c8d`. Independent review is in flight.
- These batches still do not switch `plan_tick`, remove the legacy bridge, implement P9, or satisfy SaveSlot v2. Todo 3-7 remain open; production activation remains blocked by persistence of ledger/receipt cursor/retail seen and the safe post-migration live-ledger rebase.
- SaveSlot v2 interface audit confirms the persistence format must use a dedicated quiet-point DTO for live envelopes plus cumulative audit, not deserialize the runtime `EnvelopeLedger` with its tick-local terminal/conservation/seen-key state. Required future fields include explicit `schema_version = 2`, healthy-only marker, live envelope audit DTO, receipt cursor, P6 replay identities, and complete `StrategyState`; missing/old/future versions must be explicitly rejected without migration. This remains preparation only: the legacy bridge still resets live audit, so save/restore implementation is not released until the new P4-P6 path is sole production authority and successful tick completion rebases live ledger state.
- The P4→P7 `GameSession` candidate seam is independently APPROVE and shared V1 now passes frozen hashes `ec6fd0d...535c` / `f3b23ee2...4c8d`, focused 4/4, scoped fmt, and engine check. It remains a prospective candidate API and is not a P9 authority commit.
- Next frozen, unregistered main batch: P3 validation output is partitioned by the approved continuous adapter, stock workers run through Rayon, and their owned outputs enter the approved atomic P4→P7 session-candidate seam. Focused success/adapter-rollback tests pass 2/2; frozen hashes `e9ac7757...5ecb` / `92206743...c056`; independent review is in flight.

### 2026-09-20 scheduler-rule adoption checkpoint

- Current plan fingerprint: `e6f82e18b2ab75a13fd31875fcac7847cdcfeaed8adc34aca98a21544b0bb824`. The running P3→P7 reviewer acknowledged the new full-snapshot/config/hash-drift and boundary-scenario rules, retained read-only ownership, and was sent the dependency/config fingerprint set. It is the only active semantic review.
- V1 P4→P7 completed before the rule update and has no live build process. V1 has been reactivated only to acknowledge a fixed reusable cache/config contract; no new validation job is released until that receipt and a complete frozen snapshot agree.
- Real process audit after the updated waiting rule found no live Cargo/rustc/copy job. `/tmp/p7-continuous-target.4RvjCc` is the main-owned compatible 2.0 GiB incremental cache and remains retained; it is not eligible for default end-of-batch deletion.
- Critical ordering is now explicit: finish Task 7 in an isolated unique P0-P9 candidate (including rebase and host/failure gates), then implement Task 8 SaveSlot v2 against that candidate, then consider the user-facing production switch. Persistence DTO/test scenarios may be documented before Task 7, but Task 8 implementation must not start early.

## 2026-09-20 P2 candidate-batch verification limits

- Rust LSP diagnostics timed out for `pipeline/mod.rs`, `p2_candidates.rs`, and
  `p2_candidates_tests.rs`; no LSP-clean claim is made. The focused P2 suite passed 6/6 and
  `cargo check -p engine --lib` exited 0.
- Strict Clippy is currently blocked by pre-existing dead-code errors in the independently edited
  plan-chain candidate seam and command continuation type. This P2 model did not alter those
  files; the blocker is recorded rather than suppressed.

## 2026-09-20 Player candidate source review blocker

- Independent review found the `std::mem::take` player candidate source and its
  four focused FIFO/payload/drain/side-effect tests semantically correct and
  A-share-neutral. It withheld PASS because concurrent plan-chain source/test
  changes leave the current engine test build failing before the scoped module
  executes. Historical three-test capture evidence and the separate pre-blocker
  one-test side-effect pass are not a current four-test green claim. Todo 3
  remains open.

## 2026-09-20 P2-to-P3 source-test blocker

- The requested `source_preserves_multi_continuation_order_and_payload_without_session_mutation` still fails before and after the P2-to-P3 handoff with `InvalidRouteOutcome` at its second synthetic `Canceled(order_ids[1])` resume. The P2/P3 handoff does not call plan-chain routing, continuation resume, or `consume_plan_route_command`; it is therefore not attributable to this new boundary. Its test drives an existing continuation with manually supplied outcomes and needs separate plan-chain continuation ownership review before any protected source/test change.

## 2026-09-20 P2 source-composition verification limit

- `cargo check -p engine --lib` succeeds but reports a new dead-code warning for the pure `compose_p2_candidates` adapter. This is expected until the next sequential ownership step connects real shadow-owned NPC/player/plan-chain capture to `plan_decisions`; invoking it with synthetic empty batches or suppressing the warning would falsely claim source execution. Existing plan-chain candidate fields and command ordinal also retain inherited dead-code warnings. Strict Clippy is therefore not a pass.

### 2026-09-20 P3-to-P7 order-cursor pre-review repair

- The first complete-snapshot review revision was withdrawn before approval when the next P8/P9
  preparation exposed an applicable missing assertion: successful P3 validation allocated order
  IDs, but the P3-to-P7 session candidate did not install `next_order_id_after`. That revision is
  not an accepted review result.
- TDD red evidence used the retained main-owned target
  `/tmp/p7-continuous-target.4RvjCc`: the two-stock success test observed session cursor `1`
  instead of expected `2` after one accepted place.
- The repaired seam now validates that P3 draft IDs are contiguous from the candidate session
  cursor, rejects stale P3 output before workers with the dedicated
  `pipeline::p3_p7_session_transaction` location, and installs the next order cursor only after
  the complete P4-to-P7 transaction succeeds. Adapter and downstream transaction failures retain
  the exact pre-call cursor.
- Isolated focused result: 4/4; scoped edition-2021 format and `cargo check -p engine --lib`
  pass. Current frozen-file candidates are implementation `dad8c7ee...b50` and tests
  `96ca8072...9b8`; a new complete snapshot and independent review are still required before
  shared module registration. Todo 3-7 remain open; P8/P9, rebase, legacy-bridge removal and
  SaveSlot v2 are unchanged.

### 2026-09-20 P3-to-P7 r3 independent findings and r4 repair

- Independent r3 review returned `NEEDS FIX`; r3 is not released. Rayon 1.12 does not promise
  which error is returned when a parallel `Result` collection contains multiple errors, so the
  former direct parallel `collect::<Result<Vec<_>, _>>()` could expose a schedule-dependent
  `Worker { code, source }` identity. The two-stock test also failed to snapshot the untouched
  stock, so a replacement bug could preserve map length and escape the assertion.
- The repair keeps stock work parallel but collects the indexed input results into an ordered
  `Vec<(StockCode, Result<...>)>`, then serially selects the first error in the adapter's canonical
  BTreeMap stock order. A focused two-error test pins that identity. The success scenario now
  serializes the untouched market hash projection before and after the transaction.
- Main isolated result is 5/5 after explicitly invalidating a stale Cargo fingerprint caused by
  an accidental sequential reuse of the same target from a different source root. The retained
  target remains `/tmp/p7-continuous-target.4RvjCc`; future runs must bind it to one source copy at
  a time and force correct source invalidation when changing roots.
- Disk quota recovery moved three obsolete main-owned copied `.review-targets` directories
  (6.8 GiB total) to the recoverable workspace path
  `.omo/trash/main-isolation-review-targets-20260920`; no frozen snapshot, active cache, reviewer
  cache, or other agent artifact was removed.
- r4 independent review is `APPROVE`: exact 5/5, scoped fmt, engine lib check, full frozen-list
  first/last verification, and configuration match. Main has now registered only the implementation
  and test modules in shared `pipeline/mod.rs`; V1 on a new complete registered snapshot remains
  required before this batch becomes the accepted shared integration baseline.

### 2026-09-20 P3-to-P7 registered-r5 acceptance

- V1 job `V1-P3P7-R5-001` verified the complete registered-r5 frozen snapshot before and after
  execution: 2025/2025 files and manifest digest
  `4a0ec09f356066ecb71ebe8308aa8c1e8740c474b487f7a1c140048e21c247d4` remained unchanged.
- Under configuration fingerprint
  `8d4a0e4007d290f8175f8f78e576a80bb7d61155ec9e187df66fee13f2148744`, scoped edition-2021
  format, the exact five focused tests, and `cargo check -p engine --lib` all exited 0. The reusable
  V1 cache at
  `/data1/baiyifan/workplace/.v1-cargo-targets/stock-market-game-engine-native-default-rustc-1.96.1`
  remains retained.
- This establishes the shared detached P3-to-P7 integration baseline only. It does not release
  P2, P8, P9, `plan_tick`, the legacy-bridge removal, live-ledger rebase, or SaveSlot v2; Todo 3-7
  remain open.
