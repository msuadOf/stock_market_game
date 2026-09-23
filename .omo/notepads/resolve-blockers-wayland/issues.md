# Issues — resolve-blockers-wayland

Problems and gotchas encountered during work on this plan.

_Auto-scaffolded by /start-work. Append new entries below - never overwrite._

---

## 2026-09-14 Todo 5 bounded real Worker parity attempt

- The red host-identity gate first failed because the required runner module did not exist, then passed when a
  direct-engine identity was rejected. It was removed with the incomplete runner rather than leave an unimplemented
  parity surface.
- The official `bash scripts/wasm-build.sh` completed under pinned Node 24/Corepack and regenerated the WASM package.
  A real Chromium Worker loaded `/src/host/wasm-worker.ts`, created its WASM session, saved it, rejected corrupt
  restore, and preserved the original snapshot after that rejection. Its valid reload then rejected the worker's
  own save payload: first undefined optional `best_bid`, then after the existing recursive normalizer made it null,
  `invalid type: floating point 0.0, expected i128`.

## 2026-09-14 Todo 6 verification status

- Pinned Node 24 runner tests pass 45/45 after the dirty-content and publication-boundary repair.
  The existing real fresh primary seed still timed out at its explicit 900,000 ms cap with no
  output artifact, so this repair does not claim a fresh full-matrix result.
- First independent re-review rejected ignored local source files being excluded from the
  fingerprint. The repair hashes all ordinary source-root files and has an ignored-file mutation
  regression; final focused count and second review are pending.
- Current focused pinned Node 24 result is 47/47. One prior full-run test-runner deserialization
  error occurred while another Node test process was active; the quiet rerun passed every test.
- Final independent Task-6 review returned APPROVE with no blocking findings.

## 2026-09-14 Todo 6 finalization-budget repair

- Current pinned Node 24 runner verification is 53/53. Exact child-count regressions prove the
  hard permit includes primary, cross-year, and sensitivity deterministic finalization. The full
  fresh matrix remains intentionally unclaimed after the prior capacity timeout.

- Fresh independent review found no K7 correctness defect but flagged the inherited single-file
  runner size. The current task's explicit product-file allowlist excludes helper-module extraction;
  the Task-6 evidence records this constrained-file exception and requests a focused re-review.

## 2026-09-14 Todo 5 matrix execution

- The first full-size default save correctly hit the Server's retained 8 MiB raw-load body limit (HTTP 413). The
  matrix uses a small, valid one-institution A-share fixture for cross-transport restore instead; the default-scale
  restriction remains a documented deployment boundary and was not relaxed.
- Weston headless logs the known no-keyboard-seat GDK warning and software EGL permission warning. The matrix only
  claims native WebView invoke/IPC, not physical input-seat coverage. The final run cleaned all task-owned Server,
  Vite, Weston, and Tauri processes and deleted isolated runtime/config directories.

## 2026-09-14 Todo 5 lifecycle mapping

- The coverage-manifest red test now makes every named scenario fail closed when omitted. The incomplete current
  driver must not be represented as a complete matrix: normal/closed civil progression, no-trade disclosure,
  duplicate/out-of-order lifecycle inputs, concurrent operations, full Server WS reconnect/resync, and diagnostic
  feature records still require real transport seams beyond the verified restore/stale/privacy foundation.

## 2026-09-14 Todo 5 actor seam attempt

- Server and Tauri now have feature-gated `host-parity` one-civil-day commands that travel through their existing
  actor queues, preserve engine synchronization gates, and emit their ordinary server broadcast/Tauri event path.
  Focused Server actor+authenticated-route and Tauri mock IPC tests passed. The native event-probe and full Worker/
  Server/Weston matrix remain unexecuted due to the Node/Corepack blocker recorded in problems.

## 2026-09-14 Todo 5 final privacy and diagnostics repair

- The final matrix separates native Tauri default and `simulation-diagnostics` feature builds. The aggregate manifest
  machine-enforces every normal scenario on every artifact, Tauri default unsupported diagnostics on the default
  artifact, and bounded supported diagnostics only on the feature artifact. The Server WS probe reads its Bearer token
  from inherited stdin before connecting, never process argv or persisted evidence; raw output records only token
  presence.

## 2026-09-14 Todo 7 K7 foreground capacity observation

- The first isolated after batch was terminated by the executor's 600,000 ms foreground harness
  allocation before its seed-1 child published a raw report, receipt, checkpoint, or manifest.
  Its empty directory is retained as non-success and is not resumed. A fresh task-owned detached
  batch now runs under the validated source-fingerprint contract with separate stdout/time logs.
- The detached exact batch subsequently published a fully verified 336,863,108-byte seed-1 report
  after roughly two hours, then began seed 2. At the latest observation seed 2 was CPU-active for
  52 minutes with no partial JSON/receipt, so it remains incomplete by design. The focused pinned
  Node 24 runner suite passed 53/53, but it is not a substitute for the remaining real matrix.
- Seed 2 later completed and published a 340,121,477-byte verified report, after which the runner
  began seed 3. The primary matrix remains only 2/10 complete and must not receive a manifest,
  aggregate quantile/extrema claim, or sensitivity substitution until all seed and finalizer
  permits finish under the unchanged source identity.

## 2026-09-15 Todo 5 fresh matrix repair and bounded rerun

- Added red/green coverage for matrix evidence: returned Worker/Server/Tauri identities are now validated at the
  aggregate boundary, while source revision/dirty state, Node/Corepack versions, a SHA-256 inventory of raw logs and
  artifacts, capability differences, and an aggregate cleanup receipt are required before `comparison.json` is
  written. The runner rebuilds WASM before starting any host, so stale generated bindings fail closed.
- The required fresh command `node scripts/simulation/host-parity.mjs --output
  .omo/evidence/resolve-blockers-wayland/task-5-parity/run-20260915-node25-repaired` correctly failed before host
  launch: WASM compilation succeeded, but system Node `v25.8.2` Corepack `0.24.0` crashed when pnpm's dynamic import
  began (`ERR_VM_DYNAMIC_IMPORT_CALLBACK_MISSING`). The retained `wasm-build.log` contains the exact error and a
  process probe found no task-owned Server, Vite, Weston, or Tauri process. This is a truthful non-success receipt,
  not a fresh parity claim; rerun requires the repository's Node 24.18.0/Corepack 0.35.0 runtime.

- Correction to the preceding draft repair claim: the evidence/receipt implementation was withdrawn in this turn.
  Self-adversarial review found that a list of completed hosts is not proof of process cleanup and revision plus a
  dirty boolean does not bind dirty source bytes or built binaries. The speculative receipt test and implementation
  were reverted to their inherited state rather than leave misleading-success evidence. The retained build log
  describes the attempted, subsequently withdrawn runner revision. No product repair or Task-5 completion is claimed.
- Remaining inspection findings: aggregate validation checks synthetic transport literals instead of all four
  returned identities; default Tauri hashes are not compared with other hosts; cross-host event payload equality is
  not asserted; Worker has no separate closed-day fixture; required no-trade disclosure publication is not actually
  exercised. These require failing-first runtime proofs and a complete real rerun, not boolean-only manifests.
- QA disposition: inherited manifest tests passed 4/4; attempted receipt tests passed 5/5 but were insufficient and
  withdrawn. Fresh native visual/IPC, concurrent mutation, interruption/cleanup, and hung-host probes remain
  unexecuted (blocked, not N/A). Malformed CLI and scenario omission are covered only by existing guards/tests.
  Dirty worktree was observed and left intact. The fresh build failed nonzero without comparison.json, so no
  misleading success was published. X11 fallback and physical keyboard input are out of scope, not substitutes.

## 2026-09-15 Todo 5 runtime recovery and fresh inherited-matrix result

- The pinned Node runtime was subsequently located at
  `/home/baiyifan/.claude/tmp/opencode/node-v24.18.0-linux-x64/bin`. With that directory prepended to PATH,
  the official WASM/Web rebuild and inherited host-parity matrix both exited zero. The prior runtime blocker
  is resolved, not a remaining reason to reject execution.
- Fresh evidence is `task-5-parity/run-20260915-node24-independent/`; `review.md` records exact commands,
  observed artifact hashes, matching public hashes, process-cleanup observation, and remaining coverage gaps.
  No native screenshot or interruption-safe cleanup proof was obtained. The independent verdict remains
  NEEDS WORK despite the inherited matrix's zero exit. The Task-5 checkbox remains unchanged.

- Independent report review flagged the inherited `comparison.json` no-trade-disclosure boolean as
  inconsistent with actual acceptance coverage. Raw output was preserved rather than rewritten;
  a separate `acceptance.json` now explicitly marks the run not accepted and publication, event equality,
  closed-day Worker, visual capture, source binding and interrupted cleanup as unverified. The report
  warns that the raw scenario matrix is not an independent acceptance manifest.

## 2026-09-15 Todo 7 current execution state

- The stopped legacy after process completed seed 3, so its historical primary checkpoint has three
  validated raw/receipt/aggregate entries. A red-first repair discovered that its identity also
  bound unrelated dirty-worktree status, including the runner's own ignored evidence output. That
  made exact source-byte-equivalent resume fail. The revised source-scoped Git provenance therefore
  invalidates the old checkpoint by design; it is retained but never reused.
- A new exact 11-permit after batch began under Node 24.18 at 15:29 local after process and 3.4 TiB
  free-disk preflight. Its time/Node/fixture PIDs are 2220882/2220883/2220906 and it is actively
  running primary seed 1. No task-owned process was killed. It has not published a report yet, so
  neither a completed matrix nor resource-finalization receipt is claimed.

## 2026-09-15 Todo 7 long-command probe

- At 21:25 local the current after fixture had 3h24m CPU and 571,744 KiB RSS; parallel worker
  threads remained runnable while Node polled and `/usr/bin/time` waited. No artifact published,
  exactly as required by the raw-report atomic-publication contract. With 3,714,338,598,912 bytes
  free, this is capacity duration rather than a hung process, disk exhaustion, or misleading output.
  The valid process remains untouched.

## 2026-09-16 Todo 7 continued live execution

- At 02:30 local the fresh primary-1 fixture remained CPU-active after 11h15m elapsed, with 5h59m
  CPU and 636,328 KiB RSS. No task-owned report, receipt, checkpoint, manifest, stdout, or final
  time log had published; its output root remained zero bytes. The process remains untouched, and
  sensitivity cannot start until a validated after checkpoint or final receipt exists.

## 2026-09-15 Todo 4 independent verification

- No new Task-4 issue found. Current isolated tests and the live checker pass against the current tree.
- The inherited evidence references an unavailable markdown-link script, so this verification records manual relative-link resolution instead of claiming that absent script was run.

## 2026-09-15 Todo 5 final repaired matrix

- Prior session aborted for procedural reasons (runtime path initially unknown), not capability: pinned Node 24.18.0 at
  `/home/baiyifan/.claude/tmp/opencode/node-v24.18.0-linux-x64/bin` runs the full stack. Resume came from
  `run-20260915-node24-independent` evidence rather than duplicate searching.
- Failing-first repairs (all red before green in `host-parity.test.mjs`, now 9/9): returned-artifact identity
  validation; live Saturday `CompanyDisclosurePublished` no-trade fixture via each host's real public query surface
  (start 2030-04-19, publish 2030-04-20T18:00); cross-host normal/closed event-stream and disclosure-page hashes
  across all four artifacts; authenticated generation-0 stale step requiring HTTP 400; SIGKILL-escalating cleanup that
  awaits observed exit with per-host receipts; provenance.json (revision/dirty/node/corepack/input SHA-256) and
  machine-readable capabilityDifferences in comparison.json.
- Fixture date discovery cost three fail-closed runs (complete/complete2/complete3, no comparison.json published):
  starting on the scheduled Saturday itself lets prehistory seed the report (no live event), and the engine's company
  schedule offset differs from my first JS re-derivation. The engine is authoritative; fixture starts the prior
  Friday so the Saturday advance publishes live.
- Final run `run-20260915-node24-final-verified` exits zero: public/restore/event/disclosure hashes identical across
  worker/server/tauriDefault/tauriFeature; stale/corrupt/duplicate/unauthenticated reject by contract; cleanup receipt
  shows all six host processes exited (SIGTERM observed, escalation proven by unit test).
- Native visual: first `wayland-native-qa.sh` attempt failed truthfully at the flaky weston-screenshooter (timeout
  124, no PNG, processes cleaned); the retry `run-20260915-node24-final-visual2` produced a validated 1280x800
  Pixman Weston PNG with integrity manifest; manual inspection confirms the real A-share trading UI (行情/委托下单/
  公司信息披露/分时成交), no X11 substitution.
- Remaining honest limits: reload stays within each live host process; equality covers public projection + event
  streams + disclosure pages, not host-private state; runner-level SIGINT injection not executed.

## 2026-09-15 Task 5 freshness audit resolution

- Rejected candidate omitted automatic WASM generation and hashed the shared Tauri output only after variant builds. Repaired runner builds first and executes exclusive per-run binary copies with mandatory pre/post SHA-256 checks. Final evidence: task-5-parity/run-20260915-freshness-final; 14 tests and full real host matrix pass.
- LSP unavailable (previous installation declined). Existing port-collision/cross-session-auth test coverage, broad socket retry, and misleading forced=true after SIGTERM cleanup flags are retained as bounded pre-existing observations, not repaired or hidden. Native proof stays 1280x800 Weston/Pixman, not universal Wayland/input support.

## 2026-09-15 Todo 5 final approval limits

- Final approval found no blocker. Retained non-blocking limits are controlled Weston/Pixman rather than all Wayland compositors or keyboard-seat input, reload within a live host process, public/event/disclosure rather than private-state parity, no runner-level SIGINT injection, and no hostile in-process or mutate-and-restore filesystem-race claim.

## 2026-09-16 Task 7 cross-year failure resolved locally

- The prior cross-year resume was fail-closed rather than swallowed: the run panicked after roughly 35 seconds at day 173/tick 52066. The local fixed verification ran the full requested 400 natural days (271 trading days) in 2:15.11 wall time with exit 0, so it exceeded the required five-minute-past-panic proof even though it completed before the cap.
- LSP diagnostics could not complete because the shared daemon timed out repeatedly. Cargo compilation, targeted regression, complete `plan_execution` integration target, formatting, diff check, and the real fixture were run instead; an independent diff review remains required before acceptance.

## 2026-09-16 Task 7 V3 superseded for concurrent-seed rerun

- The active V3 after process is source-stale by design after the runner policy edit. Its partial raw/receipt/checkpoint output is retained as historical non-final evidence but must not resume or finalize. Stop it before V4 launch so its single active seed does not steal capacity from the authorized concurrent rerun.
- `perf stat` cannot be used on this host because `perf_event_paranoid=4` denies performance-monitor access. The retained reproducible CPU evidence instead uses process/thread affinity, `/proc/<pid>/status`, runnable-thread counts, and five-second user+system CPU deltas; it measures behavior beyond a single aggregate process percentage.
- The bounded representative fixture initially used one natural day and correctly failed because 2030-01-01 is closed, so price-volume diagnostics reject zero trading days. The corrected two-natural-day `cross-year 1 2 1 1 1` fixture completed with fresh source, one trading day, one closed day, and 2030-01-03 end date. This was a fixture-calendar boundary validation, not a K7 runner defect.

## 2026-09-16 Task 7 single-seed measurement caveat

- Per-process CPU-time sampling uses `/proc/<pid>/stat` user+system ticks and one-second runnable-thread snapshots. It measures sustained use across the exact named fixture command and affinity partition, but it does not label individual internal source phases because the optimized release binary lacks profiler permission (`perf_event_paranoid=4`) and adding phase instrumentation would alter the K7 source identity during the live V4 run.

## 2026-09-16 Task 7 V4 superseded by serial V5 policy

- V4's active after roots are source-stale by design under `k7-resource-policy-v2` and must not resume or finalize. Atlas, not this implementation task, should stop these observed V4 process trees only after its review: `/usr/bin/time` 657219 -> Node 657220 -> fixtures 657298, 657299, 657302, 657304, 657306, 657338, 657475, 657584; and the resumed tree sandbox 668954 -> Node 668955 -> fixtures 668978, 668979, 668980, 668981, 668982, 669022, 669157, 669300.
- The short cross-year bounded fixture can finish before a second `/proc` sample, so it validates detected thread propagation and byte identity but is not presented as sustained CPU-utilization evidence. The primary bounded fixture remained running long enough to observe the expected main-plus-two-worker process thread count.

## 2026-09-17 Task 7 V5 superseded by explicit V6 cap policy

- The existing V5 process tree remains running and was not stopped: `/usr/bin/time` PID `712159` -> Node PID `712160`, output `.omo/evidence/resolve-blockers-wayland/task-7-after-v5-serial-dynamic`. Its v2 resource identity is stale after the v3 policy/source change, so it cannot finalize or resume as V6 evidence. Atlas must stop that tree only after it accepts this implementation.
