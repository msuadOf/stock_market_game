# Task 12 documentation validation

Validation date: 2026-09-23 (Asia/Shanghai).

- `node --test scripts/check-doc-symbols.test.mjs`: 2 passed, 0 failed.
- `node scripts/check-doc-symbols.mjs docs/trading-rules.md docs/architecture.md`: `PASS`, 14
  explicit `engine::symbol` references checked, `missing: []`.
- `engine::seal_allocation_snapshot` occurs in both `docs/trading-rules.md` and
  `docs/architecture.md` (two occurrences total).
- `README.md` explicitly states that panic is a process-level failure and is not promised as a
  recoverable tick.
- `docs/trading-rules.md` records the 2026-09-23 engineering-wiring review separately from the
  2026-09-22 official-rule content review, and continues to label escrow and divergence #9 fee
  collection as game simplifications rather than exchange clearing rules.
- `AGENTS.md`, `docs/testing.md`, and the current plan record the user-approved timing contract:
  ordinary command/case target <=10s; necessary long child and whole-batch wall <=300000ms; real
  multi-process/multi-core execution; build time reported separately from test execution time.

Post-commit Task 11 evidence now verifies 94/94 executions from two fresh roots at revision
`f22241797385540c8910ee042d1072a6aa50a4c9`, sharing source fingerprint
`5d3c51b2c1149dcaca21533dbbaf7e86d13ff7efb86d7722ddd5af61f1ce668e`. The documentation status
continues to describe Task 9's missing historical witness and Task 10's missing real commit boundary
honestly; it does not turn either limitation into a PASS or publish an unmeasured performance claim.
