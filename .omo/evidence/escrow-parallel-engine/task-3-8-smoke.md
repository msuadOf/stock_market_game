# Tasks 3–8 current-source multi-core smoke

Date: 2026-09-23 Asia/Shanghai. Revision:
`94f337eaafd0c3ed4e83a0a85182239c9170885b`.

The user explicitly requested one combined smoke instead of repeated task-by-task exhaustive
runs. The current source was unchanged under `packages/engine`, all three hosts, Web and bindings
while this command ran.

```text
CARGO_BUILD_JOBS=16
RAYON_NUM_THREADS=16
CARGO_TARGET_DIR=$PWD/.tmp/task-8-smoke-target
TMPDIR/TMP/TEMP=$PWD/.tmp/task-3-8-current-smoke/process
timeout 300 cargo test -p engine --lib --features simulation-diagnostics -- --test-threads=16
```

Result: **PASS**, 785 passed, 0 failed, 0 ignored, 0 filtered; test execution 46.70s;
compile + execution wall 122.42s; maximum RSS 3,168,340 KiB. Build used 16 jobs and the Rust
test harness used 16 threads; this was not a single-core run. Raw output:
`task-3-8-current-smoke.log`, SHA-256
`5649d2171828894f35e61cf6871c068e5beae9c6fbb5d70c4e287fe1a445b3a7`.

The library smoke directly includes the production pipeline's envelope conservation and audit
chains (Task 3), P3/P4 validation and rollback paths (Task 4), receipt settlement and allocation
cutoffs (Task 5), opening/closing auction state machines and rollover/day-end behavior (Task 6),
typed fatal/poison and single-point commit paths (Task 7), and 29 v2 persistence tests plus live
save/restore evidence projections (Task 8). It also includes the mandatory negative-path tests
under their actual Rust symbols, avoiding zero-test legacy filter names.

Additional already completed component evidence:

- Task 7 hosts: focused Web 24/24, server 86 passed / 1 ignored, TypeScript and lint PASS,
  production build PASS, Playwright 5/5 with two workers; independent whole-host review APPROVE.
- Task 8: current-HEAD persistence smoke 29/29 with four build jobs/four test threads and a
  full acceptance-symbol map in `task-8/acceptance-map.md`; independent whole-component review
  APPROVE.
- Cross-component: preserved-test verifier PASS; post-commit K7 94/94 with 30 child processes ×
  four Rayon threads; integrated candidate review APPROVE.

This record does not claim that every historical command string was rerun. It records the single
current-source combined smoke authorized by the user and retains the original raw output so
failures, warnings, test count and concurrency can be audited.

## Current-source closing gates

- Preserved-test verifier: PASS; baseline ancestry true, 51 tracked files, 461 tracked hunks,
  440 classified, zero untracked entries. Raw `task-3-preserved.log`, SHA-256
  `d62fa78e24c3f2fc2446a973c950ad4d9c2e31e9d62514b5138f069189161a11`.
- Auction/save integration batch: `auction` 22/22, `save_contract` 14/14,
  `verification_evidence` 1/1; 16 build jobs and 16 test threads; execution plus build 78.86s.
  Raw `task-6-8-integration.log`, SHA-256
  `e8784594e56a8cdd50761e3c3808c6fc60abe7b18c06328b2b22319e8b4bd530`.
- Web host smoke: 13/13 using Node test concurrency 8. Raw `task-7-web-smoke.log`, SHA-256
  `5daf4d88e557229e8c6bd48d37afc4d59b960edf6ccd9f49878c94ae7554d88b`.
- The package-manager wrapper failed before executing because Node 25/Corepack raised
  `ERR_VM_DYNAMIC_IMPORT_CALLBACK_MISSING`; it is not counted. The underlying documented
  `types:generate` step was therefore executed directly with 16 build/test threads: 53/53 binding
  exports passed, followed by `node scripts/check-generated-types.mjs` exit 0. Raw generation log
  `task-7-types-generation.log`, SHA-256
  `0d3211628befd7d44fab084f903abac67917918007e58f5e80929e7df738db2f`.
- Current-source three-host Rust batch used 16 build jobs and 16 test threads and passed:
  server 86 passed / 1 explicitly ignored manual performance probe, desktop 21/21, and web-wasm
  5/5; wall 88.83s. Raw `task-7-three-host.log`, SHA-256
  `1727a97889e8f2000efa027a3fa87c829129858c46e3926dab423854d511717a`.

These gates close the independent reviewer's concrete gaps: Task 3 preserved-test receipt,
Task 6 restored auction remainder/order, Task 7 current-source host/binding path, and Task 8 quiet
points plus uninterrupted continuation. They do not conceal the failed Corepack wrapper.
