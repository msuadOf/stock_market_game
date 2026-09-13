# Task 27 Independent Adversarial Final Review

Date: 2026-09-12

VERDICT: APPROVE

## Final Closure

Both prior P1 findings are closed in the final, non-regenerating worktree inspection:

- `git diff -- apps/web/src/types/generated` produced no output; generated-path `git status --short`
  is empty; `apps/web/src/types/generated/RuntimeResource.ts` does not exist.
- The previously run exact scoped formatter check with `--edition 2024 --config
  skip_children=true` exited `0`; this closure did not run rustfmt or alter Rust files.
- `git diff --check` exits `0`.
- All task-27 evidence artifacts are nonempty ASCII/UTF-8 plain text. `task-27-happy.txt` now
  records capacity `5`, save-contract `13`, plan-execution `12`, company-decision `11`,
  extraction-replay `3`, session `107 passed, 4 ignored`, and the independently observed full
  engine result `963 passed, 4 ignored`.

Cleanup sequencing is verified: behavioral test commands completed first; task-29-owned generated
bindings were restored from `HEAD` and `RuntimeResource.ts` removed second; only
non-regenerating diff, status, existence, encoding, and evidence checks ran after cleanup.

## Retained Independent Behavior Evidence

| Command | Result |
| --- | --- |
| `cargo test -p engine --lib pending_plan_event_capacity` | exit 0; 5 passed, 95 filtered |
| `cargo test -p engine --test save_contract` | exit 0; 13 passed |
| `cargo test -p engine --test plan_execution` | exit 0; 12 passed |
| `cargo test -p engine --test company_decision_session` | exit 0; 11 passed |
| `cargo test -p engine --test extraction_replay` | exit 0; 3 passed |
| `cargo test -p engine --test session` | exit 0; 107 passed, 4 ignored |
| `cargo test -p engine` | exit 0; 963 passed, 4 ignored |
| `cargo check -p engine` | exit 0 |
| scoped `rustfmt --edition 2024 --config skip_children=true --check` | exit 0 |

Strict K7 shape and validation remain confirmed: required state is persisted, unknown/missing
schema members reject, restore validates before candidate construction and does not mutate the
live session on rejection, and the real 26-NPC continuation test binary-compares tick events and
day-end saves through three resumed days. No legacy migration/default reconstruction was found.

The typed `Event::ResourceLimit` capacity path remains non-panicking, deterministic, and covered
for acceptance, requote-before-cancel, actual fill, Accepted-plus-Filled boundary, and day-end.
No A-share T+1, lot/tick, price-limit, fee, matching, shareholder-cash, or default-stock-set
semantics changed.

## Residual Non-blocking Risks

- Rust-analyzer timed out during the prior behavior review; `cargo check` passed as the fallback.
- Scoped clippy remains blocked by the pre-existing `packages/engine/src/behavior/decision.rs:239`
  `unnecessary_filter_map`, outside task 27.
- `.omo/boulder.json` is orchestration state and was excluded from product-scope assessment.
