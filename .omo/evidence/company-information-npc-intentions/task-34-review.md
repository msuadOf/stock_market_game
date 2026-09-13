# Task 34 Independent Review

Date: 2026-09-12
Scope reviewed: Task 34 React components, App/mobile integration, start-date parser and production-preview E2E.

## Independent Findings

- Live production WASM report transport is blocked before React by nullable `Option` serialization: `next_cursor` is `undefined` and `supersedes` is omitted. The strict Task 33 normalizer rejects the malformed/incomplete DTO. This is an upstream WASM producer defect, not a UI fallback opportunity.
- The Task 34 UI does not alter engine, host adapter, coordinator, company slice, generated types, listed securities, or A-share trading semantics. It keeps errors explicit instead of fabricating report data or treating missing comparison as zero.
- The current CompanyPanel implementation has real rendering branches for ready reports, error/unavailable/empty/loading states, statement tabs, exact decimal display, disclosure versions, comparison explanation, and a horizontally scrollable sticky-subject table. Production WASM cannot reach its ready branch until the producer serializes nulls correctly.
- `StartDateInput` validates 2000-01-01..2099-12-31, defaults to 2030-01-01 through the existing setup, and reuses the same `SessionSetup` lifecycle for Worker, remote, and Tauri construction.

## Verification Observed

- `pnpm --filter web test`: 241 passed, 0 failed.
- `pnpm --filter web exec tsc -b --pretty false`: passed.
- `pnpm --filter web lint`: passed.
- `pnpm --filter web build`: passed.
- `pnpm --filter web exec playwright test e2e/company-information.spec.ts --trace on`: 5 passed, 0 failed at 1280, 768, and 375 widths.
- Fresh fixed-viewport screenshots are stored in `task-34-happy/`; the company error is legible and no horizontal page overflow or panel overlap was found at the three target widths.

## Verdict

REQUEST_CHANGES for the wider Task 34 acceptance outcome only: actual public report content cannot be presented in the production WASM path until the Task 30 bridge serializes nullable public DTO fields as explicit nulls. The UI implementation and its error/date/responsive behavior are otherwise ready. No UI-side workaround is approved because it would mask the shared boundary contract failure.
