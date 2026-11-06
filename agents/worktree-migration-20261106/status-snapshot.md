# Worktree 状态起始快照（2026-11-06）

本清单依据 `/tmp/stock-worktree-status-20261106.txt` 与 `/tmp/stock-dirty-group-0.txt` 至 `-5.txt` 整理。两组输入均有 186 个 worktree，逐 worktree 状态记录一致。扫描使用 `git status --porcelain=v1 --untracked-files=normal`；未跟踪目录可能被折叠，清单不表示目录中的完整文件列表。

这是起始状态快照，不是当前状态，也不是逐文件内容或语义审查；不得据此判断改动可迁移、可提交或符合大 A 语义。迁移前应重新扫描，并独立检查内容、归属、依赖及交易语义。本清单不含未跟踪文件内容或日志内容；疑似日志、性能采样及敏感路径以占位符遮蔽，不能据此推断其内容。

标题中的 `.` 表示仓库根目录，`./` 后为相对于该根目录的路径；外部 worktree 使用可区分别名，并保留相对于仓库根目录的路径，以便在本机定位。标题不记录本机绝对路径或用户名。

状态码保留 Git porcelain 的两列：第一列为暂存区，第二列为工作区；`??` 表示未跟踪，`UU` 表示未合并。每条记录使用代码块保留空格。超时是扫描错误，不代表干净。

## `.`

```text
 M ROLE.md
 M agents/coordination/current-work.md
 M agents/coordinator-status-2026-10-22.md
 M agents/engine-max-performance/string-free-inventory.md
 M memory/MEMORY.md
 M packages/engine/src/session/hash.rs
?? [疑似日志、采样或敏感路径已遮蔽]
?? agents/api-contract-company-id/
?? agents/approved-integration/
?? agents/asm-comparator/
?? agents/branch-cleanup/
?? agents/branch-completion/
?? agents/branch-convergence-2026-11-02/
?? agents/branch-convergence/
?? agents/calendar-mirror-origin-oracle-2026-10-30/
?? agents/calendar-new-due-horizon-2026-10-30/
?? agents/company-branch-coverage-audit-2026-11-01/
?? agents/control-worker-observation/
?? agents/coordination/branch-convergence-2026-10-30/
?? agents/coordination/delivery-current-2026-10-30.md
?? agents/coordination/delivery-history-before-e1af-2026-10-30.md
?? agents/coordination/handoff-2026-10-25.md
?? agents/coordination/model-policy-2026-10-25.diff
?? agents/coordination/resume-2026-10-25.md
?? agents/coordination/resume-2026-10-29.md
?? agents/coordination/tree-review-2026-10-30/
?? agents/day-end-public-views-sync/
?? agents/deadline-hardgate/
?? agents/desktop-ipc-contract/
?? agents/desktop-matrix-fixture/
?? agents/development-main/
?? agents/escrow-harness-contract/
?? agents/final-perf-20pct-20261103/
?? agents/fix-projection-regression/
?? agents/full-regression-hash-parallel/
?? agents/interrupted-branch-audit-20261103/
?? agents/multi-group-roster-offset/consumer-evidence-2026-10-25.md
?? agents/multi-group-roster-offset/luna-merge-gate-r2-2026-10-22.md
?? agents/multi-group-roster-offset/sol-merge-gate-r2-2026-10-22.md
?? agents/native-store-deadline/
?? agents/nested-deadline/
?? agents/orderid-fixture-shrink/
?? agents/perf-intraday-independent/contract-revision-2026-10-25.md
?? agents/perf-intraday-independent/convergence-contract.md
?? agents/perf-intraday-independent/e1-dispatch-2026-10-26.md
?? agents/perf-intraday-independent/e2-handoff-preflight-2026-10-26.md
?? agents/perf-intraday-independent/e2-info-final-binding-2026-10-27.md
?? agents/perf-intraday-independent/implementation-dispatch-2026-10-26.md
?? agents/perf-intraday-independent/implementation-dispatch-review-2026-10-26.md
?? agents/perf-intraday-independent/implementation-e1-review-2026-10-26.md
?? agents/perf-intraday-independent/interface-freeze-dp1-dp4-2026-10-26.md
?? agents/perf-intraday-independent/interface-freeze-dp1-dp4-before-review-2026-10-26.md
?? agents/perf-intraday-independent/interface-freeze-review-2026-10-26.md
?? agents/perf-intraday-independent/luna-contract-verdict-v2.md
?? agents/perf-intraday-independent/luna-contract-verdict.md
?? agents/perf-intraday-independent/plan-id-ownership-design-2026-10-25.md
?? agents/perf-intraday-independent/resume-2026-10-29/
?? agents/perf-intraday-independent/review-e1/
?? agents/perf-intraday-independent/sol-contract-verdict-v2.md
?? agents/perf-intraday-independent/sol-contract-verdict.md
?? agents/perf-intraday-independent/sol-gate-verdict-2026-10-22.md
?? agents/perf-intraday-independent/sol61-contract-final-2026-10-25.md
?? agents/performance-delivery/
?? agents/price-cage-floor/
?? agents/price-improvement-refund/
?? agents/proto-compile-gate/
?? agents/public-library-restore-perf/
?? agents/reintegration-b4d/design.md
?? agents/remaining-branches-review-20261103/
?? agents/scripts-regression-inventory/
?? agents/space-cleanup-2026-10-26/
?? agents/supervisor-suite-split/
?? agents/treasury-autonomy/
?? agents/validation-archive/
?? agents/verify-metrics-regression/
?? agents/wasm-deps-deadline/
?? agents/wasm-restore-target/
?? agents/web-regression-environment/
?? agents/web-worker-error-fidelity/
?? agents/web-worker-ownership/
?? agents/worktree-reinspection-20261104/
?? agents/ws-fixture-contract/
?? agents/year-close-authority/
?? memory/feedback_leader_never_writes_code.md
?? memory/feedback_only_opus47.md
```

## `外部 worktree 01（../stock-market-game-target-dir-28）`

```text
?? agents/full-regression-target-dir/
```

## `./.claude/worktrees/agent-a5582fa4bfe4fa409`

```text
 M scripts/verify/asm-equiv-cmp-regression.py
 M scripts/verify/asm-equiv-cmp.py
```

## `./.claude/worktrees/agent-abb865e48ac6093cb`

```text
?? .tmp-review/
```

## `./.claude/worktrees/agent-ae63cc05fdf6097ee`

```text
 M packages/engine/src/runtime/group.rs
?? packages/engine/src/runtime/pending_cash_tests.rs
```

## `./.worktree/api-contract-company-id`

```text
 M apps/server/tests/api_contract.rs
```

## `./.worktree/app-hook-deps-2026-10-30`

```text
 M apps/web/src/App.tsx
```

## `./.worktree/attention-civil-fixture-2026-10-30`

```text
 M packages/engine/tests/attention_discovery/failures/discovery.rs
?? agents/branch-completion/attention-civil-fixture-2026-10-30/
```

## `./.worktree/auction-price-history-2026-10-30`

```text
?? agents/branch-completion/auction-price-history-2026-10-30/
```

## `./.worktree/branch-convergence-final-20261102`

```text
?? agents/branch-convergence-final-validation/
```

## `./.worktree/build-targets-args-2026-10-30`

```text
?? agents/branch-completion/build-targets-args-2026-10-30/
```

## `./.worktree/calendar-final-fixes-2026-10-29`

```text
 M packages/engine/src/runtime/calendar_chain_caller_tests.rs
 M packages/engine/src/runtime/calendar_group_day_end_tests.rs
 M packages/engine/src/runtime/coordinator.rs
 M packages/engine/src/runtime/desk.rs
 M packages/engine/src/runtime/drive.rs
 M packages/engine/src/runtime/drive/convergence.rs
 M packages/engine/src/runtime/drive_tests.rs
 M packages/engine/src/runtime/group.rs
 M packages/engine/src/runtime/protocol.rs
 M packages/engine/src/runtime/seal_tests.rs
 M packages/engine/src/runtime/world.rs
 M packages/engine/src/runtime/world/admin_transaction.rs
 M packages/engine/src/runtime/world/prepared_stop.rs
 M packages/engine/src/session.rs
 M packages/engine/src/session/administrative_mirror.rs
 M packages/engine/src/session/civil_information.rs
 M packages/engine/src/session/civil_information_runtime_tests.rs
 M packages/engine/src/session/failure.rs
 M packages/engine/src/session/hash.rs
 M packages/engine/src/session/notices.rs
 M packages/engine/src/session/protocol/civil/session.rs
 M packages/engine/src/session/runtime_diagnostics_tests.rs
 M packages/engine/src/session/runtime_mirror.rs
 M packages/engine/src/session/shared_ingress.rs
?? agents/branch-completion/mixed-calendar-c2c/final-gates-fixes-source-35/
?? agents/branch-completion/mixed-calendar-c2c/final-gates-fixes-source-36/
?? agents/branch-completion/mixed-calendar-c2c/g8-semantic-source-38/
?? agents/branch-completion/mixed-calendar-c2c/hash-diagnostic-source-37/
?? agents/branch-completion/mixed-calendar-c2c/pending-fatal-source-39/
?? agents/branch-completion/mixed-calendar-c2c/pending-green-source-39/
?? agents/branch-completion/mixed-calendar-c2c/pending-start-cleanup-37/
?? agents/branch-completion/mixed-calendar-c2c/pending-start-red-source-38/
?? agents/branch-completion/mixed-calendar-c2c/semantic-pending-source-38/
?? packages/engine/src/runtime/world/admin_transaction/calendar_information_intersection_tests.rs
?? packages/engine/src/session/calendar_entry_cache_tests.rs
?? packages/engine/src/session/calendar_final_boundary_tests.rs
?? packages/engine/src/session/calendar_nonzero_plan_parity_tests.rs
?? packages/engine/src/session/civil_driver_ownership_tests.rs
?? packages/engine/src/session/civil_information_acquisition_tests.rs
```

## `./.worktree/calendar-final-gates-2026-10-29`

```text
 M packages/engine/src/runtime/calendar_chain_caller_tests.rs
 M packages/engine/src/runtime/calendar_group_day_end_tests.rs
 M packages/engine/src/runtime/coordinator.rs
 M packages/engine/src/runtime/drive.rs
 M packages/engine/src/runtime/drive/convergence.rs
 M packages/engine/src/runtime/drive_tests.rs
 M packages/engine/src/runtime/group.rs
 M packages/engine/src/runtime/protocol.rs
 M packages/engine/src/runtime/seal_tests.rs
 M packages/engine/src/runtime/world.rs
 M packages/engine/src/runtime/world/admin_transaction.rs
 M packages/engine/src/session.rs
 M packages/engine/src/session/administrative_mirror.rs
 M packages/engine/src/session/civil_information_runtime_tests.rs
 M packages/engine/src/session/failure.rs
 M packages/engine/src/session/hash.rs
 M packages/engine/src/session/notices.rs
 M packages/engine/src/session/runtime_diagnostics_tests.rs
 M packages/engine/src/session/shared_ingress.rs
?? agents/branch-completion/mixed-calendar-c2c/final-gates-e2-source-34/
?? agents/branch-completion/mixed-calendar-c2c/final-gates-source-32/
?? packages/engine/src/runtime/world/admin_transaction/calendar_information_intersection_tests.rs
?? packages/engine/src/session/calendar_entry_cache_tests.rs
?? packages/engine/src/session/calendar_final_boundary_tests.rs
?? packages/engine/src/session/calendar_nonzero_plan_parity_tests.rs
?? packages/engine/src/session/civil_driver_ownership_tests.rs
?? packages/engine/src/session/civil_information_acquisition_tests.rs
```

## `./.worktree/calendar-mirror-origin-oracle-2026-10-30`

```text
 M packages/engine/src/runtime/calendar_public_restore_tests.rs
 M packages/engine/src/session/calendar_nonzero_plan_parity_tests.rs
?? agents/calendar-mirror-origin-oracle-2026-10-30/
```

## `./.worktree/calendar-new-due-horizon-2026-10-30`

```text
 M packages/engine/src/session.rs
 M packages/engine/src/session/civil_information.rs
?? agents/calendar-new-due-horizon-2026-10-30/
?? packages/engine/src/session/civil_due_horizon_runtime_tests.rs
```

## `./.worktree/calendar-runtime-ceiling-2026-10-30`

```text
 M packages/engine/src/session.rs
 M packages/engine/src/session/civil_clock.rs
?? agents/calendar-runtime-ceiling-2026-10-30/
?? packages/engine/src/session/civil_runtime_ceiling_tests.rs
```

## `./.worktree/calendar-v11-business-2026-10-29`

```text
 M packages/engine/Cargo.toml
?? agents/branch-completion/mixed-calendar-c2c/v11-business-source-33/
```

## `./.worktree/call-layer-red`

```text
 M packages/engine/src/runtime/group.rs
 M packages/engine/src/runtime/group_roster_offset_tests.rs
 M packages/engine/src/runtime/mod.rs
 M packages/engine/src/runtime/world.rs
```

## `./.worktree/civil-external-authority-2026-10-28`

```text
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/actual-review.md
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-001.stderr
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-001.stdout
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-002.stderr
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-002.stdout
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-003.stderr
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-003.stdout
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-004.stderr
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-004.stdout
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-005.stderr
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-005.stdout
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-006.stderr
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-006.stdout
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-007.stderr
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-007.stdout
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-008.stderr
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-008.stdout
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-009.stderr
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-009.stdout
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-010.stderr
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-010.stdout
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-011.stderr
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-011.stdout
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-012.stderr
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-012.stdout
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-013.stderr
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-013.stdout
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-014.stderr
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-014.stdout
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-015.stderr
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-015.stdout
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-016.stderr
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-016.stdout
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-017.stderr
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-017.stdout
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-018.stderr
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-018.stdout
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-019.stderr
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-019.stdout
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-020.stderr
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-020.stdout
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-021.stderr
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-021.stdout
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-022.stderr
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-022.stdout
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-023.stderr
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/case-023.stdout
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/feature-build.stderr
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/feature-build.stdout
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/feature-guard-resources.jsonl
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/feature-guard.stderr
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/feature-guard.stdout
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/registered-exacts.stderr
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/registered-exacts.stdout
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/seal-feature.stderr
?? agents/perf-intraday-independent/implementation/e2/civil-external-authority-01/native-green-01/seal-feature.stdout
```

## `./.worktree/clippy-capital-replay-fix-2026-10-30`

```text
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-cfg-2026-10-30`

```text
 M packages/engine/src/company/bank/income_tax.rs
 M packages/engine/src/company/cash_dividend.rs
 M packages/engine/src/company/cash_dividend_tax.rs
 M packages/engine/src/company/correction.rs
 M packages/engine/src/company/disclosure_correction_batch_proof_tests.rs
 M packages/engine/src/company/disclosure_correction_batch_tests.rs
 M packages/engine/src/company/disclosure_correction_owner_tests.rs
 M packages/engine/src/company/disclosure_correction_proof.rs
 M packages/engine/src/company/disclosure_correction_proof_tests.rs
 M packages/engine/src/company/disclosure_correction_test_fixture.rs
 M packages/engine/src/company/industrial/chart.rs
 M packages/engine/src/company/industrial/income_tax.rs
 M packages/engine/src/company/industrial/inventory_sources.rs
 M packages/engine/src/company/insurance/income_tax.rs
 M packages/engine/src/company/mod.rs
 M packages/engine/src/company/operations.rs
 M packages/engine/src/company/operations/config.rs
 M packages/engine/src/company/operations/core.rs
 M packages/engine/src/company/operations/disclosure_correction_legacy_tests.rs
 M packages/engine/src/company/real_estate/income_tax.rs
 M packages/engine/src/company/real_estate/mod.rs
 M packages/engine/src/company/simple/preferences.rs
 M packages/engine/src/company/system.rs
 M packages/engine/src/runtime/admin_tax_projection.rs
 M packages/engine/src/runtime/calendar_public_restore_tests.rs
 M packages/engine/src/runtime/coordinator.rs
 M packages/engine/src/runtime/desk.rs
 M packages/engine/src/runtime/drive.rs
 M packages/engine/src/runtime/group.rs
 M packages/engine/src/runtime/group/admin_transaction.rs
 M packages/engine/src/runtime/group/admin_transaction/information.rs
 M packages/engine/src/runtime/group/admin_transaction/information_tests.rs
 M packages/engine/src/runtime/group/admin_transaction/repurchase_approval_tests.rs
 M packages/engine/src/runtime/group/admin_transaction/repurchase_funds_tests.rs
 M packages/engine/src/runtime/group/admin_transaction/repurchase_source_tests.rs
 M packages/engine/src/runtime/group/origin_consumer_tests.rs
 M packages/engine/src/runtime/group/repurchase_funds_candidate.rs
 M packages/engine/src/runtime/information_clock_context.rs
 M packages/engine/src/runtime/information_clock_context/tests.rs
 M packages/engine/src/runtime/mod.rs
 M packages/engine/src/runtime/protocol.rs
 M packages/engine/src/runtime/world.rs
 M packages/engine/src/runtime/world/admin_transaction.rs
 M packages/engine/src/runtime/world/admin_transaction/calendar_information_intersection_tests.rs
 M packages/engine/src/runtime/world/admin_transaction/funds_cost_tests.rs
 M packages/engine/src/runtime/world/admin_transaction/information_tests.rs
 M packages/engine/src/runtime/world/admin_transaction/repurchase_approval_tests.rs
 M packages/engine/src/runtime/world/admin_transaction/repurchase_funds_tests.rs
 M packages/engine/src/runtime/world/admin_transaction/stage_information_tests.rs
 M packages/engine/src/runtime/world/information_handover.rs
 M packages/engine/src/session.rs
 M packages/engine/src/session/account_book.rs
 M packages/engine/src/session/attention.rs
 M packages/engine/src/session/calendar_entry_cache_tests.rs
 M packages/engine/src/session/calendar_final_boundary_tests.rs
 M packages/engine/src/session/calendar_nonzero_plan_parity_tests.rs
 M packages/engine/src/session/candles.rs
 M packages/engine/src/session/day_boundary_kline_tests.rs
 M packages/engine/src/session/decision_chain.rs
 M packages/engine/src/session/decision_chain/company_source_tests.rs
 M packages/engine/src/session/decision_chain/lifecycle.rs
 M packages/engine/src/session/decision_chain/retail_analysis.rs
 M packages/engine/src/session/decision_chain/roots.rs
 M packages/engine/src/session/disclosure_batch_candidate_tests.rs
 M packages/engine/src/session/disclosure_candidate_tests.rs
 M packages/engine/src/session/exchange_calendar_model.rs
 M packages/engine/src/session/failure.rs
 M packages/engine/src/session/information_anchor/export_tests.rs
 M packages/engine/src/session/notices.rs
 M packages/engine/src/session/pipeline/account_settlement.rs
 M packages/engine/src/session/pipeline/adaptive_plan_chain.rs
 M packages/engine/src/session/pipeline/continuous_tick_finalizer.rs
 M packages/engine/src/session/pipeline/market_tick_transaction.rs
 M packages/engine/src/session/pipeline/quote_expiry.rs
 M packages/engine/src/session/pipeline/read_only_start.rs
 M packages/engine/src/session/pipeline/retail_projection.rs
 M packages/engine/src/session/pipeline/session_execution_transaction.rs
 M packages/engine/src/session/pipeline/stock_execution_transaction.rs
 M packages/engine/src/session/pipeline/stock_stream.rs
 M packages/engine/src/session/player_company_command_candidates.rs
 M packages/engine/src/session/protocol/civil/session.rs
?? agents/branch-completion/clippy-ci-2026-10-30/
?? packages/engine/src/runtime/worker_context.rs
```

## `./.worktree/clippy-cfg-args-leaf-2026-10-30`

```text
 M packages/engine/src/runtime/group.rs
 M packages/engine/src/runtime/group/admin_transaction/information.rs
 M packages/engine/src/session.rs
 M packages/engine/src/session/decision_chain.rs
 M packages/engine/src/session/decision_chain/company_source_tests.rs
 M packages/engine/src/session/decision_chain/lifecycle.rs
 M packages/engine/src/session/decision_chain/retail_analysis.rs
 M packages/engine/src/session/decision_chain/roots.rs
 M packages/engine/src/session/information_anchor/export_tests.rs
 M packages/engine/src/session/notices.rs
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-cfg-args-settlement-2026-10-30`

```text
 M packages/engine/src/session/pipeline/account_settlement.rs
 M packages/engine/src/session/pipeline/continuous_tick_finalizer.rs
 M packages/engine/src/session/pipeline/session_execution_transaction.rs
 M packages/engine/src/session/pipeline/stock_execution_transaction.rs
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-cfg-args-stream-2026-10-30`

```text
 M packages/engine/src/session/pipeline/market_tick_transaction.rs
 M packages/engine/src/session/pipeline/stock_stream.rs
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-cfg-args-tests-2026-10-30`

```text
 M packages/engine/src/runtime/group.rs
 M packages/engine/src/runtime/group/admin_transaction.rs
 M packages/engine/src/runtime/group/admin_transaction/information_tests.rs
 M packages/engine/src/runtime/group/admin_transaction/repurchase_approval_tests.rs
 M packages/engine/src/runtime/group/admin_transaction/repurchase_funds_tests.rs
 M packages/engine/src/runtime/group/admin_transaction/repurchase_source_tests.rs
 M packages/engine/src/runtime/group/origin_consumer_tests.rs
 M packages/engine/src/session/day_boundary_kline_tests.rs
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-cfg-callsite-fix-2026-10-30`

```text
 M packages/engine/src/company/operations/config.rs
 M packages/engine/src/company/simple/preferences.rs
 M packages/engine/src/facade_slot/arena.rs
 M packages/engine/src/facade_slot/roles.rs
 M packages/engine/src/facade_slot/test_supervisor.rs
 M packages/engine/src/facade_slot/tests/exhaustion_tests.rs
 M packages/engine/src/facade_slot/tests/interleave_tests.rs
 M packages/engine/src/facade_slot/tests/lifecycle_tests.rs
 M packages/engine/src/market.rs
 M packages/engine/src/orderbook.rs
 M packages/engine/src/plans/mod.rs
 M packages/engine/src/runtime/desk.rs
 M packages/engine/src/runtime/drive.rs
 M packages/engine/src/runtime/drive/convergence.rs
 M packages/engine/src/runtime/group.rs
 M packages/engine/src/runtime/information_clock_context/tests.rs
 M packages/engine/src/runtime/mod.rs
 M packages/engine/src/session/administrative_mirror.rs
 M packages/engine/src/session/attention.rs
 M packages/engine/src/session/civil_information.rs
 M packages/engine/src/session/company_state.rs
 M packages/engine/src/session/corporate_actions/registry_day_end.rs
 M packages/engine/src/session/corporate_actions/rights_offering_day_end.rs
 M packages/engine/src/session/decision_chain.rs
 M packages/engine/src/session/execution/reconcile_plan.rs
 M packages/engine/src/session/failure.rs
 M packages/engine/src/session/pipeline/account_settlement_tests.rs
 M packages/engine/src/session/pipeline/decision_snapshot_capture.rs
 M packages/engine/src/session/pipeline/decision_snapshot_capture_tests.rs
 M packages/engine/src/session/pipeline/npc_decisions.rs
 M packages/engine/src/session/pipeline/npc_state_projection.rs
 M packages/engine/src/session/pipeline/npc_tick_preparation.rs
 M packages/engine/src/session/pipeline/stock_execution_transaction.rs
 M packages/engine/src/session/retail_lifecycle.rs
 M packages/engine/src/session/self_views.rs
 M packages/engine/src/thread_facade.rs
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-cfg-candidate2-2026-10-30`

```text
 M packages/engine/src/facade_slot/arena.rs
 M packages/engine/src/facade_slot/roles.rs
 M packages/engine/src/facade_slot/test_supervisor.rs
 M packages/engine/src/facade_slot/tests/exhaustion_tests.rs
 M packages/engine/src/facade_slot/tests/interleave_tests.rs
 M packages/engine/src/facade_slot/tests/lifecycle_tests.rs
 M packages/engine/src/market.rs
 M packages/engine/src/orderbook.rs
 M packages/engine/src/plans/mod.rs
 M packages/engine/src/runtime/desk.rs
 M packages/engine/src/runtime/drive.rs
 M packages/engine/src/runtime/drive/convergence.rs
 M packages/engine/src/runtime/group.rs
 M packages/engine/src/session/administrative_mirror.rs
 M packages/engine/src/session/attention.rs
 M packages/engine/src/session/civil_information.rs
 M packages/engine/src/session/company_state.rs
 M packages/engine/src/session/corporate_actions/registry_day_end.rs
 M packages/engine/src/session/corporate_actions/rights_offering_day_end.rs
 M packages/engine/src/session/decision_chain.rs
 M packages/engine/src/session/execution/reconcile_plan.rs
 M packages/engine/src/session/failure.rs
 M packages/engine/src/session/pipeline/decision_snapshot_capture.rs
 M packages/engine/src/session/pipeline/decision_snapshot_capture_tests.rs
 M packages/engine/src/session/pipeline/npc_decisions.rs
 M packages/engine/src/session/pipeline/npc_state_projection.rs
 M packages/engine/src/session/pipeline/npc_tick_preparation.rs
 M packages/engine/src/session/retail_lifecycle.rs
 M packages/engine/src/session/self_views.rs
 M packages/engine/src/thread_facade.rs
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-cfg-candidate2-approved-2026-10-30`

```text
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-cfg-candidate2-imports-2026-10-30`

```text
M  packages/engine/src/facade_slot/arena.rs
M  packages/engine/src/facade_slot/roles.rs
M  packages/engine/src/facade_slot/test_supervisor.rs
M  packages/engine/src/facade_slot/tests/exhaustion_tests.rs
M  packages/engine/src/facade_slot/tests/interleave_tests.rs
M  packages/engine/src/facade_slot/tests/lifecycle_tests.rs
M  packages/engine/src/market.rs
MM packages/engine/src/orderbook.rs
M  packages/engine/src/plans/mod.rs
M  packages/engine/src/runtime/desk.rs
M  packages/engine/src/runtime/drive.rs
M  packages/engine/src/runtime/drive/convergence.rs
M  packages/engine/src/runtime/group.rs
M  packages/engine/src/session/administrative_mirror.rs
M  packages/engine/src/session/attention.rs
M  packages/engine/src/session/civil_information.rs
M  packages/engine/src/session/company_state.rs
M  packages/engine/src/session/corporate_actions/registry_day_end.rs
M  packages/engine/src/session/corporate_actions/rights_offering_day_end.rs
M  packages/engine/src/session/decision_chain.rs
M  packages/engine/src/session/execution/reconcile_plan.rs
M  packages/engine/src/session/failure.rs
M  packages/engine/src/session/pipeline/decision_snapshot_capture.rs
M  packages/engine/src/session/pipeline/decision_snapshot_capture_tests.rs
MM packages/engine/src/session/pipeline/npc_decisions.rs
M  packages/engine/src/session/pipeline/npc_state_projection.rs
MM packages/engine/src/session/pipeline/npc_tick_preparation.rs
M  packages/engine/src/session/retail_lifecycle.rs
M  packages/engine/src/session/self_views.rs
M  packages/engine/src/thread_facade.rs
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-cfg-company-2026-10-30`

```text
 M packages/engine/src/company/bank/income_tax.rs
 M packages/engine/src/company/cash_dividend_tax.rs
 M packages/engine/src/company/correction.rs
 M packages/engine/src/company/disclosure_correction_proof.rs
 M packages/engine/src/company/industrial/chart.rs
 M packages/engine/src/company/industrial/income_tax.rs
 M packages/engine/src/company/insurance/income_tax.rs
 M packages/engine/src/company/operations.rs
 M packages/engine/src/company/operations/config.rs
 M packages/engine/src/company/operations/core.rs
 M packages/engine/src/company/real_estate/income_tax.rs
 M packages/engine/src/company/real_estate/mod.rs
 M packages/engine/src/company/simple/preferences.rs
 M packages/engine/src/company/system.rs
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-cfg-compile-seams-2026-10-30`

```text
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-cfg-compute-remain-2026-10-30`

```text
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-cfg-corporate-candidates-2026-10-30`

```text
 M packages/engine/src/session/corporate_actions/registry_day_end.rs
 M packages/engine/src/session/corporate_actions/rights_offering_day_end.rs
```

## `./.worktree/clippy-cfg-corporate-round3-2026-10-30`

```text
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-cfg-diagnostics-2026-10-30`

```text
 M packages/engine/src/company/cash_dividend.rs
 M packages/engine/src/company/industrial/inventory_sources.rs
 M packages/engine/src/runtime/group.rs
 M packages/engine/src/session/player_company_command_candidates.rs
 M packages/engine/src/session/protocol/civil/session.rs
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-cfg-diagnostics-rework-2026-10-30`

```text
 M packages/engine/src/company/cash_dividend.rs
 M packages/engine/src/company/industrial/inventory_sources.rs
 M packages/engine/src/runtime/group.rs
 M packages/engine/src/session/player_company_command_candidates.rs
 M packages/engine/src/session/protocol/civil/session.rs
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-cfg-driver-sync-testowner-2026-10-30`

```text
?? agents/sol-cfg-round4/
```

## `./.worktree/clippy-cfg-error-remain-2026-10-30`

```text
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-cfg-final12-2026-10-30`

```text
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-cfg-fixtures-2026-10-30`

```text
 M packages/engine/src/company/disclosure_correction_batch_proof_tests.rs
 M packages/engine/src/company/disclosure_correction_batch_tests.rs
 M packages/engine/src/company/disclosure_correction_owner_tests.rs
 M packages/engine/src/company/disclosure_correction_proof_tests.rs
 M packages/engine/src/company/disclosure_correction_test_fixture.rs
 M packages/engine/src/company/mod.rs
 M packages/engine/src/company/operations/disclosure_correction_legacy_tests.rs
 M packages/engine/src/runtime/calendar_public_restore_tests.rs
 M packages/engine/src/runtime/group/admin_transaction/information_tests.rs
 M packages/engine/src/runtime/group/admin_transaction/repurchase_approval_tests.rs
 M packages/engine/src/runtime/group/admin_transaction/repurchase_funds_tests.rs
 M packages/engine/src/runtime/group/admin_transaction/repurchase_source_tests.rs
 M packages/engine/src/runtime/mod.rs
 M packages/engine/src/runtime/world/admin_transaction/calendar_information_intersection_tests.rs
 M packages/engine/src/runtime/world/admin_transaction/funds_cost_tests.rs
 M packages/engine/src/runtime/world/admin_transaction/information_tests.rs
 M packages/engine/src/runtime/world/admin_transaction/repurchase_approval_tests.rs
 M packages/engine/src/runtime/world/admin_transaction/repurchase_funds_tests.rs
 M packages/engine/src/runtime/world/admin_transaction/stage_information_tests.rs
 M packages/engine/src/runtime/world/information_handover.rs
 M packages/engine/src/session/calendar_entry_cache_tests.rs
 M packages/engine/src/session/calendar_final_boundary_tests.rs
 M packages/engine/src/session/calendar_nonzero_plan_parity_tests.rs
 M packages/engine/src/session/disclosure_batch_candidate_tests.rs
 M packages/engine/src/session/disclosure_candidate_tests.rs
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-cfg-foundation-2026-10-30`

```text
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-cfg-npc-round3-2026-10-30`

```text
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-cfg-pipeline-2026-10-30`

```text
 M packages/engine/src/session/pipeline/adaptive_plan_chain.rs
 M packages/engine/src/session/pipeline/quote_expiry.rs
 M packages/engine/src/session/pipeline/read_only_start.rs
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-cfg-pipeline-stream-round3-2026-10-30`

```text
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-cfg-pipeline-txn-round3-2026-10-30`

```text
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-cfg-plan-round3-2026-10-30`

```text
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-cfg-runtime-exec-round3-2026-10-30`

```text
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-cfg-runtime-types-round3-2026-10-30`

```text
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-cfg-session-dead-2026-10-30`

```text
 M packages/engine/src/session/account_book.rs
 M packages/engine/src/session/attention.rs
 M packages/engine/src/session/candles.rs
 M packages/engine/src/session/exchange_calendar_model.rs
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-cfg-session-retail-2026-10-30`

```text
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-cfg-stream-rework-2026-10-30`

```text
 M packages/engine/src/session/pipeline/market_tick_transaction.rs
 M packages/engine/src/session/pipeline/stock_stream.rs
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-design-2026-10-30`

```text
 M apps/web-wasm/src/shutdown.rs
 M packages/engine/src/accounting/disclosure_roe.rs
 M packages/engine/src/accounting/disclosure_roe/tests.rs
 M packages/engine/src/accounting/reports/roe.rs
 M packages/engine/src/company/query.rs
 M packages/engine/src/company/simple/finance_settlement_entry_tests.rs
 M packages/engine/src/compute.rs
 M packages/engine/src/information/queries.rs
 M packages/engine/src/information/source_tests.rs
 M packages/engine/src/market.rs
 M packages/engine/src/runtime/bridge.rs
 M packages/engine/src/runtime/desk.rs
 M packages/engine/src/runtime/drive.rs
 M packages/engine/src/runtime/drive/convergence.rs
 M packages/engine/src/runtime/drive_tests.rs
 M packages/engine/src/runtime/group.rs
 M packages/engine/src/runtime/group/admin_transaction.rs
 M packages/engine/src/runtime/mod.rs
 M packages/engine/src/runtime/personal_full_validation.rs
 M packages/engine/src/runtime/protocol.rs
 A packages/engine/src/runtime/type_aliases.rs
 M packages/engine/src/runtime/world.rs
 M packages/engine/src/runtime/world/admin_transaction/stages.rs
 M packages/engine/src/runtime/world/shutdown_completion.rs
 M packages/engine/src/runtime/world/shutdown_completion/tests.rs
 M packages/engine/src/runtime/world/wasm_shutdown.rs
 M packages/engine/src/session.rs
 M packages/engine/src/session/company_assembly.rs
 M packages/engine/src/session/company_capital_restore.rs
 M packages/engine/src/session/company_command_candidates.rs
 M packages/engine/src/session/company_state.rs
 M packages/engine/src/session/company_state_tests.rs
 M packages/engine/src/session/corporate_actions/distributions_day_end.rs
 M packages/engine/src/session/corporate_actions/distributions_day_end_tests.rs
 M packages/engine/src/session/deferred_drop.rs
 A packages/engine/src/session/error_transport_tests.rs
 M packages/engine/src/session/failure.rs
 M packages/engine/src/session/pipeline/npc_decisions.rs
 M packages/engine/src/session/pipeline/npc_tick_preparation.rs
 M packages/engine/src/session/pipeline/plan_continuation_domain_tests.rs
 M packages/engine/src/session/plan_chain_candidates.rs
 M packages/engine/src/session/player_company_command_candidates.rs
 M packages/engine/src/session/player_company_command_classification.rs
 M packages/engine/src/session/player_company_commands.rs
 M packages/engine/src/session/shutdown.rs
 M packages/engine/src/session/stock_distribution_command_candidate.rs
 M packages/engine/src/strategy/fundamental/update.rs
```

## `./.worktree/clippy-desktop-private-errors-2026-10-30`

```text
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-doc-comments-2026-10-30`

```text
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-final-3fix-20261103`

```text
?? agents/clippy-final-3fix/
```

## `./.worktree/clippy-idioms-company-2026-10-30`

```text
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-idioms-docs413-2026-10-30`

```text
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-idioms-helper3265-2026-10-30`

```text
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-idioms-misc-2026-10-30`

```text
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-idioms-tests-2026-10-30`

```text
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-machine-json-2026-10-30`

```text
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-mut-cfg-repair-2026-10-30`

```text
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-public-payload-tests-2026-10-30`

```text
 M packages/engine/src/accounting/disclosure_roe/tests.rs
 M packages/engine/src/information/source_tests.rs
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-root-transport-2026-10-30`

```text
 M packages/engine/src/session/plan_chain_candidates.rs
```

## `./.worktree/clippy-server-41398b13-2026-10-30`

```text
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-server-mirror-origin-79f6-2026-10-30`

```text
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-server-probe-e59-2026-10-30`

```text
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-server-tests-09ff-2026-10-30`

```text
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/clippy-shutdown-layout-tests-2026-10-30`

```text
 M packages/engine/src/runtime/world/shutdown_completion/tests.rs
?? agents/branch-completion/clippy-ci-2026-10-30/
```

## `./.worktree/closed-day-protocol-retained-2026-10-30`

```text
?? agents/branch-completion/closed-day-protocol-retained-2026-10-30/
```

## `./.worktree/company-b4e-repurchase-anchor-red-2026-10-28`

```text
 M packages/engine/src/session/player_company_command_candidates.rs
```

## `./.worktree/company-catalog-bc`

```text
?? agents/branch-completion/company-catalog-bc/app-lint-baseline/
?? agents/branch-completion/company-catalog-bc/desktop-ready-r2/
?? agents/branch-completion/company-catalog-bc/desktop-ready/
?? agents/branch-completion/company-catalog-bc/engine-3019-green-ready/
?? agents/branch-completion/company-catalog-bc/full-review-freeze-r1/
?? agents/branch-completion/company-catalog-bc/full-review-freeze-r2/
?? agents/branch-completion/company-catalog-bc/full-review-freeze-r3/
?? agents/branch-completion/company-catalog-bc/full-review-freeze-r4/evidence/
?? agents/branch-completion/company-catalog-bc/full-review-freeze-r4/relative-r3-paths.txt
?? agents/branch-completion/company-catalog-bc/full-review-freeze-r4/source/
?? agents/branch-completion/company-catalog-bc/intent-slice/
?? agents/branch-completion/company-catalog-bc/main-combination-native-r7/
?? agents/branch-completion/company-catalog-bc/restore-review-freeze-r3/
?? agents/branch-completion/company-catalog-bc/restore-review-freeze/
?? agents/branch-completion/company-catalog-bc/restore-review-lint-r4/
?? agents/branch-completion/company-catalog-bc/review-fixes-r3/
?? agents/branch-completion/company-catalog-bc/review-fixes-r4/
?? agents/branch-completion/company-catalog-bc/rust-format-scope-r2/
?? agents/branch-completion/company-catalog-bc/rust-prepared-red-r2/
?? agents/branch-completion/company-catalog-bc/rust-prepared-red/
?? agents/branch-completion/company-catalog-bc/server-ready-r2/
?? agents/branch-completion/company-catalog-bc/server-ready-r3/
?? agents/branch-completion/company-catalog-bc/server-ready-r4/
?? agents/branch-completion/company-catalog-bc/server-ready/
?? agents/branch-completion/company-catalog-bc/web-wasm-ready/
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/build.stderr
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/build.stdout
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/case-0.stderr
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/case-0.stdout
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/case-1.stderr
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/case-1.stdout
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/case-10.stderr
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/case-10.stdout
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/case-11.stderr
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/case-11.stdout
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/case-12.stderr
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/case-12.stdout
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/case-13.stderr
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/case-13.stdout
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/case-14.stderr
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/case-14.stdout
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/case-15.stderr
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/case-15.stdout
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/case-16.stderr
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/case-16.stdout
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/case-17.stderr
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/case-17.stdout
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/case-18.stderr
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/case-18.stdout
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/case-2.stderr
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/case-2.stdout
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/case-3.stderr
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/case-3.stdout
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/case-4.stderr
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/case-4.stdout
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/case-5.stderr
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/case-5.stdout
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/case-6.stderr
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/case-6.stdout
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/case-7.stderr
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/case-7.stdout
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/case-8.stderr
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/case-8.stdout
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/case-9.stderr
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/case-9.stdout
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/metadata.stderr
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/metadata.stdout
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/native-guard-resources.jsonl
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/native-guard.stderr
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/native-guard.stdout
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/readback-generated-company-state-registry.json.stderr
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/readback-generated-company-state-registry.json.stdout
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/readback-generated-custom-monthly-nonempty.json.stderr
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/readback-generated-custom-monthly-nonempty.json.stdout
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/readback-generated-quarterly-nonempty.json.stderr
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/readback-generated-quarterly-nonempty.json.stdout
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/readback-legacy-company-state.json.stderr
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/readback-legacy-company-state.json.stdout
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/readback-manual-company-state-registry.json.stderr
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/readback-manual-company-state-registry.json.stdout
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/readback-manual-custom-monthly-nonempty.json.stderr
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/readback-manual-custom-monthly-nonempty.json.stdout
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/readback-manual-quarterly-nonempty.json.stderr
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/readback-manual-quarterly-nonempty.json.stdout
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/readback-nonempty-legacy-group.json.stderr
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/readback-nonempty-legacy-group.json.stdout
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/readback.rs
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/reader-build.stderr
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/reader-build.stdout
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/run-batch.py
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/seal-single-i2-unit.stderr
?? agents/branch-completion/source-b-web-strict-save/srcb-combination-native-v1/seal-single-i2-unit.stdout
?? node_modules
```

## `./.worktree/company-contract-parsers`

```text
?? node_modules
```

## `./.worktree/company-pure-inputs`

```text
?? node_modules
```

## `./.worktree/company-r4-production`

```text
 M agents/branch-completion/r4-production/run-clock-fixture.mjs
?? agents/branch-completion/r4-production/administrative-adapter-design-review-2026-10-28.md
?? agents/branch-completion/r4-production/administrative-adapter-implementation-2026-10-28/
?? agents/branch-completion/r4-production/administrative-adapter-three-red-2026-10-28/
?? agents/branch-completion/r4-production/administrative-civil-projection-implementation-2026-10-28/
?? agents/branch-completion/r4-production/administrative-inspection-compatibility-design-review-2026-10-28.md
?? agents/branch-completion/r4-production/administrative-projection-design-review-2026-10-28.md
?? agents/branch-completion/r4-production/approval-cutoff-design-review-2026-10-28.md
?? agents/branch-completion/r4-production/baseline-alignment-2026-10-28/
?? agents/branch-completion/r4-production/capital-approval-context-design-review-2026-10-28.md
?? agents/branch-completion/r4-production/capital-command-wip-independent-review-2026-10-28.md
?? agents/branch-completion/r4-production/correction-parent-transaction-2026-10-28.md
?? agents/branch-completion/r4-production/correction-parent-transaction-2026-10-28/
?? agents/branch-completion/r4-production/e42-alignment-index-independent-review-2026-10-28.md
?? agents/branch-completion/r4-production/f6-production-contract-review-2026-10-28.md
?? agents/branch-completion/r4-production/f6-production-integration-2026-10-28.md
?? agents/branch-completion/r4-production/f6-save-boundary-design-cd6-2026-10-28.md
?? agents/branch-completion/r4-production/f6-scope-addendum-independent-review-2026-10-28.md
?? agents/branch-completion/r4-production/f6-state-integration-map-2026-10-28.md
?? agents/branch-completion/r4-production/four-session-2026-10-28/
?? agents/branch-completion/r4-production/independent-review-stage-v3-2026-10-27.md
?? [疑似日志、采样或敏感路径已遮蔽]
?? [疑似日志、采样或敏感路径已遮蔽]
?? [疑似日志、采样或敏感路径已遮蔽]
?? [疑似日志、采样或敏感路径已遮蔽]
?? agents/branch-completion/r4-production/ordinary-tax-production-2026-10-28/dependency-alignment/
?? agents/branch-completion/r4-production/ordinary-tax-production-2026-10-28/freeze-01/
?? agents/branch-completion/r4-production/ordinary-tax-production-2026-10-28/joint-first-red-handoff/
?? agents/branch-completion/r4-production/ordinary-tax-production-2026-10-28/prepare-alignment.py
?? agents/branch-completion/r4-production/ordinary-tax-production-2026-10-28/r1-metadata-green.stderr
?? agents/branch-completion/r4-production/ordinary-tax-production-2026-10-28/r1-metadata-green.stdout
?? agents/branch-completion/r4-production/ordinary-tax-production-2026-10-28/r1-r2-metadata-green.stderr
?? agents/branch-completion/r4-production/ordinary-tax-production-2026-10-28/r1-r2-metadata-green.stdout
?? agents/branch-completion/r4-production/ordinary-tax-production-2026-10-28/r2-metadata-red.stderr
?? agents/branch-completion/r4-production/ordinary-tax-production-2026-10-28/r2-metadata-red.stdout
?? agents/branch-completion/r4-production/ordinary-tax-production-2026-10-28/runner-freeze-01/
?? agents/branch-completion/r4-production/ordinary-tax-production-2026-10-28/runner-freeze-02/
?? agents/branch-completion/r4-production/ordinary-tax-production-2026-10-28/runner-freeze-03/
?? agents/branch-completion/r4-production/pending-correction-save-2026-10-28/
?? agents/branch-completion/r4-production/production-source-followup-2026-10-28.md
?? agents/branch-completion/r4-production/resume-2026-10-27/b4e-pre-resume-dirty.tar.gz
?? agents/branch-completion/r4-production/resume-2026-10-27/cash-stock-session-v1-source.tar.gz
?? agents/branch-completion/r4-production/resume-2026-10-27/cash-stock-session-v1.patch
?? agents/branch-completion/r4-production/resume-2026-10-27/cash-stock-session-v2-source.tar.gz
?? agents/branch-completion/r4-production/resume-2026-10-27/cash-stock-session-v2.patch
?? agents/branch-completion/r4-production/resume-2026-10-27/pre-resume-dirty.tar.gz
?? agents/branch-completion/r4-production/resume-2026-10-27/v1-fixed-test-source.tar.gz
?? agents/branch-completion/r4-production/resume-2026-10-27/v1-red1/
?? agents/branch-completion/r4-production/resume-2026-10-27/v1-red2/
?? agents/branch-completion/r4-production/resume-2026-10-27/v2-fixed-test-source.tar.gz
?? agents/branch-completion/r4-production/resume-2026-10-27/v2-green1/
?? agents/branch-completion/r4-production/resume-2026-10-27/v2-lazy-fix-source.tar.gz
?? agents/branch-completion/r4-production/resume-2026-10-27/v2-lazy-fix.patch
?? agents/branch-completion/r4-production/stage-contract-v3-2026-10-27.md
?? agents/branch-completion/r4-production/stage-evidence-execution-2026-10-27.md
?? agents/branch-completion/r4-production/stage-evidence-fixture.rs
?? agents/branch-completion/r4-production/stage-hunk-map-51ca-2026-10-27.md
?? agents/branch-completion/r4-production/stage-minimal-interface-cd6-2026-10-28.md
?? agents/branch-completion/r4-production/stage-production-tests-first-cd6-2026-10-28.md
?? agents/branch-completion/r4-production/tax-stage-view-v1-2026-10-27.md
?? packages/engine/src/runtime/admin_stage_evidence.rs
```

## `./.worktree/company-read-panels`

```text
?? node_modules
```

## `./.worktree/company-schedule-inputs`

```text
?? node_modules
```

## `./.worktree/company-source-read-facts`

```text
 M packages/engine/src/session.rs
 M packages/engine/src/session/decision_chain.rs
 M packages/engine/src/session/decision_chain/retail_analysis.rs
 M packages/engine/src/session/decision_chain/roots.rs
 M packages/engine/src/session/pipeline/npc_split.rs
?? packages/engine/src/session/company_facts.rs
?? packages/engine/src/session/decision_chain/company_source_tests.rs
```

## `./.worktree/company-system-inputs`

```text
?? node_modules
```

## `./.worktree/company-write-host-2026-10-28`

```text
 M agents/branch-completion/company-write-host-2026-10-28/README.md
 M agents/branch-completion/company-write-host-2026-10-28/red-matrix.md
M  apps/desktop/src-tauri/src/actor.rs
A  apps/desktop/src-tauri/src/actor/correction_recovery_tests.rs
M  apps/server/src/actor.rs
A  apps/server/src/actor/correction_recovery_tests.rs
A  apps/web-wasm/src/company_write_tests.rs
M  apps/web-wasm/src/lib.rs
M  packages/engine/src/runtime/desk.rs
A  packages/engine/src/runtime/desk/repurchase.rs
A  packages/engine/src/runtime/desk/repurchase_group_fixture.rs
A  packages/engine/src/runtime/desk_repurchase_transport_tests.rs
M  packages/engine/src/runtime/group.rs
M  packages/engine/src/runtime/group/admin_transaction.rs
A  packages/engine/src/runtime/group/admin_transaction/repurchase_source_tests.rs
M  packages/engine/src/runtime/group/origin_consumer_tests.rs
M  packages/engine/src/runtime/group/repurchase_order_fact.rs
M  packages/engine/src/runtime/mod.rs
M  packages/engine/src/runtime/protocol.rs
M  packages/engine/src/runtime/protocol_tests.rs
A  packages/engine/src/runtime/repurchase_transport.rs
M  packages/engine/src/session/issuer_repurchase_approval_tests.rs
A  packages/engine/src/session/player_company_classification_tests.rs
M  packages/engine/src/session/player_company_commands.rs
M  packages/engine/src/session/player_company_commands_tests.rs
M  packages/engine/src/session/protocol.rs
M  packages/engine/src/session/protocol/civil/session.rs
A  packages/engine/src/session/protocol/company_writes.rs
A  packages/engine/tests/company_r4_tax_production.rs
?? agents/branch-completion/company-write-host-2026-10-28/combined-red-01/b4-actor-tests.patch
?? agents/branch-completion/company-write-host-2026-10-28/combined-red-01/complete-product-v2.diff
?? agents/branch-completion/company-write-host-2026-10-28/combined-red-01/complete-product-v3.diff
?? agents/branch-completion/company-write-host-2026-10-28/combined-red-01/complete-product.diff
?? agents/branch-completion/company-write-host-2026-10-28/combined-red-01/execution-handoff-2026-10-29.md
?? agents/branch-completion/company-write-host-2026-10-28/combined-red-01/joint-native-and-export-02/
?? agents/branch-completion/company-write-host-2026-10-28/combined-red-01/js-export-tap-only-2026-10-29/
?? agents/branch-completion/company-write-host-2026-10-28/combined-red-01/native-and-export-01/
?? agents/branch-completion/company-write-host-2026-10-28/combined-red-01/wasm-export.test.mjs
?? agents/branch-completion/company-write-host-2026-10-28/negative-source-diagnosis-2026-10-29.md
```

## `./.worktree/company-write-impl-2026-10-29`

```text
 M apps/desktop/src-tauri/src/actor.rs
 M apps/desktop/src-tauri/src/actor/correction_recovery_tests.rs
 M apps/desktop/src-tauri/src/actor/failure.rs
 M apps/desktop/src-tauri/src/lib.rs
 M apps/server/src/actor.rs
 M apps/server/src/lib.rs
 M apps/server/src/routes.rs
 M apps/server/src/routes/company_write_tests.rs
 M apps/web-wasm/src/lib.rs
 M packages/engine/src/session/issuer_repurchase_approval.rs
 M packages/engine/src/session/player_company_commands.rs
 M packages/engine/src/session/protocol.rs
 M packages/engine/src/session/protocol/company_writes.rs
 M packages/engine/tests/company_r4_tax_production.rs
?? agents/branch-completion/company-write-impl-2026-10-29/
?? packages/engine/src/session/player_company_command_classification.rs
```

## `./.worktree/company-write-web-2026-10-28`

```text
UU apps/web/src/host/wasm-worker-ownership.test.ts
UU apps/web/src/host/wasm-worker.ts
A  apps/web/src/host/worker-host-company-write.test.ts
UU apps/web/src/host/worker-host.ts
A  apps/web/src/host/worker-request-company-write.test.ts
M  apps/web/src/host/worker-request.ts
?? agents/branch-completion/company-write-web-2026-10-28/worker-integration/
```

## `./.worktree/control-worker-2026-10-30`

```text
?? node_modules
```

## `./.worktree/d3-p1-integration-2026-10-28`

```text
 M packages/engine/src/session/ordinary_tax_day_end.rs
?? agents/branch-completion/host-receive-2026-10-29/
?? agents/branch-completion/joint-43-2026-10-29/
?? agents/branch-completion/joint-green41-2026-10-29/
?? agents/branch-completion/joint-green50-2026-10-29/
?? agents/branch-completion/joint-green50-r2-2026-10-29/
?? agents/branch-completion/joint-next-2026-10-29/
?? agents/branch-completion/joint-next-r2-2026-10-29/
?? agents/branch-completion/joint-red-2026-10-29/
?? agents/branch-completion/joint-red-r2-2026-10-29/
?? agents/branch-completion/wasm-w1/combined/
?? packages/engine/src/session/ordinary_tax_day_end/
```

## `./.worktree/day-end-public-views`

```text
 M packages/engine/src/runtime/desk.rs
 M packages/engine/src/runtime/group.rs
 M packages/engine/src/runtime/mod.rs
 M packages/engine/src/runtime/protocol.rs
 M packages/engine/src/runtime/world.rs
?? packages/engine/src/runtime/day_end_public_views_tests.rs
```

## `./.worktree/dayend-partial-start-fix`

```text
 M apps/server/tests/api_contract.rs
 M packages/engine/src/runtime/desk.rs
 M packages/engine/src/runtime/drive.rs
 M packages/engine/src/runtime/group.rs
 M packages/engine/src/runtime/mod.rs
 M packages/engine/src/runtime/protocol.rs
 M packages/engine/src/runtime/world.rs
 M packages/std-thread-proto/src/lib.rs
 M packages/std-thread-proto/src/oworker.rs
 M packages/std-thread-proto/src/worker.rs
 M packages/std-thread-proto/src/xworker.rs
?? packages/engine/src/runtime/day_end_public_views_tests.rs
?? packages/engine/src/runtime/pending_cash_tests.rs
?? packages/engine/src/runtime/treasury_autonomy_tests.rs
?? packages/std-thread-proto/src/stock_identity.rs
```

## `./.worktree/deadline-hardgate-fix`

```text
 M apps/server/tests/api_contract.rs
 M docs/trading-rules.md
 M packages/engine/src/runtime/desk.rs
 M packages/engine/src/runtime/drive.rs
 M packages/engine/src/runtime/group.rs
 M packages/engine/src/runtime/mod.rs
 M packages/engine/src/runtime/protocol.rs
 M packages/engine/src/runtime/world.rs
 M packages/engine/src/session.rs
 M packages/std-thread-proto/src/lib.rs
 M packages/std-thread-proto/src/oworker.rs
 M packages/std-thread-proto/src/worker.rs
 M packages/std-thread-proto/src/xworker.rs
 M scripts/run-full-regression.mjs
 M scripts/run-full-regression.test.mjs
 M scripts/run-with-deadline.mjs
 M scripts/run-with-deadline.test.mjs
 M scripts/verify/asm-equiv-cmp-regression.py
 M scripts/verify/asm-equiv-cmp.py
?? packages/engine/src/runtime/day_end_public_views_tests.rs
?? packages/engine/src/runtime/group_roster_consumer_tests.rs
?? packages/engine/src/runtime/pending_cash_tests.rs
?? packages/engine/src/runtime/treasury_autonomy_tests.rs
?? packages/std-thread-proto/src/stock_identity.rs
?? scripts/full-regression-artifact-hash-worker.mjs
?? scripts/full-regression-artifact-hash.mjs
?? scripts/full-regression-artifact-hash.test.mjs
?? scripts/rust-test-case-supervisor-lifecycle.test.mjs
?? scripts/rust-test-case-supervisor-proc.test.mjs
?? scripts/rust-test-case-supervisor.mjs
?? scripts/rust-test-case-supervisor.test.mjs
```

## `./.worktree/desktop-ipc-contract-fix`

```text
 M apps/desktop/src-tauri/src/lib_tests.rs
 M apps/server/tests/api_contract.rs
 M docs/trading-rules.md
 M packages/engine/src/runtime/desk.rs
 M packages/engine/src/runtime/drive.rs
 M packages/engine/src/runtime/group.rs
 M packages/engine/src/runtime/mod.rs
 M packages/engine/src/runtime/protocol.rs
 M packages/engine/src/runtime/world.rs
 M packages/engine/src/session.rs
 M packages/std-thread-proto/src/lib.rs
 M packages/std-thread-proto/src/oworker.rs
 M packages/std-thread-proto/src/worker.rs
 M packages/std-thread-proto/src/xworker.rs
 M scripts/run-full-regression.mjs
 M scripts/run-full-regression.test.mjs
 M scripts/verify/asm-equiv-cmp-regression.py
 M scripts/verify/asm-equiv-cmp.py
?? packages/engine/src/runtime/day_end_public_views_tests.rs
?? packages/engine/src/runtime/group_roster_consumer_tests.rs
?? packages/engine/src/runtime/pending_cash_tests.rs
?? packages/engine/src/runtime/treasury_autonomy_tests.rs
?? packages/std-thread-proto/src/stock_identity.rs
?? scripts/full-regression-artifact-hash-worker.mjs
?? scripts/full-regression-artifact-hash.mjs
?? scripts/full-regression-artifact-hash.test.mjs
```

## `./.worktree/desktop-matrix-fixture`

```text
 M scripts/desktop/build-matrix.test.mjs
 M scripts/run-with-deadline.mjs
```

## `./.worktree/development-main-2026-10-29`

```text
?? agents/branch-completion/wasm-w2/
?? agents/development-main/
?? node_modules
```

## `./.worktree/diagnostics-feature-fix`

```text
 M packages/engine/src/session.rs
 M packages/engine/src/session/civil_clock.rs
 M packages/engine/src/session/failure.rs
 M packages/engine/src/session/pipeline/plan_owner_tests.rs
 M packages/engine/src/session/protocol/civil/session.rs
 M packages/engine/tests/diagnostics.rs
```

## `./.worktree/diagnostics-plan-identity-check`

```text
M  packages/engine/src/diagnostics/causal.rs
M  packages/engine/src/diagnostics/causal/aggregate.rs
M  packages/engine/src/diagnostics/causal/microstructure.rs
M  packages/engine/src/session/causal.rs
M  packages/engine/src/session/notices.rs
M  packages/engine/src/session/pipeline/continuous_tick_finalizer.rs
M  packages/engine/src/session/pipeline/continuous_tick_finalizer_tests.rs
M  packages/engine/tests/causal_diagnostics.rs
M  packages/engine/tests/company_scale.rs
M  packages/engine/tests/diagnostics.rs
```

## `./.worktree/diagnostics-retained-check`

```text
扫描错误：git status 在 25 秒后超时，状态未知。
```

## `./.worktree/drain-residual-six-2026-10-30`

```text
?? agents/branch-completion/remaining-regression-2026-10-30/
```

## `./.worktree/escrow-harness-contract`

```text
 M scripts/run-with-deadline.mjs
 M scripts/simulation/escrow-performance-harness.test.mjs
?? agents/escrow-harness-contract/
```

## `./.worktree/exempt-setup-fix`

```text
?? agents/branch-completion/exempt-setup-fix/build-resource-snapshot.txt
?? agents/branch-completion/exempt-setup-fix/epsilon-revision/
?? agents/branch-completion/exempt-setup-fix/official-stamp-tax-2026-10-25.html
```

## `./.worktree/fix-roster-regression-20261103`

```text
?? agents/fix-roster-regression-20261103/
```

## `./.worktree/fix-solmid-b57066ea`

```text
?? agents/fix-solmid/
```

## `./.worktree/full-regression-hash-parallel`

```text
 M scripts/run-full-regression.mjs
 M scripts/run-full-regression.test.mjs
?? scripts/full-regression-artifact-hash-worker.mjs
?? scripts/full-regression-artifact-hash.mjs
?? scripts/full-regression-artifact-hash.test.mjs
```

## `./.worktree/groups2-purse`

```text
?? packages/engine/examples/b4c3_scratch.rs
```

## `./.worktree/holiday-gate`

```text
 M packages/engine/src/session/civil_clock.rs
 M packages/engine/src/session/failure.rs
 M packages/engine/tests/session.rs
?? .tmp-caldates.rs
?? .tmp-fail-all.txt
?? .tmp-fail-lib.txt
```

## `./.worktree/integration-117-118-121-124-126`

```text
 M apps/desktop/src-tauri/src/lib_tests.rs
 M apps/server/tests/api_contract.rs
 M apps/server/tests/ws.rs
 M apps/web/src/host/wasm-worker-failure.test.ts
 M apps/web/src/host/wasm-worker-ownership.test.ts
 M apps/web/src/host/wasm-worker.ts
 M docs/trading-rules.md
 M packages/engine/src/information/public_view.rs
MM packages/engine/src/runtime/desk.rs
M  packages/engine/src/runtime/drive.rs
M  packages/engine/src/runtime/group.rs
M  packages/engine/src/runtime/mod.rs
M  packages/engine/src/runtime/protocol.rs
MM packages/engine/src/runtime/world.rs
 M packages/engine/src/session.rs
 M packages/engine/src/session/persistence/saved_runtime_tests.rs
 M packages/engine/tests/company_scenarios/lifecycle.rs
M  packages/std-thread-proto/src/lib.rs
M  packages/std-thread-proto/src/oworker.rs
M  packages/std-thread-proto/src/worker.rs
M  packages/std-thread-proto/src/xworker.rs
 M scripts/desktop/build-matrix.test.mjs
 M scripts/run-full-regression.mjs
 M scripts/run-full-regression.test.mjs
 M scripts/run-with-deadline.mjs
 M scripts/run-with-deadline.test.mjs
 M scripts/simulation/escrow-performance-harness.test.mjs
 M scripts/verify/asm-equiv-cmp-regression.py
 M scripts/verify/asm-equiv-cmp.py
 M scripts/wasm-build-dependencies.test.mjs
?? packages/engine/src/runtime/day_end_public_views_tests.rs
?? packages/engine/src/runtime/group_roster_consumer_tests.rs
?? packages/engine/src/runtime/pending_cash_tests.rs
?? packages/engine/src/runtime/treasury_autonomy_tests.rs
?? packages/std-thread-proto/src/stock_identity.rs
?? scripts/full-regression-artifact-hash-worker.mjs
?? scripts/full-regression-artifact-hash.mjs
?? scripts/full-regression-artifact-hash.test.mjs
?? scripts/native-build-dependencies.test.mjs
?? scripts/rust-test-case-supervisor-execution.test.mjs
?? scripts/rust-test-case-supervisor-lifecycle.test.mjs
?? scripts/rust-test-case-supervisor-proc.test.mjs
?? scripts/rust-test-case-supervisor.mjs
?? scripts/rust-test-case-supervisor.test.mjs
?? scripts/server-build-dependencies.test.mjs
```

## `./.worktree/isolate-export-tests`

```text
 M docs/testing.md
 M scripts/full-regression-execution.test.mjs
 M scripts/run-full-regression.mjs
 M scripts/rust-test-case-supervisor.mjs
 M scripts/rust-test-case-supervisor.test.mjs
```

## `./.worktree/issue119-closing-pre-d0`

```text
扫描错误：git status 在 25 秒后超时，状态未知。
```

## `./.worktree/last-integration38-skeleton-2026-10-30`

```text
?? agents/branch-completion/last-integration-2026-10-30/gold-probe/forward_transform_v2.diff
?? agents/branch-completion/last-integration-2026-10-30/gold-probe/forward_transform_v4.diff
```

## `./.worktree/m2-producer-20261029`

```text
 M docs/trading-rules.md
 M packages/engine/src/runtime/desk.rs
 M packages/engine/src/runtime/diagnostics.rs
 M packages/engine/src/session/causal.rs
 M packages/engine/src/session/runtime_diagnostics_tests.rs
 M packages/engine/src/session/runtime_diagnostics_tests/rich_lifecycle_tests.rs
 M packages/engine/tests/runtime_diagnostics_d1.rs
?? agents/branch-completion/m2-producer-2026-10-29/
```

## `./.worktree/main-merge-2026-10-25`

```text
 M packages/engine/src/lib.rs
?? agents/perf-remeasure-20261103/
?? agents/worktree-reinspection-20261104/
?? packages/engine/src/wasm_tombstone.rs
?? packages/engine/src/wasm_tombstone_tests.rs
```

## `./.worktree/mixed-calendar-c2c`

```text
 M packages/engine/examples/runtime_s5_probe.rs
?? agents/branch-completion/mixed-calendar-c2c/merge-main-09-backup/
?? agents/branch-completion/mixed-calendar-c2c/merge-main-cd6-15-backup/
?? agents/branch-completion/mixed-calendar-c2c/resume-live/
?? agents/branch-completion/mixed-calendar-c2c/run-resume-old-feedback-2026-10-27.py
```

## `./.worktree/mixed-calendar-final-c2c-2026-10-28`

```text
 M packages/engine/src/runtime/calendar_chain_caller_tests.rs
 M packages/engine/src/runtime/coordinator.rs
 M packages/engine/src/runtime/drive_tests.rs
 M packages/engine/src/runtime/group.rs
 M packages/engine/src/runtime/protocol.rs
 M packages/engine/src/runtime/seal_tests.rs
 M packages/engine/src/runtime/world.rs
?? agents/branch-completion/mixed-calendar-c2c/final-source-preparation-26/
?? agents/branch-completion/mixed-calendar-c2c/next-v1-entry-cache-plan/
?? agents/branch-completion/mixed-calendar-c2c/next-v10-info-calendar-plan/
?? agents/branch-completion/mixed-calendar-c2c/next-v11-real-trade-plan/
?? agents/branch-completion/mixed-calendar-c2c/next-v12-final-boundary-plan/
?? agents/branch-completion/mixed-calendar-c2c/next-v7-closed-collector-plan/
?? agents/branch-completion/mixed-calendar-c2c/next-v7-lifecycle-plan/
?? agents/branch-completion/mixed-calendar-c2c/next-v7-native-serial-plan-parity/
?? agents/branch-completion/mixed-calendar-c2c/next-v7-nonzero-day-end-abort-plan/
?? agents/branch-completion/mixed-calendar-c2c/real-lifecycle-witness-26/
```

## `./.worktree/mixed-calendar-main-c2c-2026-10-28`

```text
 M packages/engine/examples/runtime_s5_probe.rs
?? agents/branch-completion/mixed-calendar-c2c/caller-and-example-first-23/
?? agents/branch-completion/mixed-calendar-c2c/example-green-exact-hunks.patch
?? agents/branch-completion/mixed-calendar-c2c/main-combination-22/
?? agents/branch-completion/mixed-calendar-c2c/main-combination-22r2-example/
?? agents/branch-completion/mixed-calendar-c2c/main-combination-22r3-normal-initial-measure/
?? agents/branch-completion/mixed-calendar-c2c/main-combination-22r4-clock-input-equality/
?? agents/branch-completion/mixed-calendar-c2c/next-public-restore-candidate/
?? agents/branch-completion/mixed-calendar-c2c/next-v10-info-calendar-plan/
?? agents/branch-completion/mixed-calendar-c2c/next-v11-real-trade-plan/
?? agents/branch-completion/mixed-calendar-c2c/next-v12-final-boundary-plan/
?? agents/branch-completion/mixed-calendar-c2c/next-v7-chain-caller-candidate/
?? agents/branch-completion/mixed-calendar-c2c/next-v7-lifecycle-plan/
?? agents/branch-completion/mixed-calendar-c2c/strict-restore-and-caller-24/
?? agents/branch-completion/mixed-calendar-c2c/strict-restore-and-caller-24r2/
?? agents/branch-completion/mixed-calendar-c2c/strict-restore-clock-oracle-25/
```

## `./.worktree/mixed-calendar-resume-oracle-c2c-2026-10-28`

```text
 M packages/engine/src/runtime/calendar_chain_caller_tests.rs
 M packages/engine/src/runtime/coordinator.rs
 M packages/engine/src/runtime/drive_tests.rs
 M packages/engine/src/runtime/group.rs
 M packages/engine/src/runtime/protocol.rs
 M packages/engine/src/runtime/seal_tests.rs
 M packages/engine/src/runtime/world.rs
?? agents/branch-completion/mixed-calendar-c2c/resume-feedback-oracle-27/
?? agents/branch-completion/mixed-calendar-c2c/resume-feedback-oracle-27r2/
```

## `./.worktree/mixed-calendar-v1-entry-cache-c2c-2026-10-28`

```text
 M packages/engine/src/runtime/coordinator.rs
 M packages/engine/src/runtime/world.rs
 M packages/engine/src/session.rs
 M packages/engine/src/session/failure.rs
 M packages/engine/src/session/shared_ingress.rs
?? agents/branch-completion/mixed-calendar-c2c/v1-entry-cache-source-29/
?? packages/engine/src/session/calendar_entry_cache_tests.rs
```

## `./.worktree/mixed-calendar-v10-final-c2c-2026-10-28`

```text
 M packages/engine/src/runtime/world/admin_transaction.rs
?? agents/branch-completion/mixed-calendar-c2c/v10-pending-market-source-31/
?? packages/engine/src/runtime/world/admin_transaction/calendar_information_intersection_tests.rs
```

## `./.worktree/mixed-calendar-v12-final-c2c-2026-10-28`

```text
 M packages/engine/src/session.rs
?? agents/branch-completion/mixed-calendar-c2c/v12-final-boundary-source-27/
?? packages/engine/src/session/calendar_final_boundary_tests.rs
```

## `./.worktree/mixed-calendar-v7-gaps-c2c-2026-10-28`

```text
 M packages/engine/src/runtime/calendar_chain_caller_tests.rs
 M packages/engine/src/runtime/calendar_group_day_end_tests.rs
 M packages/engine/src/runtime/coordinator.rs
 M packages/engine/src/runtime/drive_tests.rs
 M packages/engine/src/runtime/group.rs
 M packages/engine/src/runtime/protocol.rs
 M packages/engine/src/runtime/seal_tests.rs
 M packages/engine/src/runtime/world.rs
 M packages/engine/src/session.rs
?? agents/branch-completion/mixed-calendar-c2c/v7-gap-source-30/
?? packages/engine/src/session/calendar_nonzero_plan_parity_tests.rs
```

## `./.worktree/native-store-deadline-diagnosis`

```text
 M packages/engine/src/information/public_view.rs
 M packages/engine/src/runtime/desk.rs
 M packages/engine/src/runtime/drive.rs
 M packages/engine/src/runtime/group.rs
 M packages/engine/src/runtime/mod.rs
 M packages/engine/src/runtime/protocol.rs
 M packages/engine/src/runtime/world.rs
 M packages/engine/src/session.rs
 M packages/engine/src/session/persistence/saved_runtime_tests.rs
 M packages/std-thread-proto/src/lib.rs
 M packages/std-thread-proto/src/oworker.rs
 M packages/std-thread-proto/src/worker.rs
 M packages/std-thread-proto/src/xworker.rs
 M scripts/run-full-regression.mjs
 M scripts/run-full-regression.test.mjs
 M scripts/run-with-deadline.mjs
 M scripts/run-with-deadline.test.mjs
 M scripts/verify/asm-equiv-cmp-regression.py
 M scripts/verify/asm-equiv-cmp.py
?? packages/engine/src/runtime/day_end_public_views_tests.rs
?? packages/engine/src/runtime/group_roster_consumer_tests.rs
?? packages/engine/src/runtime/pending_cash_tests.rs
?? packages/engine/src/runtime/treasury_autonomy_tests.rs
?? packages/std-thread-proto/src/stock_identity.rs
?? scripts/full-regression-artifact-hash-worker.mjs
?? scripts/full-regression-artifact-hash.mjs
?? scripts/full-regression-artifact-hash.test.mjs
?? scripts/rust-test-case-supervisor-lifecycle.test.mjs
?? scripts/rust-test-case-supervisor.mjs
?? scripts/rust-test-case-supervisor.test.mjs
```

## `./.worktree/nested-deadline-diagnosis`

```text
 M scripts/desktop/build-matrix.test.mjs
 M scripts/run-full-regression.mjs
 M scripts/run-full-regression.test.mjs
 M scripts/run-with-deadline.mjs
 M scripts/run-with-deadline.test.mjs
 M scripts/simulation/escrow-performance-harness.test.mjs
 M scripts/verify/asm-equiv-cmp-regression.py
 M scripts/verify/asm-equiv-cmp.py
 M scripts/wasm-build-dependencies.test.mjs
?? scripts/full-regression-artifact-hash-worker.mjs
?? scripts/full-regression-artifact-hash.mjs
?? scripts/full-regression-artifact-hash.test.mjs
?? scripts/native-build-dependencies.test.mjs
?? scripts/rust-test-case-supervisor-execution.test.mjs
?? scripts/rust-test-case-supervisor-lifecycle.test.mjs
?? scripts/rust-test-case-supervisor-proc.test.mjs
?? scripts/rust-test-case-supervisor.mjs
?? scripts/rust-test-case-supervisor.test.mjs
?? scripts/server-build-dependencies.test.mjs
```

## `./.worktree/orderid-fixture-shrink`

```text
 M packages/engine/src/runtime/desk.rs
 M packages/engine/src/runtime/drive.rs
 M packages/engine/src/runtime/group.rs
 M packages/engine/src/runtime/mod.rs
 M packages/engine/src/runtime/protocol.rs
 M packages/engine/src/runtime/world.rs
 M packages/engine/src/session.rs
 M packages/engine/src/session/persistence/saved_runtime_tests.rs
 M packages/std-thread-proto/src/lib.rs
 M packages/std-thread-proto/src/oworker.rs
 M packages/std-thread-proto/src/worker.rs
 M packages/std-thread-proto/src/xworker.rs
?? packages/engine/src/runtime/day_end_public_views_tests.rs
?? packages/engine/src/runtime/group_roster_consumer_tests.rs
?? packages/engine/src/runtime/pending_cash_tests.rs
?? packages/engine/src/runtime/treasury_autonomy_tests.rs
?? packages/std-thread-proto/src/stock_identity.rs
```

## `./.worktree/p1-group-consumer-2026-10-29`

```text
?? agents/branch-completion/p1-group-consumer-2026-10-29/
```

## `./.worktree/package-static-web-2026-10-30`

```text
?? agents/branch-completion/package-static-web-2026-10-30/
```

## `./.worktree/protocol-save-seq36`

```text
 M packages/engine/src/session/company_source_runtime_tests.rs
 M packages/engine/src/session/protocol/civil/day_end_npc_tests.rs
 M packages/engine/src/session/protocol/civil/session.rs
 M packages/engine/src/session/protocol/civil/single_owner_tests.rs
?? agents/branch-completion/protocol-save-seq36/
```

## `./.worktree/public-library-restore-perf`

```text
 M packages/engine/src/information/public_view.rs
 M packages/engine/src/runtime/desk.rs
 M packages/engine/src/runtime/drive.rs
 M packages/engine/src/runtime/group.rs
 M packages/engine/src/runtime/mod.rs
 M packages/engine/src/runtime/protocol.rs
 M packages/engine/src/runtime/world.rs
 M packages/engine/src/session.rs
 M packages/engine/src/session/persistence/saved_runtime_tests.rs
 M packages/std-thread-proto/src/lib.rs
 M packages/std-thread-proto/src/oworker.rs
 M packages/std-thread-proto/src/worker.rs
 M packages/std-thread-proto/src/xworker.rs
?? packages/engine/src/runtime/day_end_public_views_tests.rs
?? packages/engine/src/runtime/group_roster_consumer_tests.rs
?? packages/engine/src/runtime/pending_cash_tests.rs
?? packages/engine/src/runtime/treasury_autonomy_tests.rs
?? packages/std-thread-proto/src/stock_identity.rs
```

## `./.worktree/r4-payment-world-leaf-2026-10-29`

```text
 M packages/engine/src/runtime/drive/administrative_mirror.rs
 M packages/engine/src/runtime/world/admin_transaction/stages.rs
```

## `./.worktree/r4-tax-production-2026-10-29`

```text
?? agents/branch-completion/r4-tax-production-2026-10-29/
```

## `./.worktree/regression-calendar-fixtures-2026-10-30`

```text
 M packages/engine/src/session/pipeline/decision_snapshot_capture_tests.rs
 M packages/engine/src/session/pipeline/t3_boundary_tests.rs
?? agents/branch-completion/regression-calendar-fixtures-2026-10-30/
```

## `./.worktree/regression-decision-fixtures-2026-10-30`

```text
 M packages/engine/src/session/decision_chain.rs
?? agents/branch-completion/regression-decision-fixtures-2026-10-30/
```

## `./.worktree/regression-diagnostics-fixtures-2026-10-30`

```text
 M packages/engine/src/session/pipeline/authoritative_tick_tests.rs
?? agents/branch-completion/regression-diagnostics-fixtures-2026-10-30/
```

## `./.worktree/regression-efficiency-20261101`

```text
?? apps/desktop/src-tauri/gen
?? node_modules
```

## `./.worktree/regression-error-boundaries-2026-10-30`

```text
 M packages/engine/src/session/pipeline/continuous_matching_adapter_tests.rs
 M packages/engine/src/session/pipeline/continuous_tick_finalizer_tests.rs
 M packages/engine/src/session/pipeline/read_only_start.rs
?? agents/branch-completion/regression-error-boundaries-2026-10-30/
```

## `./.worktree/regression-group-fixtures-2026-10-30`

```text
 M packages/engine/src/runtime/group.rs
?? agents/branch-completion/regression-group-fixtures-2026-10-30/
```

## `./.worktree/regression-publication-contract-2026-10-30`

```text
 M packages/engine/src/session/pipeline/spine_v0_contract_tests.rs
?? agents/branch-completion/regression-publication-contract-2026-10-30/
```

## `./.worktree/reviewed-fixes-integration`

```text
 M packages/engine/src/runtime/drive.rs
 M packages/engine/src/runtime/group.rs
 M packages/engine/src/runtime/mod.rs
 M packages/engine/src/runtime/world.rs
?? packages/engine/src/runtime/group_roster_offset_tests.rs
?? packages/engine/src/runtime/treasury_autonomy_tests.rs
```

## `./.worktree/rich-desk-reject-2026-10-30`

```text
 M docs/trading-rules.md
 M packages/engine/src/runtime/coordinator.rs
 M packages/engine/src/runtime/desk.rs
 M packages/engine/src/runtime/diagnostics.rs
 M packages/engine/src/session/causal.rs
 M packages/engine/src/session/runtime_diagnostics_tests.rs
?? agents/branch-completion/rich-desk-reject-2026-10-30/
```

## `./.worktree/roster-consumer-evidence`

```text
 M apps/server/tests/api_contract.rs
 M packages/engine/src/runtime/desk.rs
 M packages/engine/src/runtime/drive.rs
 M packages/engine/src/runtime/group.rs
 M packages/engine/src/runtime/mod.rs
 M packages/engine/src/runtime/protocol.rs
 M packages/engine/src/runtime/world.rs
 M packages/std-thread-proto/src/lib.rs
 M packages/std-thread-proto/src/oworker.rs
 M packages/std-thread-proto/src/worker.rs
 M packages/std-thread-proto/src/xworker.rs
?? packages/engine/src/runtime/day_end_public_views_tests.rs
?? packages/engine/src/runtime/group_roster_consumer_tests.rs
?? packages/engine/src/runtime/pending_cash_tests.rs
?? packages/engine/src/runtime/treasury_autonomy_tests.rs
?? packages/std-thread-proto/src/stock_identity.rs
```

## `./.worktree/runtime-diagnostics-d0b-storage`

```text
?? agents/branch-completion/runtime-diagnostics-d0b/
```

## `./.worktree/runtime-diagnostics-d2-continuous`

```text
 M docs/diagnostics.md
 M packages/engine/src/runtime/coordinator.rs
 M packages/engine/src/runtime/desk.rs
 M packages/engine/src/runtime/drive.rs
 M packages/engine/src/runtime/drive_tests.rs
 M packages/engine/src/runtime/mod.rs
 M packages/engine/src/runtime/protocol.rs
 M packages/engine/src/runtime/world.rs
 M packages/engine/src/session.rs
 M packages/engine/src/session/failure.rs
 M packages/engine/src/session/retail_lifecycle_tests.rs
 M packages/engine/src/session/runtime_diagnostics_tests.rs
?? agents/branch-completion/runtime-diagnostics-d0b/
?? packages/engine/src/runtime/retail_lifecycle.rs
?? packages/engine/src/session/retail_lifecycle.rs
```

## `./.worktree/runtime-diagnostics-d2-retained-test`

```text
 M packages/engine/src/session/failure.rs
?? agents/branch-completion/runtime-diagnostics-d0b/
?? packages/engine/src/session/runtime_diagnostics_tests/
```

## `./.worktree/runtime-diagnostics-d2b-fixtures`

```text
 M packages/engine/src/session.rs
?? agents/branch-completion/runtime-diagnostics-d0b/
?? packages/engine/src/session/retail_lifecycle_tests.rs
```

## `./.worktree/runtime-diagnostics-d3-trace-projection`

```text
 M packages/engine/src/session/decision_chain.rs
?? agents/branch-completion/runtime-diagnostics-d0b/
?? packages/engine/src/session/decision_chain/trace_projection.rs
```

## `./.worktree/runtime-personal-full-validation`

```text
?? agents/perf-intraday-independent/implementation/
?? packages/engine/src/runtime/personal_full_validation.rs
?? packages/engine/tests/personal_full_validation.rs
```

## `./.worktree/runtime-plan-owner-cache-2026-10-30`

```text
 M packages/engine/src/session/player_company_repurchase_tests.rs
?? agents/performance-delivery/owner-cache-2026-10-30/
```

## `./.worktree/runtime-sync-e1`

```text
?? agents/perf-intraday-independent/implementation/
```

## `./.worktree/runtime-sync-hosts-2026-10-29`

```text
?? agents/perf-intraday-independent/resume-2026-10-29/
```

## `./.worktree/runtime-sync-hosts-b-2026-10-29`

```text
 M apps/desktop/src-tauri/src/actor/correction_recovery_tests.rs
 M apps/server/src/actor/correction_recovery_tests.rs
 M apps/server/src/routes.rs
 M apps/server/tests/ws.rs
 M apps/web-wasm/src/protocol_tests.rs
 M apps/web/src/host/protocol-failure.ts
 M apps/web/src/host/remote-company-write.test.ts
 M apps/web/src/host/remote-host.ts
 M apps/web/src/host/tauri-company-write.test.ts
 M apps/web/src/host/tauri-host.ts
?? agents/perf-intraday-independent/resume-2026-10-29/
```

## `./.worktree/runtime-sync-hosts-b-impl-2026-10-30`

```text
?? agents/perf-intraday-independent/resume-2026-10-29/
```

## `./.worktree/runtime-sync-hosts-b-web-f2-2026-10-29`

```text
?? agents/perf-intraday-independent/resume-2026-10-29/
```

## `./.worktree/runtime-sync-performance-2026-10-29`

```text
?? agents/performance-delivery/c542-preflight.mjs
?? agents/performance-delivery/implementation-review-2026-10-29.md
?? agents/performance-delivery/memory-growth.md
?? agents/performance-delivery/runtime-sync-memory-evidence/
?? agents/performance-delivery/runtime-sync-policy-evidence/
?? agents/performance-delivery/runtime-sync-policy.md
```

## `./.worktree/runtime-sync-policy-2026-10-29`

```text
?? agents/perf-intraday-independent/resume-2026-10-29/
```

## `./.worktree/runtime-sync-progress-cost-2026-10-30`

```text
?? agents/performance-delivery/progress-cost-2026-10-30/
```

## `./.worktree/save-correction-root-2026-10-29`

```text
 M apps/web/src/host/report-corrections.ts
 M apps/web/src/save/current-save-fixture.ts
 M apps/web/src/save/representative-save-test-fixture.ts
 M apps/web/src/save/schema/company/closing.test.ts
 M apps/web/src/save/schema/root.ts
?? agents/branch-completion/save-correction-root-2026-10-29/
?? node_modules
```

## `./.worktree/std-thread-proto-fix`

```text
 M packages/std-thread-proto/src/lib.rs
 M packages/std-thread-proto/src/oworker.rs
 M packages/std-thread-proto/src/worker.rs
 M packages/std-thread-proto/src/xworker.rs
?? packages/std-thread-proto/src/stock_identity.rs
```

## `./.worktree/step-skeleton-gold-884bd5ad-2026-10-30`

```text
?? packages/engine/examples/step_skeleton_projection_probe.rs
```

## `./.worktree/step-skeleton-gold-current-2026-10-30`

```text
?? packages/engine/examples/step_skeleton_projection_probe.rs
```

## `./.worktree/supervisor-suite-split`

```text
 M scripts/run-full-regression.mjs
 M scripts/run-with-deadline.mjs
 M scripts/run-with-deadline.test.mjs
?? scripts/full-regression-artifact-hash-worker.mjs
?? scripts/full-regression-artifact-hash.mjs
?? scripts/rust-test-case-supervisor-execution.test.mjs
?? scripts/rust-test-case-supervisor.mjs
?? scripts/rust-test-case-supervisor.test.mjs
```

## `./.worktree/tax-entitlement-k1`

```text
?? agents/branch-completion/tax-admin-projection/
```

## `./.worktree/tax-stage-group-leaf-2026-10-29`

```text
?? packages/engine/src/runtime/group/admin_transaction/stages.rs
```

## `./.worktree/tax-stage-raw-leaf-2026-10-29`

```text
?? packages/engine/src/runtime/world/admin_transaction/stage_information_tests.rs
?? packages/engine/src/session/ordinary_tax_day_end/
```

## `./.worktree/trading-timeline-day-guard-2026-10-30`

```text
 M apps/web/src/components/trading-timeline.test.ts
 M apps/web/src/components/trading-timeline.ts
?? agents/branch-completion/trading-timeline-day-guard-2026-10-30/
```

## `./.worktree/treasury-autonomy`

```text
 M packages/engine/src/runtime/drive.rs
 M packages/engine/src/runtime/mod.rs
?? packages/engine/src/runtime/treasury_autonomy_tests.rs
```

## `./.worktree/verify-metrics-regression`

```text
 M apps/server/tests/api_contract.rs
 M packages/engine/src/runtime/desk.rs
 M packages/engine/src/runtime/drive.rs
 M packages/engine/src/runtime/group.rs
 M packages/engine/src/runtime/mod.rs
 M packages/engine/src/runtime/protocol.rs
 M packages/engine/src/runtime/world.rs
 M packages/engine/src/session.rs
 M packages/std-thread-proto/src/lib.rs
 M packages/std-thread-proto/src/oworker.rs
 M packages/std-thread-proto/src/worker.rs
 M packages/std-thread-proto/src/xworker.rs
?? packages/engine/src/runtime/day_end_public_views_tests.rs
?? packages/engine/src/runtime/pending_cash_tests.rs
?? packages/engine/src/runtime/treasury_autonomy_tests.rs
?? packages/std-thread-proto/src/stock_identity.rs
```

## `./.worktree/w0l-gate`

```text
?? agents/branch-completion/wasm-license-gate/build.rs
?? agents/branch-completion/wasm-license-gate/license.rs
```

## `./.worktree/wasm-app-save-capture-2026-10-30`

```text
 M apps/web/src/App.tsx
 M apps/web/src/host/worker-host.test.ts
 M apps/web/src/host/worker-host.ts
 M apps/web/src/host/worker-request-scope.test.ts
 M apps/web/src/host/worker-request.ts
?? agents/branch-completion/wasm-app-save-capture-2026-10-30/
?? apps/web/src/save/civil-day-end-capture.test.ts
?? apps/web/src/save/civil-day-end-capture.ts
```

## `./.worktree/wasm-deps-deadline`

```text
 M scripts/wasm-build-dependencies.test.mjs
?? scripts/native-build-dependencies.test.mjs
?? scripts/server-build-dependencies.test.mjs
```

## `./.worktree/wasm-document-handover-leaf-2026-10-29`

```text
 M apps/web/src/host/wasm-memory-schema.ts
 M apps/web/src/host/wasm-memory-storage.ts
?? apps/web/src/host/wasm-memory-handover.test.ts
?? apps/web/src/host/wasm-memory-handover.ts
?? apps/web/src/host/wasm-memory-reload-freeze.test.ts
?? apps/web/src/host/wasm-memory-snapshot-handover.test.ts
?? apps/web/src/host/wasm-memory-storage-handover.test.ts
```

## `./.worktree/wasm-document-owner-2026-10-29`

```text
?? apps/web/src/host/wasm-runtime-options.ts
?? node_modules
```

## `./.worktree/wasm-handover-selection-ui-2026-10-30`

```text
?? node_modules
```

## `./.worktree/wasm-initial-x-consumer-2026-10-29`

```text
 M apps/web/src/app/session-host-lifecycle.test.ts
 M apps/web/src/app/useSessionHostLifecycle.ts
 M apps/web/src/host/engine-supervisor.test.ts
 M apps/web/src/host/engine-supervisor.ts
 M apps/web/src/host/facade-engine-handshake.ts
 M apps/web/src/host/facade-engine-initialize.test.ts
 M apps/web/src/host/facade-engine-initialize.ts
 M apps/web/src/host/facade-engine-protocol.ts
 M apps/web/src/host/facade-initial-archive.test.ts
 M apps/web/src/host/facade-supervisor-control.ts
 M apps/web/src/host/wasm-ingress-worker.ts
 M apps/web/src/host/wasm-session-slot.ts
 M apps/web/src/host/wasm-worker-facade-init.test.ts
 M apps/web/src/host/wasm-worker-ingress-bridge.test.ts
 M apps/web/src/host/wasm-worker-ownership.test.ts
 M apps/web/src/host/wasm-worker.ts
 M apps/web/src/host/worker-host-w1-bootstrap.test.ts
 M apps/web/src/host/worker-host-w1.test-support.ts
 M apps/web/src/host/worker-host.ts
 M apps/web/src/host/worker-request.ts
?? apps/web/src/app/session-host-lifecycle-worker-release.test.ts
?? apps/web/src/host/engine-supervisor-shutdown.test.ts
?? apps/web/src/host/facade-runtime-initialize.test.ts
?? apps/web/src/host/facade-shutdown-initialize.test.ts
?? apps/web/src/host/wasm-allocation-observation.test.ts
?? apps/web/src/host/wasm-document-memory-owner.ts
?? apps/web/src/host/wasm-ingress-worker-shutdown.test.ts
?? apps/web/src/host/wasm-lifecycle-observation.ts
?? apps/web/src/host/wasm-restore-capability-source.test.ts
?? apps/web/src/host/wasm-restore-capability-source.ts
?? apps/web/src/host/wasm-restore-package-capability.ts
?? apps/web/src/host/wasm-runtime-options.ts
?? apps/web/src/host/wasm-session-initial-restore.test.ts
?? apps/web/src/host/wasm-session-shutdown.test.ts
?? apps/web/src/host/wasm-shutdown-report.test.ts
?? apps/web/src/host/wasm-shutdown-report.ts
?? apps/web/src/host/wasm-wait-abort.test.ts
?? apps/web/src/host/wasm-wait-abort.ts
?? apps/web/src/host/wasm-worker-restore-preflight.test.ts
?? apps/web/src/host/worker-host-initial-x-consumer.test.ts
?? apps/web/src/host/worker-host-shutdown.test.ts
?? node_modules
```

## `./.worktree/wasm-js-shutdown-2026-10-29`

```text
?? apps/web/src/host/wasm-document-memory-owner.ts
?? node_modules
```

## `./.worktree/wasm-memory-capacity-2026-10-29`

```text
 M apps/web/src/host/facade-wasm-module.test.ts
 M apps/web/src/host/facade-wasm-module.ts
?? apps/web/src/host/wasm-memory-capacity.test.ts
?? apps/web/src/host/wasm-memory-capacity.ts
```

## `./.worktree/wasm-p0`

```text
?? apps/web/dist-e2e/
```

## `./.worktree/wasm-product-default-coarse500-2026-10-30`

```text
?? agents/branch-completion/wasm-w2/resume-2026-10-29/production-browser/product-default-wait-fix/independent-review/
?? agents/branch-completion/wasm-w2/resume-2026-10-29/production-browser/product-default-wait-fix/product-source-only.patch
```

## `./.worktree/wasm-spawn-ui-timing-diagnostic-2026-10-30`

```text
?? node_modules
```

## `./.worktree/wasm-step-fatal-diagnostic-2026-10-30`

```text
?? agents/branch-completion/wasm-w2/
```

## `./.worktree/wasm-twenty-session-budget-2026-10-30`

```text
?? agents/branch-completion/wasm-w3a/
?? node_modules
```

## `./.worktree/wasm-w1-build-gate`

```text
?? agents/branch-completion/wasm-w1/default-topology/legal-two-groups-fixture-r2/build-output/monitor.lock
?? agents/branch-completion/wasm-w1/default-topology/native-green-2026-10-27-01/build-output/
```

## `./.worktree/wasm-w1-host-tdd`

```text
?? agents/branch-completion/wasm-w1/host-tdd/4dc12507-complete.diff
?? agents/branch-completion/wasm-w1/host-tdd/f7207afc-complete.diff
```

## `./.worktree/wasm-w3-storage-2026-10-28`

```text
 M apps/web/src/host/wasm-session-slot.ts
 M apps/web/src/host/wasm-worker-facade-init.test.ts
 M apps/web/src/host/wasm-worker.ts
 M apps/web/src/host/worker-host.ts
?? apps/web/src/host/wasm-initial-archive-slot.test.ts
?? apps/web/src/host/wasm-restore-topology-gate.test.ts
?? apps/web/src/host/wasm-worker-initial-archive.test.ts
?? apps/web/src/host/worker-host-initial-archive.test.ts
```

## `./.worktree/web-origin-historical-2026-10-29`

```text
 M apps/web/src/save/schema/company/state.test.ts
```

## `./.worktree/web-origin-regression-2026-10-29`

```text
?? agents/perf-intraday-independent/resume-2026-10-29/
```

## `./.worktree/web-origin-remote-2026-10-29`

```text
 M apps/web/src/host/company-read-hosts.test.ts
 M apps/web/src/host/remote-company-reports.test.ts
 M apps/web/src/host/remote-company-write.test.ts
 M apps/web/src/host/remote-host.test.ts
 M apps/web/src/host/remote-lifecycle.test.ts
 M apps/web/src/host/remote-publisher-state.test.ts
 M apps/web/src/host/remote-startup.test.ts
 M apps/web/src/host/remote-state-contract.test.ts
 M apps/web/src/host/tauri-startup-contract.test.ts
?? agents/perf-intraday-independent/resume-2026-10-29/
```

## `./.worktree/web-origin-tauri-2026-10-29`

```text
 M apps/web/src/host/tauri-company-write.test.ts
 M apps/web/src/host/tauri-host.test.ts
 M apps/web/src/host/tauri-timeline-state.test.ts
?? agents/perf-intraday-independent/resume-2026-10-29/
```

## `./.worktree/web-origin-worker-2026-10-29`

```text
 M apps/web/src/host/worker-host-company-write.test.ts
 M apps/web/src/host/worker-host-legacy.test-support.ts
 M apps/web/src/host/worker-host.test.ts
?? agents/perf-intraday-independent/resume-2026-10-29/
```

## `./.worktree/worker-error-fidelity`

```text
 M apps/web/src/host/wasm-worker-failure.test.ts
 M apps/web/src/host/wasm-worker.ts
?? agents/web-worker-error-fidelity/
?? node_modules
```

## `./.worktree/worker-ownership-fix`

```text
 M apps/web/src/host/wasm-worker-ownership.test.ts
 M scripts/run-with-deadline.mjs
?? node_modules
```

## `./.worktree/ws-fixture-fix`

```text
 M apps/desktop/src-tauri/src/lib_tests.rs
 M apps/server/tests/api_contract.rs
 M apps/server/tests/ws.rs
 M packages/engine/src/information/public_view.rs
 M packages/engine/src/runtime/desk.rs
 M packages/engine/src/runtime/drive.rs
 M packages/engine/src/runtime/group.rs
 M packages/engine/src/runtime/mod.rs
 M packages/engine/src/runtime/protocol.rs
 M packages/engine/src/runtime/world.rs
 M packages/engine/src/session.rs
 M packages/engine/src/session/persistence/saved_runtime_tests.rs
 M packages/std-thread-proto/src/lib.rs
 M packages/std-thread-proto/src/oworker.rs
 M packages/std-thread-proto/src/worker.rs
 M packages/std-thread-proto/src/xworker.rs
 M scripts/run-full-regression.mjs
 M scripts/run-full-regression.test.mjs
 M scripts/run-with-deadline.mjs
 M scripts/run-with-deadline.test.mjs
 M scripts/verify/asm-equiv-cmp-regression.py
 M scripts/verify/asm-equiv-cmp.py
?? packages/engine/src/runtime/day_end_public_views_tests.rs
?? packages/engine/src/runtime/group_roster_consumer_tests.rs
?? packages/engine/src/runtime/pending_cash_tests.rs
?? packages/engine/src/runtime/treasury_autonomy_tests.rs
?? packages/std-thread-proto/src/stock_identity.rs
?? scripts/full-regression-artifact-hash-worker.mjs
?? scripts/full-regression-artifact-hash.mjs
?? scripts/full-regression-artifact-hash.test.mjs
?? scripts/rust-test-case-supervisor-lifecycle.test.mjs
?? scripts/rust-test-case-supervisor-proc.test.mjs
?? scripts/rust-test-case-supervisor.mjs
?? scripts/rust-test-case-supervisor.test.mjs
```

## `./.worktree/year-close-diagnosis`

```text
 M apps/desktop/src-tauri/src/lib_tests.rs
 M apps/server/tests/api_contract.rs
 M apps/server/tests/ws.rs
 M packages/engine/src/information/public_view.rs
 M packages/engine/src/runtime/desk.rs
 M packages/engine/src/runtime/drive.rs
 M packages/engine/src/runtime/group.rs
 M packages/engine/src/runtime/mod.rs
 M packages/engine/src/runtime/protocol.rs
 M packages/engine/src/runtime/world.rs
 M packages/engine/src/session.rs
 M packages/engine/src/session/persistence/saved_runtime_tests.rs
 M packages/engine/tests/company_scenarios/lifecycle.rs
 M scripts/run-full-regression.mjs
 M scripts/run-full-regression.test.mjs
 M scripts/run-with-deadline.mjs
 M scripts/run-with-deadline.test.mjs
 M scripts/verify/asm-equiv-cmp-regression.py
 M scripts/verify/asm-equiv-cmp.py
?? agents/year-close-authority/
?? packages/engine/src/runtime/day_end_public_views_tests.rs
?? packages/engine/src/runtime/group_roster_consumer_tests.rs
?? packages/engine/src/runtime/pending_cash_tests.rs
?? packages/engine/src/runtime/treasury_autonomy_tests.rs
?? scripts/full-regression-artifact-hash-worker.mjs
?? scripts/full-regression-artifact-hash.mjs
?? scripts/full-regression-artifact-hash.test.mjs
?? scripts/rust-test-case-supervisor-lifecycle.test.mjs
?? scripts/rust-test-case-supervisor-proc.test.mjs
?? scripts/rust-test-case-supervisor.mjs
?? scripts/rust-test-case-supervisor.test.mjs
```
