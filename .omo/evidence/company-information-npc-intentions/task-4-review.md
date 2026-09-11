# Task 4 — Independent Review (E/task-4-review.md)

- Reviewer: independent subagent (ses_f74750dbbffeB9kCtwYojOif53), did NOT implement
- Subject: commit `bf00279` "feat(engine): 增加公历及可冻结交易日历" (14 files, +2596)
- Date: 2026-09-10
- Reviewer test run: cargo test -p engine --test calendar → 10 passed / 0 failed. Orchestrator re-ran: same, exit 0.

## Key verification performed BY THE REVIEWER (independent recomputation, not trust)
- Leap/weekday/day-number arithmetic: days_from_civil verified line-by-line vs Hinnant reference; 8 weekday anchors independently computed (2000-01-01 Sat, 2030-01-01 Tue, 2032-02-29 Sun, 2100-01-01 Fri, 1998-01-01 Thu, 2024-02-10 Sat, 2025-01-29 Wed, 2030-02-03 Sun) — all match.
- 504-day prehistory (1998: 261 weekdays − 9 holidays = 252; 1999: 261 − 9 = 252) independently recomputed — matches test assertions exactly; 360 ≪ 504.
- Live HKO fetch: T2030e.txt row matches embedded table on all 4 lunar/solar dates + weekday; provenance constants present (URL/retrieval/summary). 9+ lunar anchors correct.
- Frozen policy: restore-wins proven against a semantically divergent v2 (not mere round-trip); tamper tests at all 3 digest levels.
- Fallback ruleset verbatim per K1; 调休 weekends never trading (tested vs real 2024-02-04 / 2025-01-26 makeup Sundays).
- Runtime gates: <2000 / >2099 typed errors; prehistory floor 1998-01-01 reachable; four-way year labels all exercised; zero Official years without notice text (2026 = NoticeTextUnverified).
- Scope: 14 files only calendar+lib.rs(+6)+tests; no new deps; no runtime network; all files ≤250 pure logic lines; zero unwrap/expect in src.

## Non-blocking notes
1. date.rs:328 doc typo "1970-01-00" → 1970-01-01 (doc-only).
2. 2051 defect prose inconsistency between module header (English file) and LUNAR_WEEKDAY_DEFECT_NOTE (both en/tc) — zero functional impact (weekday column never consumed).
3. holidays.rs:97 `.ok()?` theoretical silent-None for gap years — unreachable by construction (check_facts_coverage at every policy funnel).

VERDICT: APPROVE
