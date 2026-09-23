# Task 10 diagnostic divergence audit

Audit date: 2026-09-22 (Asia/Shanghai).

No numeric diagnostic expectation or assertion was changed in
`packages/engine/src/diagnostics.rs` or `packages/engine/tests` for Task 10. The structured
expectation-delta list is therefore empty: there is no old value, new value, or divergence number
to record.

The only diagnostics-example compatibility edit is in
`packages/engine/examples/causal_diagnostics.rs`: `session.step()` became `session.step()?` after
the engine API began returning `Result`. It propagates an explicit `StepFatal`; it does not change
an expected value or silently discard a failure.

## Boundary limitation

The plan asks for Task 9 and Task 10 completion commit SHAs and a bounded diff between them. This
integration worktree has intentionally not been committed, so those two commit boundaries do not
exist and are not fabricated here. Consequently, the commit-boundary sub-gate remains open even
though the current diagnostics tests and strict clippy validation pass. The final integration
owner must record real boundary SHAs after authorized commits exist, or explicitly retain this
limitation in the final status.

The regression added during validation concerns seeded initial holdings with no game-generated buy
identity. It prevents a losing or profitable partial sell from mutating buy-failure experience and
prevents an identity-less failure event. This restores the existing A-share game model's internal
identity invariant; it does not alter exchange rules, nominal fees, T+1, price limits, or auction
semantics.
