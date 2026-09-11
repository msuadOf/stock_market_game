# Task 21 — Independent Review (E/task-21-review.md)

- Reviewer: the orchestrator (Atlas) — did NOT implement; full code-read of all four module files
- Subject: commits `6cdc219` "feat(engine): 增加可追溯的持续个人交易计划" (+2112, 6 files) + `61eb698` "fix(engine): 修复计划到期事件的不可达路径" (+165/−14)
- Date: 2026-09-10

## Review findings (first pass)
Code-read of plans/{mod,state,revision,validation}.rs found the design sound: typed errors throughout (PlanError with full context), accept≠fill (ChildOrderAccepted never advances filled_qty), real-fill-only progress, reverse-direction hysteresis threshold (K5a 2000bp), below-filled revision requires termination rationale, day-end ends child-order lifecycle only, time monotonicity, save-serializes-minimal-state with index rebuild + inconsistency rejection.

**Defect found (fix-level)**: `ensure_event_allowed` rejected ALL events beyond last_valid_trading_day, but `expire()` required trading_day > last_valid — the Expired event's success path was unreachable (all inputs error), and a missed/late TradingDayEnded stranded the plan Active forever with no legal terminating event. Confirmed against tests: only the before-horizon rejection and day-end paths were covered; the dead path was untested because it was untestable.

## Fix verification (second pass, commit 61eb698)
- Surgical: `allow_beyond_horizon: bool` parameter; only expire/end_of_trading_day pass true; terminal-state and time-backwards guards unchanged for all events; zero API/field changes.
- TDD: 5 new tests written first, red run captured (exit 101, 3 reachability failures as expected), then green.
- Orchestrator re-ran: `cargo test -p engine --test calendar --test plans` → 10/10 + 33/33, exit 0. Worker's full suite: 485 passed / 0 failed / 4 ignored.

## Verdict rationale
State machine now total: every public event variant has a reachable success path or an intentional typed rejection; the K6 acceptance criteria (no reset on calm observation, explicit reasons for pause/reverse/expiry, day-end ≠ Completed, cross-day survival) are all covered by real tests.

## Non-blocking observations
- ts_rs exports regenerate Plan*.ts into apps/web/src/types/generated/ on every test run — left untracked for task 29 (single owner of generated dir) to commit via the official pipeline.
- plans/mod.rs is 302 total lines (re-exports + PlanBook container) — within 250 pure-logic convention after doc/derive exclusion; noted for the record.

VERDICT: APPROVE (after fix 61eb698)
