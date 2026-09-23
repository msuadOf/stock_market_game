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

This record does not claim that Task 11 is complete. The final documentation status must continue
to describe K7 and any other still-open gate honestly until their evidence roots verify.
