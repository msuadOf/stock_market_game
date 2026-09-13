# Task 28 Independent Review

Date: 2026-09-12

VERDICT: REJECT

Task 28 requires a scenario suite that, collectively, proves the production `GameSession`
accounting -> information -> belief -> plan -> allocation -> urgency -> orderbook ->
fill/restore loop. The production path exists, but its task-28 scenarios do not prove the
critical controlled connections. Four required claims are direct domain-function tests rather
than session wiring. The reported diagnostics parity is only an empty Cargo feature compiling
the identical test target, not a comparison of authoritative event/state bytes.

## Acceptance Coverage Table

| Task-28/ADR clause | Scenario test and call path | Result |
|---|---|---|
| Real GameSession, civil operations/disclosure and closed-day tick-free behavior | `lifecycle::civil_information_chain_keeps_unread_beliefs_stable_and_closed_days_tick_free`, `GameSession::new -> step x60 -> end_civil_day x2`; [lifecycle.rs](/data1/baiyifan/workplace/stock_market_game/packages/engine/tests/company_scenarios/lifecycle.rs:5) | Partial. Real session/closed-day state is checked, but no report acquisition, belief update, plan, allocation, quote, or fill is asserted. |
| Cross-year/report/disclosure state | `lifecycle::year_boundary_keeps_company_operations_and_disclosure_state_authoritative`, `GameSession::new -> step x60 -> end_civil_day`; [lifecycle.rs](/data1/baiyifan/workplace/stock_market_game/packages/engine/tests/company_scenarios/lifecycle.rs:45) | Reject. Only date advance and nonempty pending obligations are asserted. No year-end close, report/publication ID, disclosure output, or accounting output is inspected. |
| Same public message, different legitimate priors | Direct `revise_forecast` with locally constructed `ForecastState` and `GrowthObservation`; [participant.rs](/data1/baiyifan/workplace/stock_market_game/packages/engine/tests/company_scenarios/participant.rs:14) | Reject. Never constructs `GameSession`, reads `PublicLibrary`, acquires through `NpcInformationState`, or observes session `BeliefBook` mutation. |
| Same P&L, different experience | Direct `allocate_soft_budgets` with locally constructed `AllocationFunds`, `AllocationRequest`, and `AllocationExperience`; [participant.rs](/data1/baiyifan/workplace/stock_market_game/packages/engine/tests/company_scenarios/participant.rs:43) | Reject. No authoritative experience history, account P&L, session allocation pass, or downstream order is used. |
| Cheap-but-withdraw through real cancellation routing | Local `PlanBook`, price vector, `assess_urgency`, then `decide_quote`; [participant.rs](/data1/baiyifan/workplace/stock_market_game/packages/engine/tests/company_scenarios/participant.rs:87) | Reject. The asserted `QuoteAction::Cancel` has no session-owned child, no `execute_plan_observation`, no `OrderCanceled`, and no freeze release. |
| Cross-stock pending sale cannot fund buys | Direct `allocate_soft_budgets` with fabricated requests/funds; [constraints.rs](/data1/baiyifan/workplace/stock_market_game/packages/engine/tests/company_scenarios/constraints.rs:35) | Reject. It contains neither a real sale order/freeze nor a session decision-chain allocation pass. |
| Real partial fill, account/candle/freeze reconciliation | `GameSession::execute_plan_observation -> router/orderbook -> synchronize_plan_execution`; [matching.rs](/data1/baiyifan/workplace/stock_market_game/packages/engine/tests/company_scenarios/matching.rs:81) | Partial. Trade, cash/positions, 300-share remainder, 300003-cent buyer reserve, seller release, and candle turnover are genuinely asserted. The plans, allocation grant, and quote are manually constructed, and this scenario is not saved/restored. |
| No counterparty means legal zero fill | `GameSession::execute_plan_observation` with no float, then synchronization; [matching.rs](/data1/baiyifan/workplace/stock_market_game/packages/engine/tests/company_scenarios/matching.rs:217) | Partial. Real book zero-fill/no-candle behavior is covered, but from a manually created plan. |
| T+1 same-day acquired shares | `GameSession::enqueue_player_intent -> step`; [constraints.rs](/data1/baiyifan/workplace/stock_market_game/packages/engine/tests/company_scenarios/constraints.rs:64) | Reject. The test claims acquisition but submits a sell immediately on a fresh session. It never performs a buy fill and starts with zero sellable inventory, so it proves only generic zero-inventory rejection. |
| Malformed fixture | `serde_json::from_value::<ScenarioFixture>` after a scalar mutation; [constraints.rs](/data1/baiyifan/workplace/stock_market_game/packages/engine/tests/company_scenarios/constraints.rs:91) | Pass as typed fixture-boundary coverage, not session-loop evidence. |
| Future observation/unbalanced accounting are rejected atomically | Real save JSON is mutated; future candidate is decoded then `GameSession::restore` fails, unbalanced report fails `decode_save_slot`, original session bytes are compared; [lifecycle.rs](/data1/baiyifan/workplace/stock_market_game/packages/engine/tests/company_scenarios/lifecycle.rs:72) | Pass for the stated negative save candidates. |
| Save/restore continuation | `GameSession::save -> decode_save_slot -> GameSession::restore -> step x60`, canonical event bytes and final save bytes compared; [restore.rs](/data1/baiyifan/workplace/stock_market_game/packages/engine/tests/company_scenarios/restore.rs:4) | Partial. Strong generic restoration evidence, but no asserted plan/fill/partial-fill state occurs in the restored scenario. |
| Diagnostics on/off parity | Default and feature invocation list the same target tests; [task-28-happy.txt](/data1/baiyifan/workplace/stock_market_game/.omo/evidence/company-information-npc-intentions/task-28-happy.txt:20) | Reject. This is test-list parity only, not authoritative event/state byte parity. |

## Rejections

### R1: Critical - four controlled acceptance claims bypass `GameSession`

- Violated clauses: task 28 "完整GameSession" at [plan](/data1/baiyifan/workplace/stock_market_game/.omo/plans/company-information-npc-intentions.md:517), acceptance clauses at [plan](/data1/baiyifan/workplace/stock_market_game/.omo/plans/company-information-npc-intentions.md:521), and ADR0016 controlled prior/experience/withdraw/budget requirements at [ADR0016](/data1/baiyifan/workplace/stock_market_game/docs/decisions/0016-fundamental-factor-model.md:69).
- Proof: `participant.rs` calls `revise_forecast` at lines 29-30, `allocate_soft_budgets` at lines 68-73, and `assess_urgency`/`decide_quote` at lines 139-181; [participant.rs](/data1/baiyifan/workplace/stock_market_game/packages/engine/tests/company_scenarios/participant.rs:29). `constraints.rs` calls `allocate_soft_budgets` at line 49; [constraints.rs](/data1/baiyifan/workplace/stock_market_game/packages/engine/tests/company_scenarios/constraints.rs:49). None creates a `GameSession`.
- Production proof of the missing connection: the actual chain acquires `PublicLibrary` material into `NpcInformationState`, updates `BeliefBook`, drives plans, then executes allocation/urgency/quote/routing at [decision_chain.rs](/data1/baiyifan/workplace/stock_market_game/packages/engine/src/session/decision_chain.rs:314) and [decision_chain.rs](/data1/baiyifan/workplace/stock_market_game/packages/engine/src/session/decision_chain.rs:863).
- Minimal required fix: add controlled, typed test support or a deterministic session fixture that drives each critical fact through the real `GameSession` chain and asserts the authoritative `information_states`, `belief_books`, session `plans`, parent/working order, and routed event results. Do not replace these with direct domain calls.

### R2: High - T+1 evidence is mislabeled

- Violated clause: task-28 negative path requires T+1 sell, not a generic insufficient-inventory rejection; [plan](/data1/baiyifan/workplace/stock_market_game/.omo/plans/company-information-npc-intentions.md:523).
- Proof: the comment says the player "acquires shares today" but the test creates a fresh session then immediately enqueues only `Intent::PlaceLimit { side: Sell }`; [constraints.rs](/data1/baiyifan/workplace/stock_market_game/packages/engine/tests/company_scenarios/constraints.rs:64). It asserts `InsufficientShares` after zero sellable inventory at line 80. No buy/fill event or `t1_locked` state exists in the setup.
- Minimal required fix: acquire the exact shares through a real same-day fill, assert `qty > 0` and `t1_locked > 0`, then submit the sell through the normal session route and assert T+1 rejection/no accepted order.

### R3: High - cross-year scenario does not prove reports or disclosure outputs

- Violated clauses: task 28 cross-year/report/disclosure requirement and K4 natural-day close/disclosure behavior; [plan](/data1/baiyifan/workplace/stock_market_game/.omo/plans/company-information-npc-intentions.md:518) and [plan](/data1/baiyifan/workplace/stock_market_game/.omo/plans/company-information-npc-intentions.md:118).
- Proof: the scenario only asserts civil date `2031-01-01` and that scheduler pending work is nonempty; [lifecycle.rs](/data1/baiyifan/workplace/stock_market_game/packages/engine/tests/company_scenarios/lifecycle.rs:55). It does not inspect report count/ID/kind/period, disclosure cursor, accounting close result, or published report contents.
- Minimal required fix: cross a scheduled report/year boundary with a deterministic company schedule and assert the authoritative close/report/disclosure outputs and the absence of a market tick on any closed civil day used.

### R4: High - partial fill and restore are separate, disconnected scenarios

- Violated clause: complete GameSession partial-fill/restore loop; [plan](/data1/baiyifan/workplace/stock_market_game/.omo/plans/company-information-npc-intentions.md:518).
- Proof: the real partial-fill scenario stops after its in-memory reconciliation at [matching.rs](/data1/baiyifan/workplace/stock_market_game/packages/engine/tests/company_scenarios/matching.rs:148). The restore scenario starts instead from a separate unconstrained fixture session and never asserts a trade, parent plan, child remainder, or reserve; [restore.rs](/data1/baiyifan/workplace/stock_market_game/packages/engine/tests/company_scenarios/restore.rs:4).
- Minimal required fix: save after a real partial fill, restore that exact save, continue the same child/plan lifecycle, and compare canonical events and final authoritative save bytes with an uninterrupted twin.

### R5: High - "diagnostics parity" is not behavior parity, and the Cargo feature is out of task-28 scope

- Violated clauses: K7 requires diagnostic isolation and no economic-behavior change; [plan](/data1/baiyifan/workplace/stock_market_game/.omo/plans/company-information-npc-intentions.md:172). Task 35 owns feature-gated diagnostic targets and explicit `required-features`; [plan](/data1/baiyifan/workplace/stock_market_game/.omo/plans/company-information-npc-intentions.md:583).
- Proof: current feature implementation is only `simulation-diagnostics = []`; [Cargo.toml](/data1/baiyifan/workplace/stock_market_game/packages/engine/Cargo.toml:13). Search finds no feature-gated behavior in engine source. The happy evidence claims parity solely because both commands list 12 names; [task-28-happy.txt](/data1/baiyifan/workplace/stock_market_game/.omo/evidence/company-information-npc-intentions/task-28-happy.txt:20). My reproduction confirms both lists contain the same 12 tests but does not compare any output/state bytes.
- Minimal required fix: remove this task-28 Cargo change unless its owning task is explicitly reassigned. In the proper diagnostics task, compare serialized authoritative event stream and save/state bytes for identical controlled runs, and test the absence/presence boundary rather than test-name parity.

## Evidence Integrity and Scope

- No fabricated `Trade`, fill, candle, or liquidity evidence was found in the two matching tests. The partial-fill test uses the public `GameSession::execute_plan_observation`, asserts a real `Event::Trade`, derives fees from `session.save().setup.config`, and checks cash, positions, active candle, and reserve. This is valid downstream evidence, but it is not decision-chain evidence.
- The future-report and unbalanced-accounting probes mutate real session save candidates. The original session bytes are checked unchanged. The future probe reaches `GameSession::restore`; the unbalanced candidate is rejected earlier by `decode_save_slot`, which is a valid restore-boundary rejection.
- The empty Cargo feature is a three-line modification in the current dirty worktree. The scenario directory/fixture/evidence are untracked. Task-29 Web/generated/query/save paths also coexist dirty. No attribution boundary proves who edited shared `Cargo.toml`; task-29 paths were not treated as task-28-owned solely because they are unrelated shared-tree changes.
- Existing A-share semantics are not changed by the tests. The test's false same-day-acquisition description is nevertheless misleading evidence and must be corrected.

## Reproduced Verification

| Command | Actual result |
|---|---|
| `cargo test -p engine --test company_scenarios -- --nocapture` | Exit 0, 12 passed, 0 failed, 0 ignored. |
| `cargo test -p engine --features simulation-diagnostics --test company_scenarios -- --nocapture` | Exit 0, 12 passed, 0 failed, 0 ignored. This establishes only empty-feature target parity. |
| Both `--list` commands above | Each lists exactly the same 12 test names. |
| Relevant ten-target command from task-28 evidence | Exit 0. Counts: fundamental beliefs 22, experience feedback 25, allocation 28, urgency 27, plan execution 12, decision session 11, save contract 13, civil clock 10, publications 22, replay 3. |
| `cargo test -p engine` | Exit 0. Full engine suite: 963 passed, 0 failed, 4 ignored stress tests; task-28 target ran 12/12. |
| `cargo check -p engine` and `cargo check -p engine --release` | Both exit 0. |
| `cargo clippy -p engine --test company_scenarios -- -D warnings` | Exit 101 on pre-existing `packages/engine/src/behavior/decision.rs:239` `unnecessary_filter_map`; no scenario-file diagnostic was reported first. |
| Scoped `rustfmt --edition 2024 --check --config skip_children=true` on the six scenario Rust files | Exit 0. |
| `git diff --check` | Exit 0. |
| `lsp_diagnostics packages/engine/tests/company_scenarios` | Timed out after 30 seconds. Cargo check is recorded as the static fallback, not an LSP pass. |

## Cleanup Receipt

No product code, test, fixture, plan, boulder state, commit, reset, clean, stash, amend, rebase, checkout, or push was performed by this review. No temporary repository file or process was created. The sole review artifact is this UTF-8 file; the substantive finding below was appended to the existing issues notepad.
