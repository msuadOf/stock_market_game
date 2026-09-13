# Notepad Recovery Receipt

Date: 2026-09-12
Scope: `.omo/notepads/company-information-npc-intentions/learnings.md` and `issues.md` only, plus this receipt.

## Recovery method

- Exact historical bases were recovered from the tracked `HEAD` blobs with read-only `git show`.
- The recovered base line counts were `1161` for `learnings.md` and `1140` for `issues.md`.
- The tracked base SHA-256 values were:
  - `learnings.md`: `540d1fe8e5cd8c16426f092c80b764ba352a9d56941eff472ca2a9c250d2b82a`
  - `issues.md`: `6eab99add04d597c72d835ee7f6232efb026661000289ca2c7aa0e7b88a431e1`
- The damaged working-tree files contained only 8 and 11 lines respectively, consisting of the
  Task 32 tail. Their exact pre-recovery working-tree hashes were not captured before replacement.
- Historical mojibake was retained byte-for-byte from the Git blobs. No old prose was invented or
  normalized.

## Appended sources

- Task 27: `.omo/evidence/company-information-npc-intentions/task-27-happy.txt` and
  `task-27-review.md`, including the independent APPROVE, focused test counts, capacity behavior,
  restore validation, and generated-binding cleanup.
- Task 28: `task-28-happy.txt` and `task-28-review.md`, including the final scenario outputs and
  the initial REJECT findings retained as remediation history.
- Task 29: `task-29-happy.txt`, `task-29-review.md`, and the prior session exploration transcript,
  covering public projection, immutable report ordering, decimal strings, unavailable comparison,
  strict save parsing, generated types, and verification constraints.
- Task 30: `task-30-happy.txt`, `task-30-review.md`, and the prior session transcript, covering
  WASM exports, Map normalization, generation fencing, candidate-first restore, and the build-time
  limitation.
- Task 31: independent review session `ses_f6bcc28c3ffeOIYFXZyEWvJ8yw`, whose findings are
  recorded as unresolved issues. No completion claim was made.
- Task 32: `task-32-happy.txt`, `task-32-failure.txt`, and the prior session transcript. The
  existing Task 32 tail was restored from `HEAD` and preserved, then represented in the recovered
  chronological append sections.

## Result

- `learnings.md`: 1227 lines, SHA-256
  `2e8b4217efaf27e6afb8f05496745483468adaaf6066d7f0b4f8d847b9f0f58b`.
- `issues.md`: 1195 lines, SHA-256
  `e761cb1284b536e3d29e75d95a1445776c790321b38cb40a3dd1f6bf67d992ac`.
- Both files decode as UTF-8 and end with a newline.
- Required historical headings including Tasks 1, 16, 18, 22, 23, 24, 25, and 26 remain present.
- Task 27 through Task 32 append headings are present in chronological order. Identical Task 32
  content was not duplicated.

## Unresolved loss and incident

The exact post-HEAD notepad text for Tasks 27 through 31 was not available as a complete blob or
single transcript export. The appended material therefore contains only facts directly supported
by task evidence and session outputs. Any post-HEAD prose not represented by those sources remains
unrecoverable and is not fabricated here.

A subagent overwrote append-only notepads with whole-file writes, causing the incident repaired by
this receipt. Future agents must never use whole-file write on `.omo/notepads/**`. They must recover
an exact base first, append with a bounded edit, preserve historical bytes, and record provenance,
line counts, and hashes before claiming recovery.

## Scope receipt

No product code, tests, plans, boulder state, existing evidence, Git index, or Git history was
intentionally edited by this recovery. The only new artifact from this task is this receipt.
