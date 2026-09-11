# Task 22 independent review — 2026-09-11

Reviewer: uninvolved explore subagent `ses_f71aecedeffeUBuHqID7xwx5Qh`

## First pass: REJECT

Valid findings:

1. Duplicate PlanIds could share the complete sort key, making results depend on caller order.
2. Public target-share conversion accepted lot sizes other than the mainland A-share 100-share lot.
3. Half-even ties lacked direct public-surface regression tests.

Corrections:

- Added `AllocationError::DuplicatePlanRequest` and reject duplicate identities before sorting.
- Enforced `config::A_SHARE_BOARD_LOT` in target-share conversion.
- Added positive/negative score ties and cash-reserve cent ties.
- Retained and verified typed `u32` share-quantity overflow.

The suggested intermediate `i128` overflow test was investigated and rejected as unreachable:
all public operands are bounded `i64`/`i32`/`u32` values with fixed 10,000-scale multipliers, far
below `i128::MAX`. The reviewer confirmed this conclusion on re-review.

## Second pass: APPROVE

No remaining findings. The reviewer confirmed A-share semantics, task necessity/minimality,
authoritative cash less reservations and fees, no sale-proceeds pre-spend, T+1 via sellable
inventory, 100-share lot rounding, deterministic unique-plan ordering, unavailable-signal
reweighting, typed overflow, and honest game-policy parameters. Focused suite: 28/28 passed.
