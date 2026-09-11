# Task 1 — Independent Review (E/task-1-review.md)

- Reviewer: independent subagent (ses_f7533a398ffesFOuxCHsOYerle), did NOT implement
- Subject: commit `0a77d8f` "test(engine): 固定公司模型变更前量价基线" (diff vs parent `6ad461e`), add-only, 3 files, 1431 insertions
- Date: 2026-09-10

## Gate 1 — A-share semantics: PASS
Matrix copy verified field-by-field against `apps/web/src/config/defaults.ts` (stocks/codes/exchanges/prices/totals/floats; ChiNext ±20%, main & ST main board ±10% per documented 2026-07-06 rule at config.rs:147; commission 0.00025/min ¥5/stamp 0.0005/lot 100/T+1/tick 1¢; clock 900+14400+180=15300; v_params/strategy_params/float_allocation/history_len/fundamental_value_means all match). Compressed copy faithful to `session.rs:774–819` including prefix-based exchange/category mapping, overridden 100M/80M shares, and direct `GameConfig::proposed_defaults()` call. Report logic delegated to pre-existing `run_price_volume_baseline` — no engine logic duplicated.

## Gate 2 — Necessity & minimal scope: PASS
Exactly the 3 files required by plan task 1. No product behavior change. No save access. No legacy-format support. Rust pinning tests are load-bearing (drift in either true source fails them). Runner's SCENARIOS table is the reconciliation expectation — drift fails every capture (baseline-run.mjs:159–175).

## Gate 3 — Tests, drift, complexity: PASS
22 node tests (real assertions; negative tests mutate one field and assert field-named throw; e2e injects exec at the subprocess boundary; nonzero-exit test asserts no success manifest survives). All four plan-required failure modes covered (duplicate seed :168, nonzero exit :364, missing report :395, config mismatch :404) plus determinism-break (:419) and dirty-dir (:436). 5 Rust tests hard-pin every field. LOC justified under the exactly-3-files constraint; no dead code.

## Gate 4 — Honesty: PASS
Every failure path throws with scenario/seed/field/exit-code context. `engine_error_events` (matrix 55–98/seed) validated, logged per-seed "不静默", stored in manifest (disk-verified: 98,91,90,62,70,89,88,55,86,60), dedicated test asserting nonzero values must appear in logs+manifest. No --dry-run. pnpm unavailability recorded available:false. Orchestrator independently verified sha256 manifest↔disk MATCH both scenarios; node tests 22/22 exit 0; cargo test -p engine exit 0.

## Non-blocking observations (nits, no action required this task)
1. baseline-run.mjs:58 — malformed BASELINE_FIXTURE_TIMEOUT_MS silently falls back to default (cannot corrupt evidence; note for future touch).
2. test:146 — dead corepack branch in fakeExec stub.
3. Runner truncates stderr to 2000 chars in failure messages (failure still propagates).
4. Invalid-JSON stdout path (:363–368) lacks a direct unit test (adjacent empty-stdout path tested).

Pre-existing (NOT introduced): clippy `unnecessary_filter_map` at src/behavior.rs:346 already red at base revision 6ad461e — add-only diff touching no src/ file cannot have introduced it.

VERDICT: APPROVE
