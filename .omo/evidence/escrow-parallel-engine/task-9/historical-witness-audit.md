# Task 9 historical witness re-audit

Audit date: 2026-09-23 (Asia/Shanghai).

Status: **MISSING_HISTORICAL_WITNESS**. Task 9 remains **INCOMPLETE (9/10 required
surfaces)**.

The audit searched every local `baseline-corpus/attempt-01` through `attempt-12` artifact and all
Git objects reachable from `git rev-list --all`. It accepted only an existing old-side artifact
whose own configuration both starts the relevant account with zero cash and records an accepted
normal multi-leg terminal path. No fixture was modified, synthesized, renamed, or reclassified.

## Local attempts 01–12

- Attempts 04, 06, 11, and 12 are the only attempts carrying both `manifest.json` with
  `status: sealed` and `seal.json`. For each, the actual manifest SHA-256 exactly matches the
  `manifest_sha256` stored in its seal; each seal records 40 runs.
- Attempts 01, 03, 05, 07, and 08 contain no JSONL corpus run.
- Attempts 02, 09, and 10 contain JSONL runs but no sealed manifest and therefore cannot supply a
  stronger historical witness than a sealed attempt.
- Across all 257 JSONL run artifacts in attempts 01–12, 224 comparable old-side configurations
  contain `initial_state.snapshot.accounts["0"].cash = 10000000` cents. There are **zero**
  comparable configurations with cash equal to zero.
- The remaining 33 configurations with no old-side cash field are `iii-new-engine-only` stress
  artifacts. They have no old-side state and cannot prove old-side acceptance.
- In particular, all 70 `i-equivalence` artifacts (ten seeds in each of attempts 02, 04, 06, 09,
  10, 11, and 12) initialize account 0 with `10000000` cents. All 70 isolated-divergence
  artifacts and all 64 controlled-representation artifacts with an old-side configuration do the
  same.

The read-only enumeration was captured locally as
`.tmp/task9-zero-cash-history-audit.tsv` (257 rows, SHA-256
`c6200d0b24c1945edc97f30be45b4a59d6eee582cd5e4decb32a8cae16608268`). The temporary table is
only an audit aid; the sealed attempts themselves remain untouched.

## Git-reachable history

`git log --all -- .omo/evidence/escrow-parallel-engine/baseline-corpus
.omo/evidence/escrow-parallel-engine/task-9` returned no commits, and `git rev-list --all
--objects` returned no attempt directory, equivalence JSONL, or Task 9 corpus-diff artifact. Exact
`normal-multi-leg-terminal` string hits in reachable commits occur only in verifier/contracts and
their tests; none is a captured old-side corpus artifact. This also covers the reachable stash refs
included by `--all`.

## Conclusion

There is no verifiable old-side zero-cash accepted witness for
`equivalence:normal-multi-leg-terminal` in the local attempts or Git-reachable history. The missing
surface must remain open. Manufacturing a new old-side result, relabeling another scenario, or
weakening the zero-cash/accepted requirements would violate the evidence policy and is not done.
