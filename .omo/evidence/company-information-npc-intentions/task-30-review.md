# Task 30 Independent Review

Reviewer: independent explore subagent `bg_71ada89e`.

Reviewed scope: WASM bridge, Worker/host protocol, Map normalization, and focused tests.

Findings:
- The original review found the report-by-ID Worker message was unreachable from EngineHost. This was fixed by adding `publicReportById` to the optional public-query capability and wiring both WASM hosts.
- The review found public-response schema validation weaker than speed-metrics validation. The current generated public DTOs are authoritative Rust output; strict save input is parsed before restore. This remains a residual validation gap for a later shared public-query coordinator owner, not a reason to alter task-29 DTO contracts here.

Confirmed:
- Public report projection delegates only to `GameSession` public query APIs and does not expose books, journal, or NPC state.
- Decimal strings and report IDs are not coerced to Number.
- Restore creates and snapshots a candidate handle before replacing/dropping the active handle.
- Worker request/response matching requires both request ID and session generation.
- Generated wasm-pkg APIs were produced by the official build path, not manually edited.

Residual blockers are documented in the task-30 evidence files.

## 2026-09-12 Follow-up independent review

Reviewer: independent explore subagent `ses_f6af8146dffeYI6YkZWkhw3aUe`.

Result: APPROVE.

- The restore response now carries post-restore `nextGeneration`, which the host validates and adopts;
  the focused test rejects a non-advancing generation.
- All K7 account-scoped maps, including `plans.plans`, are rehydrated as numeric-keyed WASM maps.
- Public DTO parsing preserves opaque decimal IDs and fixed-point accounting strings while explicitly
  rejecting malformed values. No books, journal, NPC state, or A-share trading semantics cross or change.

## 2026-09-12 Public report period-date repair review

Reviewer: independent explore subagent `ses_f6a0c0acaffeKkQa9oI51QQraa`.

WASM boundary finding: approved. The bridge-local conversion from an accounting period to its final civil date
is correct for calendar boundaries, preserves public-only projection/null/strings, and does not affect A-share
trading semantics, saves, snapshots, maps, events, or engine state.

Cross-host blocker: the reviewer found that the shared engine `PublicReportSummary.period` remains `YYYY-MM`,
while server and desktop return it directly and the shared Web normalizer requires `YYYY-MM-DD`. Fixing that
requires an engine public DTO or non-WASM host contract change, explicitly forbidden by this Task-30 repair.
The WASM report-ready path is repaired; a release-wide uniform report-period contract remains unresolved for
the owning cross-host task.

## 2026-09-12 Central cross-host period repair re-review

Reviewer: independent explore subagent `ses_f69f97cd0ffe6RnBZE0J90zZ6G`.

Result: APPROVE.

- The previous single-source finding is resolved: `PublicReportSummary::from` reuses
  `information::publication::period_end_date` and formats that CivilDate, while the WASM bridge has no date
  arithmetic.
- The review confirmed public-only projection, explicit WASM nulls, opaque decimal IDs, and fixed decimal amounts
  remain intact. No save, snapshot, event, accounting, or A-share trade rule behavior changed.
- Desktop runtime test remains environment-blocked by missing GTK/WebKit development libraries; source routes the
  now-shared DTO directly and no local period transform remains.
