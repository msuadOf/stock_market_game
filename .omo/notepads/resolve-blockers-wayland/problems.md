# Problems — resolve-blockers-wayland

Unresolved blockers and technical debt discovered during work on this plan.

_Auto-scaffolded by /start-work. Append new entries below - never overwrite._

## 2026-09-14 Todo 4 review repair

- The first semantic checker revision exposed that broad negation matching could treat ordinary words such as
  `not` inside unrelated text as qualification. Word-bounded negation tokens were added, and all 27 isolated
  negative fixtures now fail as intended.

---

## 2026-09-14 Todo 6 finalization durability boundary

- The new finalization receipt is integrity-checked and atomically visible, but not fsync-durable
  under sudden power loss. Its missing or altered state is rejected or recomputed under a newly
  acquired permit; no final manifest is emitted from an unfinalized matrix.

## 2026-09-14 Todo 5 real Worker reload blocker

- Generated WASM Worker save output cannot currently round-trip through its own required strict JSON
  `parseSaveSlot`/`prepareSaveForWasm` boundary. Optional market fields were serialized as `undefined`; after
  normalizing those to `null`, an authoritative `i128` field was still represented as floating-point `0.0` and
  rejected by Rust restore. Therefore a real Worker save/reload parity driver cannot meet Todo 5 without a separately
  scoped, schema-preserving WASM save serialization repair. No direct engine substitute, stale WASM, or fake host
  artifact was retained.

## 2026-09-14 Todo 5 resolution

- The Worker reload blocker is resolved by `restore_json`: the strict JSON payload preserves the original i128 tokens
  that `serde_wasm_bindgen::from_value` could not recover from JS `number`. The full three-host live run now records
  matching public and restored hashes, explicit corrupt rejection with unchanged public state, and release diagnostics
  without records. The independent review additionally identified that event-driven normal/closed-day, no-trade
  disclosure, duplicate/out-of-order ingestion, concurrent requests, reconnect, and feature-enabled diagnostic record
  parity remain uncovered by this first matrix fixture. The Server's existing real WS suite separately covers
  authenticated baseline/frame delivery, malformed commands, pull behavior, and restore resync, but those results are
  not yet normalized into this driver.

## 2026-09-14 Todo 5 remaining lifecycle-matrix blocker

- The real-host parity foundation cannot truthfully claim the remaining event lifecycle scenarios with the current
  production surfaces. Worker has an explicit `endCivilDay` postMessage seam, but Server only settles civil dates
  after its wall-clock actor reaches a market boundary and Tauri only settles after an emitted `DayBoundary`; neither
  offers a deterministic one-civil-day command. The live Tauri WebKit probe exposes `invoke`, but not a supported
  application event-subscription bridge for capturing ordered `engine-event` payloads. Adding direct engine calls,
  synthetic event injection, or an unauthenticated query-token WebSocket workaround would violate Task 5.
- Server WS baseline/frame/resync can be exercised by the existing real Rust `tokio-tungstenite` integration client,
  but the Node 24 runtime has no installed header-capable WebSocket client. Native/browser WebSocket cannot send the
  required Bearer upgrade header, and the Server deliberately rejects query-string credentials. A dedicated Rust
  parity WS probe or a declared header-capable Node dependency is required; neither is currently part of the approved
  script-only parity seam.

## 2026-09-14 Todo 6 remaining capacity evidence

- Atomic rename supplies visibility, not guaranteed power-loss durability without fsync. The runner
  nevertheless fails closed for every missing or inconsistent publication layer. A full fresh K7
  execution remains blocked by capacity, not by a reduction of the mandated matrices.
- The declared source-root policy is intentionally conservative: any ordinary ignored file under
  a K7 engine/build/runner root changes the checkpoint fingerprint, while symlinks or unsupported
  filesystem entries cause explicit failure rather than invisible reuse.

## 2026-09-14 Todo 5 host-parity execution blocker

- The official WASM build regenerated the current Worker artifact successfully, but its required Corepack Web
  build and the real host-parity Worker runner cannot start in this environment because the available Node 25.8.2
  Corepack process aborts with `ERR_VM_DYNAMIC_IMPORT_CALLBACK_MISSING`. The failed run is retained outside the
  repository under `/home/baiyifan/.claude/tmp/opencode/host-parity-20260914/`; no partial matrix is represented
  as evidence.

## 2026-09-14 Todo 5 resolved runtime note

- The Node 25 Corepack failure is an environment-specific system-runtime issue, not a remaining parity blocker: the
  final matrix uses the already-installed pinned Node 24.18.0/Corepack 0.35.0 runtime and succeeds with all required
  real transport scenarios. The final receipt is `task-5-parity/run-20260914-node24-final-private/`.

## 2026-09-14 Todo 7 execution remains in progress

- One full-cost fresh primary K7 child exceeds the 600-second foreground execution allocation. The
  task-owned detached after batch is intentionally left running rather than reducing seeds/days,
  using a save, or treating an empty directory as a checkpoint. Its eventual source-bound raw
  reports, receipts, manifests, and resource log require inspection before any completion claim.

## 2026-09-15 Todo 7 remaining execution blocker

- Full-cost matrix duration exceeds this executor turn: prior real primary reports took roughly two
  hours each and the required work still includes the full ten-seed primary with canonical rerun,
  five-seed 400-natural-day cross-year with canonical rerun, and seven unique ten-seed sensitivity
  matrices with their seven canonical reruns. The freshly launched exact after batch is active; no
  complete manifest, aggregate quantile/extrema claim, sensitivity output, final `/usr/bin/time -v`
  receipt, cleanup receipt, or C06 completion can honestly be supplied until detached execution
  exits and all raw/receipt/checkpoint chains pass read-only validation.

- The active fixture is CPU-active rather than hung, so forced interruption is not a safe completion
  tactic. The next safe state transition is atomic raw-report publication or process exit; then
  validate every raw->receipt->aggregate relationship under the fresh source identity, capture final
  time/disk facts at exit, write cleanup receipt, and only then start the exact sensitivity matrix.

- At 02:30 local the active after fixture still had no atomic output after 11h15m elapsed despite
  active multi-core CPU use. Task 7 remains blocked by real matrix duration: no after manifest,
  sensitivity artifacts, final resource/cleanup receipt, or quantitative completion claim is valid.

## 2026-09-15 orchestrator: Todo-8 escalation threshold (post-compaction insurance)
- bg_59ff1ade = THIRD resume of session ses_f5b4f28ccffeQORPESWv1fAk4o (Todo 8 release-contract).
  Two prior resumes died on 30-min inactivity timeouts with only the 26-line test stub on disk.
- Decision rule for future ticks: if this resume times out again OR produces no `release-contract.mjs`
  within ~30 min of dispatch, STOP resuming that session — spawn a FRESH deep session with distilled
  context (plan lines 117-123, existing stub path, Task-5 build/hash patterns, anti-timeout protocol,
  heartbeat-wrapped builds). Three identical failures = looping on broken approach.
- K7 after batch self-driving: seed-3 in flight (~44m @ 754% CPU), seeds 1-2 sealed+validated.
  Remaining serial launches after batch exit: cross-year 5x400, then sensitivity 77 permits —
  dispatch when after-matrix approaches seal; do not pre-launch.

## 2026-09-15 orchestrator: after/sensitivity launch chain (from baseline-run.mjs:911-963)
- `after` 命令是自包含的：primary(10×30) → cross-year(5×400) → finalize → manifest（含 C06 not_completed_no_authorized_data）。
- 当前批次 `after --batch-size 11` 的 11 个许可 = primary 10 seeds + 1 规范重跑 finalizer。
  许可耗尽后进程退出，cross-year 需后续 `after --resume`（同 --output 目录，checkpoint 续跑）。
- 后续编排链（批次进程退出后逐段验证+派发）：
  1. primary 密封（seed-10 + rerun manifest 字段 complete+finalized）
  2. `node scripts/simulation/baseline-run.mjs after --output .omo/evidence/resolve-blockers-wayland/task-7-after-resume-v2 --resume --batch-size <N>` → cross-year 5+1 许可
  3. after manifest 完整密封后 → `sensitivity --output <sensitivity-dir> --resume --batch-size 77`
- 每段启动前：checkpoint identity 校验 + 磁盘/进程 preflight；不许并行 sensitivity。

## 2026-09-16 00:20 orchestrator: Todo-8 escalation executed (4th attempt = FRESH session)
- bg_59ff1ade (3rd resume) sat ~5h with zero output and a lost/never-delivered timeout
  notification; background_cancel revealed it had silently died with status=error.
- Per the recorded three-strikes rule: dispatched FRESH deep session bg_f03c85cc
  (ses_f5a21ac80ffe1ND5Q6kIEkLQXl) with distilled context, phase-ordered execution
  (files-first), and mandatory heartbeat-wrapped builds. Escalation threshold for this
  attempt: if no release-contract.mjs on disk within ~40 min of dispatch, report to user
  and consider splitting Todo 8 into smaller sub-dispatches.
- K7 batch unaffected: primary 7 in flight (PID 359134, ~1h23m at last check), seeds 1-5 sealed.

## 2026-09-16 Task 8 Fresh-Pass Intermediate Result

- Verifier artifact scanner and eight passing tests now exist; missing-module red baseline observed before implementation.
- Fresh normal `scripts/wasm-build.sh` passed with heartbeat. Built worker still contains `npc_decision_trace`; strict artifact scanner exits 1, so clean-release success is not claimed.
- Evidence: `task-8-release/status.md` and `wasm-build.log`. No native services or K7 processes were touched.
- Remaining: resolve release worker command isolation without weakening detection, add all-surface `--output` orchestration, fresh default native builds, bearer HTTP, Weston IPC, full hash and cleanup receipts. Current CLI only implements `--inspect`.

### Task 8 Continuation Checkpoint

- Worker production trace code fixed with a DEV guard after built-artifact red test; 12 release tests and 249 Web tests now pass. Fresh default server/Tauri release builds pass; bearer HTTP unsupported/no-fields and normal snapshot pass. Built clone injection exits 1 as required.
- Real default Tauri release IPC remains blocked: no WebKit inspector target under Weston/Pixman. Read-only investigation found Tauri/Wry release inspector support gated by devtools compile feature; default binary lacks it. No debug/feature substitution made.
- Evidence and independent partial review: task-8-release/status.md, review.md, run-2/. Native processes exited, temporary dirs removed, both probe ports unbound. Full task completion not claimed.

## 2026-09-16 Task 8 Approved Completion Path

- Accepted alternative for default Tauri release completed: release-profile IPC handler tests, live Weston/Pixman default-binary PNG, and verifier-enforced diagnostics-feature-versus-default symbol absence replace structurally unavailable external release inspector IPC.
- Final `release-contract.mjs --output .../task-8-release/final-8` exited 0. Aggregate receipt `final-8/result.json`; Tauri symbols record 14 feature-only traces/collector symbols and zero default matches. No devtools/debug/diagnostics binary was substituted for the default release launch.
- Default external IPC limitation remains explicit: Tauri/Wry inspector is compile-time devtools gated in release. Cleanup receipt records SIGTERM for app, normal Weston exit, removed private dirs; post-run ports 19238/19239 unbound. K7 and task-7 untouched.

### Task 8 Final Review Repair

- Independent review identified missing default Server binary diagnostics exclusion. Verifier now feature-compares Server and Tauri symbols. Fresh final-9 all-surface run exited 0; server-symbols has 13 feature-only diagnostics symbols and zero default matches; Tauri has 14 and zero default matches. Evidence in task-8-release/final-9.

## 2026-09-16 Task 7 single-seed CPU investigation

- Hypotheses: H1 the Rayon pool is unavailable or CPU-affinity constrained; decisive evidence is a missing worker pool or narrowed affinity. H2 the independent-NPC decision kernel is present but briefly active while ordered routing/matching/settlement/diagnostics dominate; decisive evidence is all worker threads sleeping with one runnable session thread during sustained compute. H3 a further deterministic parallel phase can be isolated without changing event/order semantics; decisive evidence is an immutable batch whose output can be sorted/reduced exactly before the next mutation boundary.
- Debug artifacts to remove before final disposition: no source instrumentation planned; any measurement JSON/log goes under `/home/baiyifan/.claude/tmp/opencode/` and is removed after digest/evidence values are appended here or to the Task-7 notepads. The live V4 process is observational only and must not be stopped by this investigation.
- H1 is refuted: the single-seed 16-worker run had full 112-127 affinity, 17 OS threads, a mean 5.26 runnable threads, and peak 11. H2 is confirmed: the same run averages 3.09 CPU cores, not the 16 workers configured, matching the mandatory ordered session phases. H3 has no safe minimal candidate: the next authoritative seams mutate shared order books, accounts, order/event sequences, plan state, and deterministic causal/diagnostic order. Replacing their serial semantics would be a redesign, not a safe scheduling patch.
- V4 does not need supersession. It already applies the tested `RAYON_NUM_THREADS=16` policy and the current source fingerprint intentionally records it. No engine/runner code change is justified from this investigation.

## 2026-09-17 Task 7 V6 matrix execution blocker

- Full mandatory K7 after/sensitivity execution remains duration-bound: V6 invalidates all V5 checkpoints by source fingerprint/resource-policy identity. The clean V6 chain must start only after Atlas stops the V5 tree and must not reuse its raw artifacts: `PATH=/home/baiyifan/.claude/tmp/opencode/node-v24.18.0-linux-x64/bin:$PATH node scripts/simulation/baseline-run.mjs after --output .omo/evidence/resolve-blockers-wayland/task-7-after-v6-thread-cap --batch-size 17 --max-threads auto`; after the after manifest seals, run `sensitivity --output .omo/evidence/resolve-blockers-wayland/task-7-sensitivity-v6-thread-cap --batch-size 77 --max-threads auto`.
