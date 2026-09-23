# Problems — escrow-parallel-engine

Unresolved blockers and technical debt discovered during work on this plan.

_Auto-scaffolded by /start-work. Append new entries below - never overwrite._

## 2026-09-20 Production phase wiring remains incomplete

- P0/P1 perform real shadow work, but P2-P8 in `pipeline::plan_tick` remain compatibility tokens and P9 still invokes the full legacy bridge. Component tests and adapters are not production-path acceptance.
- The legacy bridge and the P0 diagnostic splice must be removed only when the complete P2-P8 path is ready; earlier removal loses behavior, while retaining it after real phase wiring would duplicate routing, matching and settlement.
- `step_frame` still attaches facts after commit through a fallible path. SaveSlot v2 still lacks the complete envelope ledger audit, global receipt cursor and all required authoritative strategy state. Todos 3-7 therefore remain open.

## 2026-09-20 B1 adaptive plan-chain phase-contract conflict

- Independent Astra read-only review confirmed that one production plan-chain can yield multiple
  dependent commands in the same tick: Replace is cancel-then-submit, and conflict cleanup can be
  cancel N times then submit. Existing tests pin generation `0/1/2`; each successor is selected
  only after the previous real P4 outcome.
- A flat `P2CandidateBatch` cannot enumerate those successors before P4 without predicting cancel
  success or activating commands that should not exist. Running another P3-P7 batch after the
  outcome would violate the current “one P3 validation/P3-P7” B1 wording and can incorrectly reuse
  same-batch released resources. Moving the successor to another tick is an unapproved behavior
  change.
- The current isolated B1 one-yield adapter therefore fails atomically if resume exposes another
  ready route. This prevents partial-chain commit but does not satisfy full B1 plan-chain support.
- The smallest implementable design keeps one P1 snapshot and one persistent P3 budget/identity
  driver, interleaves only the dependent plan-chain P3/P4 operations, then performs P5-P9 once.
  That conflicts with the strict existing statement that complete two-pass P3 and ID-overflow
  detection finish before any P4. The alternative is a new conditional execution-program
  contract with explicit inactive-branch budget/ID semantics. Either requires a reviewed Plan/ADR
  clarification; main must not silently choose one.
- This blocker does not change A-share T+1, lot size, price cage or auction rules. It concerns
  execution ordering and atomicity. Player/NPC continuous paths and unrelated B1 tests can proceed,
  but full B1 freeze/APPROVE/commit cannot be claimed until the contract is resolved.

### 2026-09-20 新版 Plan 已解除该契约冲突

- Plan `7ef2ec23...` 与 ADR-0017 `bc7e7f47...` 已明确允许同一 P1 截点下增量交错 P3/P4，并要求操作流排空后 P5-P9 各一次；上文“需用户澄清”的阻塞结论已失效，仅保留为历史记录。
- 当前真实缺口转为实现缺口：候选仍是单个外部 `PlanChainYieldDriver`，仍含 `AdaptivePlanChainRequiresAnotherRoute`，且 outcome 仍从展示事件反查。L2/L3/L4 已分别负责完整根计划 coordinator、持续 P3 和增量股票 shadow/typed facts；main 负责删除旧接线并组装。

## 2026-09-20 Production cutover critical path and first integration gates

The integration owner must advance one isolated `GameSession` shadow path in this order; component approval alone does not move the production entry point:

1. P2 coordinator: capture immutable decision input, run/project NPC state, drain player input, and yield/resume plan-chain commands without routing.
2. Stateful P3: consume the canonical `npc → player → plan_chain` stream against one P1 snapshot, then normalize unknown-stock cancels into ordinary rejection facts.
3. P4: partition normalized operations, run the phase-appropriate stock worker, and merge explicit stock-local facts without using worker completion order.
4. P5/P6: atomically insert created envelopes, validate receipts/terminals, settle accounts, and project retail experience exactly once.
5. P7/P8: construct all explicit producer facts, canonicalize external events, and validate both hashes before any authority write.
6. P9: replace the compatibility bridge with commit-only behavior and remove the P0 diagnostic splice only when the complete path above passes its failure gates.

First isolated integration scenarios, defined before production cutover:

- A real player non-crossing limit buy creates exactly one P3 envelope, one live order and one `OrderAccepted`; cash is escrowed once, no settlement occurs, and authority remains unchanged until P9.
- A real crossing buy consumes a resting sell through P4, P5 produces a contiguous two-envelope receipt chain, P6 applies Buy-before-Sell settlement once, and P9 exposes the resulting balances/positions/events once (no legacy duplicate).
- An unknown-stock cancel produces one ordinary `IntentRejected` with its sealed identity, allocates no order ID/envelope, does not enter a stock adapter, and does not poison the tick.
- A typed failure injected after P4, after P6, and immediately before commit leaves the pre-tick business hash unchanged, emits no partial event, persists poison, and makes save/next-step fail explicitly.

These tests are prepared and run in a content-addressed isolated integration snapshot. They do not enable an incomplete path in the shared production entry point.

## 2026-09-19 TickShadow / P9 后续范围

- 本里程碑没有迁移 P0 过期、P1 截点、P2 决策、P3 验证/ID、P4 撮合、P5 收据或 P6 结算业务本体；它们仍在 shadow 兼容桥内由旧 tick 执行，后续迁移必须逐阶段替换该桥，不能把当前空 envelope ledger 误报为完整 Todo 3 接受。

## 2026-09-17 Todo 2 authoritative-baseline blocker

- `HEAD` `7041d35dc362ca74f4f3313e6804db9499f0679a` is clean and reproducible only in a detached worktree.
- The live checkout already contained behavior changes before Todo 2, including `packages/engine/src/session/decision_chain.rs` and the simulation runner. Using HEAD omits those behaviors; using the live tree cannot honestly claim a single committed `preserved_test_baseline_sha`.
- Todo 2 therefore stopped before engine/test edits. Resolution requires choosing committed HEAD, first integrating the pre-existing work, or changing the plan to bind a composite HEAD+patch identity.

## 2026-09-17 Todo 2 canonical event-order owner decision

- A detached composite-baseline probe produced `OrderAccepted → AuctionTick` for a valid buy.
- The planned key orders Stock before Account in the same phase, producing `AuctionTick → OrderAccepted` after canonical merge.
- This changes non-`seq` event order. Plan divergence #6 forbids silently mapping it, so Todo 2 is blocked until the user chooses baseline-preserving merge order or authorizes a new display-order divergence.
- Superseded final resolution: each tick remains a complete committed frame and acceleration batches frames only. Frame event order has no business meaning; explicit time-series payload drives the frontend, and comparison is tick-scoped event identity/payload multiset comparison. The recorded order inversion is not a blocker or a new business divergence.

---

## Todo 1 limitations

- Markdown diagnostics could not run because no `.md` LSP server is configured. The exact `lsp_diagnostics` result was “No LSP server configured for extension: .md”.
- The worktree was dirty before this task. Existing unrelated changes include many engine, host, script, evidence, README, and documentation changes. The pre-existing `docs/trading-rules.md` C06 section was not reverted.

## Repair entry: 2026-09-17

- Resolved both AdversarialVerify findings. No unresolved repair blocker remains. Markdown LSP remains unavailable because no `.md` server is configured.

## 2026-09-19 Todo 3 保留测试门禁阻断

- 已完整读取 sealed attempt-12 manifest 与 `b-test-inventory.json`。冻结 B 集合精确为 4 个符号；`constraints.rs:90-94` 是计划软预算排除项，直接卖方 cash assertion anchor 为空。
- live worktree 相对 `7041d35dc362ca74f4f3313e6804db9499f0679a` 已修改 32 个既有 `packages/engine/tests` 文件（716 新增 / 447 删除），另有 12 个未跟踪测试文件。raw SHA 比较不能把这些变更诚实收敛为冻结的 4 项 B 集合。
- 独立复核确认 sealed manifest 的基准策略是 `HEAD + applicable pre-existing closure overlay`，故 raw SHA 差异本身不足以断言所有这些文件都不允许。Todo 3 仍须在首门禁 fail-closed：先以 sealed closure/overlay 摘要重建 composite baseline，再审计残留 hunk；未完成前不编辑 engine 代码、既有断言或 sealed artifacts。详见 `.omo/evidence/escrow-parallel-engine/task-3/preservation-gate-needs-fix.md`。

## 2026-09-19 Todo 3 修正 composite baseline 后的残余门禁

- 已直接读取 sealed `attempt-12/overlay.json`：仅 desktop/server Cargo.toml 和 `packages/engine/src/session/decision_chain.rs` 三项，完全没有 `packages/engine/tests/**`。故测试 baseline bytes 确实等于 preserved commit `7041d35dc362ca74f4f3313e6804db9499f0679a`，不再声称“缺少测试 overlay 对账”。
- 独立逐 hunk 审查将 32 个 tracked 测试文件中绝大多数 `step/save -> Result expect` 改写归 `(c)`，12 个 untracked test 路径为 additive Todo 2 protocol/key/TickFrame/Civil/StrategyState 或 characterization 文件，均无 path collision 或 #9 seller 语义。
- 但发现 3 个不属于 `(b)` 或现有 `(c)` 的语义残余：`account.rs::reexport_from_crate_root` 的 `production_state/NonAuthoritative` 断言；`company_event_contract.rs::announcement_event_follows_successful_immutable_library_insertion` 的 immutable publication membership 断言；`company_event_contract.rs::civil_report_refresh_validates_and_reconnect_resolves_publication` 整个新 refresh/reconnect 测试。按任务规则 fail-closed，未创建 verifier 或 ledger 实现。详见 `.omo/evidence/escrow-parallel-engine/task-3/preservation-gate-corrected-needs-fix.md`。

## 2026-09-19 Todo 3 追加 C 决定后的新 residual

- 三个授权 `(c)` anchors 已不再阻断；在 hunk verifier 的真实 diff 解析中发现新且未授权 hunk：`packages/engine/tests/company_scenarios/restore.rs::live_partial_fill_restores_and_continues_identically`，`@@ -131 +150,0 @@`，hash `dfd70220f09e77227a08fc98e4eb57c843db7b7752d3cb649b6509e03553c10d`。
- 该 hunk 删除一个 `restored.save().plans...filled_qty == 200` 断言；虽有相邻 Result unwrap 展开，删除本身不能无精确 policy 自动视为 mechanical `(c)`，也不能扩为 #9 `(b)`。fail-closed；已删除未完成 verifier/inventory 草稿，未编辑 engine ledger 行为。

## 2026-09-20 Todo 3 acceptance remains open

- The preservation gate and a bounded per-envelope/aggregate conservation increment are green, but Todo 3's required P4/P5/P6 receipt, seller-cap, cross-tick fill, auction rollover, and complete negative-path matrix are not implemented by this increment. The Todo 3 checkbox remains open and no completion claim is authorized.

## 2026-09-20 Todo 3 preservation-gate repair boundary

- The exact restore assertion deletion is repaired and now independently guarded by sealed baseline assertion identity/anchor plus current normalized hash. This resolves only the blocking preservation-gate defect; Todo 3 envelope ledger, conservation implementation, and top-level checkbox remain open.

## 2026-09-20 Todo 3 lane D handoff

- Subtask C provides a pure `pipeline::transition` result with buyer resource delta, seller capped charged fee components, cumulative audit after-state, and delivery cash. It does not build `EnvelopeReceipt`, update `EnvelopeAudit`, persist terminal charged history, or call settlement. Lane D must consume this API when extending ledger receipt-chain validation and must retain seller cash envelope/spent/released cash at zero.
- Independent review confirms the missing authoritative receipt/settlement integration and token-only P5/P7/P8 phases. These are valid Todo 3 integration concerns but are explicitly prohibited in atomic subtask C. The review's zero-test observation is inapplicable: `cargo test -p engine session::pipeline::transition_tests --lib -- --nocapture` ran and passed 4 tests.

## 2026-09-20 Todo 3 ledger subtask D boundary

- The typed ledger validates receipt facts and conservation rows but does not create P3 envelopes from candidates, produce P4 receipts, aggregate P5 outboxes, call P6 settlement, or replace compatibility tokens. Todo 3 remains open.

## 2026-09-20 Todo 3 ledger size refactor boundary

- This repair only splits the already-validated ledger implementation and tests below the project
  size ceiling. P3/P4/P5/P6 authoritative receipt construction, aggregation, and settlement are
  still pending, so neither this refactor nor Subtask D completes Todo 3.

## 2026-09-20 P3-P6 authoritative integration blocker

- A temporary real-GameSession red test queued a player buy, ran `plan_tick`, and required a
  matching `P3Created` ledger envelope. It failed as expected because P3 remains a token. The
  test was removed after recording the result; the restored pipeline test module passes 6/6.
- Implementing only a P3 ledger insertion would duplicate legacy routing in P9 and then lose the
  row to `finish_p0_tick`; it cannot satisfy receipt-driven settlement. The prerequisite is P2
  composition that yields ordered non-mutating NPC/player/resumable-plan candidates before P3.
  Evidence: `task-3/p3-p6-integration-blocker-20260920.md`.
