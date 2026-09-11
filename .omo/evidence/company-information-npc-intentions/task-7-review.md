# Task 7 — Independent Review (E/task-7-review.md)

- Reviewer: independent subagent (ses_f73e25307ffe6hwsHZJUyT2gTU), did NOT implement
- Subject: commit `637f184` "feat(engine): 增加公司与独立经营账套" (15 files, +2430)
- Date: 2026-09-10

## Verdict basis (reviewer-performed)
1. Fund boundary: company cash only in Books (1002); CounterpartyId(String) vs AccountId(u64) compiler-enforced separation; zero crate::session imports in company/; `initialization_does_not_pay_investors` genuine — all 70 accounts snapshot-compared + full SaveSlot byte comparison + stock-spec pinning; opening funding has external Lender counterparty with flow_count==0 (state, not payment).
2. Issuer mapping: exact-share validation + 3 typed errors; 600101 hand-recomputed: 8,928,571,429 shares × 1元 par = 892,857,142,900分 (NOT price-derived ≈1e11); triple cross-check defaults.ts ↔ test pins ↔ baseline_fixture.rs — zero drift; 5 listed mapped + 4 unlisted test entities; no trading spec changes.
3. Opening mechanics: voucher → post_batch only (no bare Journal); unbalanced → typed OpeningPost with no half-constructed company; sub-ledger placeholders are pure types; opening_event_id()==1 < 2^53; serde round-trip verified by reviewer's external probe.
4. Contract/credit surface: data validation complete; NoCreditLine + DebtBeyondCreditLine typed with context; exactly-full==limit legal with test; state-unchanged on rejections.
5. Tests/scope: 15/15 green; all 6 mandated failure modes + bonus coverage (4 cycle shapes); 561/0/4 full suite; no f64/TODO/unwrap; 7-vs-5 file split + defaults.rs 290-line data table registered per precedent.

## Downstream observations (non-blocking, routed to owners)
- O1 → tasks 26/27: add in-repo serde round-trip lock for Company/CompanyRegistry; restored registries must re-validate (Deserialize currently bypasses validate_set — unlike Books' replay-on-restore).
- O2 → tasks 8–11/14: opening 2001 debt has NO seeded contract — outstanding_borrowings ignores it in credit checks; industry handlers must decide opening-debt interest/repayment governance or seed opening contracts.
- O3 (minor): validate_issuer_mapping uses first-match find; upstream session validation guarantees uniqueness.

VERDICT: APPROVE
