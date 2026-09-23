# Decisions — resolve-blockers-wayland

Architectural choices and rationales discovered during work on this plan.

_Auto-scaffolded by /start-work. Append new entries below - never overwrite._

---

## 2026-09-16 Task 7 cross-year parent-slot decision

- Keep `IncompatibleExecutionState` strict. Do not weaken its guard: a live child or parent owned by another plan remains an error. The demonstrated exception is not compatibility relaxation: when terminating a plan with no active child, remove its still-linked parent record before applying the terminal plan revision.
- No A-share trading rule is implicated. `docs/trading-rules.md` remains unchanged because the correction concerns internal plan-parent lifecycle ownership, while ordinary exchange order/day-end behavior is preserved.

## 2026-09-16 Task 7 independent review approval

- Independent non-implementer review task `bg_10a8c989` APPROVED the cross-year lifecycle diff. It confirmed the strict incompatible-state guard remains intact, the unlink only removes a parent owned by the terminating plan, and no A-share order, T+1, lot, auction, trading-calendar, accounting, or disclosure semantic changes. The reviewer found no release-blocking missing boundary test or cross-layer drift.
