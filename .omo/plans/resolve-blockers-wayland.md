# resolve-blockers-wayland - Work Plan

## TL;DR (For humans)
<!-- Fill this LAST, after the detailed plan below is written, so it summarizes the REAL plan. -->
<!-- Plain English for a non-engineer: NO file paths, NO todo numbers, NO wave/agent/tool names. -->

**What you'll get:** A reproducible Wayland-first desktop QA path plus complete, independently reviewed evidence for every presently blocked host, baseline, release, documentation, and final-gate task.

**Why this approach:** Weston must prove rendered native UI and IPC rather than a live process, while K7 data collection must resume safely without reducing the required matrix.

**What it will NOT do:** It will not alter A-share trading rules, fake screenshots, reduce seeds, silently raise limits, or use X11-only results as a Wayland pass.

**Effort:** XL
**Risk:** High - native rendering and long deterministic simulations are environment/resource sensitive.
**Decisions to sanity-check:** Pixman Weston capture is primary; WebDriver receives only a bounded probe; 8 MiB remote-body mismatch is documented, not hidden.

Your next move: execute this plan with `$start-work resolve-blockers-wayland`. Full execution detail follows below.

---

> TL;DR (machine): XL high-risk blocker-resolution plan: reconcile evidence, validate native Weston UI/IPC, complete parity and resumable K7 matrices, then release/final review.

## Scope
### Must have
- Pixel-valid native Tauri capture and real IPC under Weston Wayland at ≥1280 width; Xvfb only as separately reported fallback.
- Targeted reconciliation of unchecked evidence/review debt, real WASM/Server/Tauri parity, fresh resumable K7 matrices, release isolation, final docs/evidence/F1–F4 reviews.
- All final gates run from a clean worktree at a pinned committed revision with regenerated WASM/artifacts.
### Must NOT have (guardrails, anti-slop, scope boundaries)
- No matching/T+1/fee/price-limit/calendar/RNG changes; no fake/black/shell-only screenshots, stale WASM, grep-only release proof, reduced matrices, silent limit increase, historical evidence deletion, or broad dependency migrations.

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: TDD using Cargo tests, Node built-in tests, Playwright, live HTTP/WS/IPC, and Weston capture.
- Evidence: `.omo/evidence/resolve-blockers-wayland/task-<N>-*`; final sealed evidence is copied/linked only after validation.

## Execution strategy
### Parallel execution waves
> Target 5-8 todos per wave. Fewer than 3 (except the final) means you under-split.
- Wave 1: 1–4 in parallel — reconcile receipts/clean revision, Corepack+clippy, Weston visual/IPC, named-limit documentation.
- Wave 2: 5–6 in parallel after Wave 1 — three-host parity and resumable K7 runner; 7 follows 6 to execute full matrices.
- Wave 3: 8–10 — release contract after parity, docs after matrices/release, final seal after all work.
- Wave 4: F1–F4 in parallel after Todo 10.

### Dependency matrix
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| 1 | — | 5, 6, 10 | 2, 3, 4 |
| 2 | — | 8, 10 | 1, 3, 4 |
| 3 | — | 5, 8, 10 | 1, 2, 4 |
| 4 | — | 9, 10 | 1, 2, 3 |
| 5 | 1, 3 | 8, 10 | 6 |
| 6 | 1 | 7, 9, 10 | 5 |
| 7 | 6 | 9, 10 | — |
| 8 | 2, 3, 5 | 9, 10 | — |
| 9 | 4, 7, 8 | 10 | — |
| 10 | 1-9 | F1-F4 | — |

## Todos
> Implementation + Test = ONE todo. Never separate.
<!-- APPEND TASK BATCHES BELOW THIS LINE WITH edit/apply_patch - never rewrite the headers above. -->
- [x] 1. Reconcile prior review/evidence debt and create a clean verification revision
  What to do / Must NOT do: Inventory commit series and outstanding status; independently review tasks 31/32/35 and add task-36 acceptance receipt; run only missing native/workspace gates. Create a clean worktree at pinned revision. Do not wholesale re-verify old waves or overwrite user work.
  Parallelization: Wave 1 | Blocked by: — | Blocks: 5,6,10
  References: `.omo/plans/company-information-npc-intentions.md:546-679`; `AGENTS.md`; `.omo/evidence/company-information-npc-intentions/task-{31,32,35,36}-*`; notepads.
  Acceptance criteria: independent receipt artifacts exist; `cargo test --workspace` passes from pinned clean worktree; status is clean and revision recorded.
  QA scenarios: happy `git worktree add <path> <commit> && cargo test --workspace`; failure missing/stale receipt makes verifier nonzero. Evidence `.omo/evidence/resolve-blockers-wayland/task-1-reconciliation.md`.
  Commit: Y | `chore(verification): reconcile prior completion evidence`

- [x] 2. Make strict Corepack web and clippy gates reproducible
  What to do / Must NOT do: Canonicalize `corepack pnpm` invocation and minimally fix existing clippy debt, beginning with failing behavior tests. Do not globally install pnpm, suppress warnings, or change domain behavior.
  Parallelization: Wave 1 | Blocked by: — | Blocks: 8,10
  References: `package.json:19`; `.github/workflows/ci.yml`; `packages/engine/src/behavior/decision.rs:239`; notepad clippy entries.
  Acceptance criteria: from clean worktree, `corepack pnpm --filter web test`, `corepack pnpm lint`, typecheck, and build exit zero.
  QA scenarios: happy with PATH lacking pnpm; failure bare pnpm emits actionable error. Evidence `.omo/evidence/resolve-blockers-wayland/task-2-toolchain.txt`.
  Commit: Y | `fix(tooling): make strict web gates reproducible`

- [x] 3. Make Weston Wayland native Tauri capture and IPC verification pass
  What to do / Must NOT do: Run bounded 2×2 renderer/dimension probe, use pixman only if confirmed, choose output ≥1280×800, update README command, launch real Tauri with Wayland, validate screenshot dimensions/pixels/app region, run actor/IPC diagnostics, and clean all processes. Use Weston VNC/RDP fallback after a bounded screenshooter failure; Xvfb cannot pass this task.
  Parallelization: Wave 1 | Blocked by: — | Blocks: 5,8,10
  References: `README.md:92-121`; `task-35-{weston,wayland-info,wayland-desktop,failure}.*`; `apps/desktop/src-tauri/{src/actor.rs,src/lib.rs,Cargo.toml}`.
  Acceptance criteria: `wayland-info` reports current ≥1280×800; decoded PNG matches dimensions and nontrivial pixels/app window; feature/default actor tests and stale generation/no-record tests pass under Weston; no remaining compositor/app process.
  QA scenarios: `weston --backend=headless-backend.so --renderer=pixman --width=1280 --height=800 --socket=rbw-wayland`; `GDK_BACKEND=wayland WAYLAND_DISPLAY=rbw-wayland ...`; failure black/no-op capture fails PNG validator. Evidence `.omo/evidence/resolve-blockers-wayland/task-3-wayland.*`.
  Commit: Y | `test(desktop): verify native Wayland headless rendering`

- [x] 4. Document named scale/body-limit and platform boundary findings
  What to do / Must NOT do: Give Task 39's 8 MiB remote-body consequence, verified Wayland scope, and external primary-source limitations a named documentation home. Do not invent external-market calibration or raise limits.
  Parallelization: Wave 1 | Blocked by: — | Blocks: 9,10
  References: `task-39-happy.txt`; `docs/{diagnostics.md,testing.md,trading-rules.md}`; README; problems.
  Acceptance criteria: Markdown claim/link checker finds named 8 MiB and Wayland-scope statements; no unsupported success claim.
  QA scenarios: compare commands/claims with artifact outputs; injected stale claim fails doc contract. Evidence `.omo/evidence/resolve-blockers-wayland/task-4-doc-contract.txt`.
  Commit: Y | `docs: record deployment and Wayland limits`

- [x] 5. Add and execute real WASM/Server/Tauri host-parity matrix
  What to do / Must NOT do: Implement actual isolated drivers and `scripts/simulation/host-parity.mjs` for generated WASM worker, authenticated Server HTTP/WS, and Weston Tauri IPC. Cover normal/closed day, no-trade disclosure, sequence errors, corrupt atomic restore, reload, concurrency and feature/release diagnostics. Do not replace hosts with direct engine calls or stale wasm.
  Parallelization: Wave 2 | Blocked by: 1,3 | Blocks: 8,10
  References: parent plan task 37; `apps/{web-wasm,server,desktop}`; `apps/web/src/host/{wasm-host,remote-host,tauri-host,event-buffer}.ts`.
  Acceptance criteria: normalized public state/event sequence/restore hash matches across real hosts; malformed cases reject by contract; JSON artifacts include host hashes/capability differences.
  QA scenarios: `node scripts/simulation/host-parity.mjs --output ...`; live curl/WS and Weston IPC; corrupt restore preserves pre-state hash. Evidence `.omo/evidence/resolve-blockers-wayland/task-5-parity/`.
  Commit: Y | `test(hosts): add real three-host parity matrix`

- [x] 6. Make fresh K7 baseline runner resumable and bounded
  What to do / Must NOT do: Add exact-spec atomic per-seed checkpoints, digest/source revision validation, bounded batch/resume CLI, deterministic rerun, and validated reuse of redundant 1× sensitivity artifact. Preserve all mandatory seed/multiplier/raw value/quantile/extreme/no-sample semantics. Do not shorten matrix or accept old saves.
  Parallelization: Wave 2 | Blocked by: 1 | Blocks: 7,9,10
  References: `scripts/simulation/{baseline-run.mjs,baseline-run.test.mjs}`; `packages/engine/examples/k7_baseline_fixture.rs`; parent plan task 38; partial seed artifact.
  Acceptance criteria: tests reject stale/corrupt/mismatched checkpoints; kill/resume reuses completed seed only with matching SHA; incomplete matrix cannot finalize; fresh provenance has no save path.
  QA scenarios: `node --test scripts/simulation/baseline-run.test.mjs`; injected subprocess kill/resume; old-save/duplicate seed failure. Evidence `.omo/evidence/resolve-blockers-wayland/task-6-resume.txt`.
  Commit: Y | `feat(simulation): make K7 runs resumable`

- [~] 7. Execute and seal all mandated fresh K7 after/sensitivity matrices (blocked pending execution of the approved escrow-parallel-engine plan at `.omo/plans/escrow-parallel-engine.md`; concurrency semantics selected there supersede the old coordinator draft)
  What to do / Must NOT do: Execute full primary 10×30-day, cross-year 5×400-natural-day, and behavior/event/C01 0.5/1/2 matrices via Todo 6. Retain raw canonical per-seed metrics, quantiles/extrema/no-sample and error events. Do not discard zeros, claim C06, or reduce matrix.
  Parallelization: Wave 2 | Blocked by: 6 | Blocks: 9,10
  References: parent task 38; task-1 before evidence; Task 36 diagnostics; Task 39 measurement.
  Acceptance criteria: validated manifests contain exact seed sets, fresh source, canonical repeat digest, calendar accounting, resource/disk/time facts, and explicit incomplete C06.
  QA scenarios: `node scripts/simulation/baseline-run.mjs after --output ...`; `... sensitivity --output ...`; incomplete resume fails manifest validation. Evidence `.omo/evidence/resolve-blockers-wayland/task-7-{after,sensitivity}.txt`.
  Commit: Y | `test(simulation): seal fresh K7 matrices`

- [x] 8. Add and run all-surface release-contract verifier
  What to do / Must NOT do: Build fresh default release surfaces and add `scripts/simulation/release-contract.mjs`; test Web, Server and Weston Tauri for no diagnostic collector/chunk/command/records and explicit no-data unsupported response. Do not use grep-only proof or feature wasm as release wasm.
  Parallelization: Wave 3 | Blocked by: 2,3,5 | Blocks: 9,10
  References: parent task 40; `apps/web/{inspector.html,src/dev*,wasm-pkg}`; diagnostics host adapters; Task 35 evidence.
  Acceptance criteria: verifier probes real artifacts/requests and fails injected diagnostics artifact; digests show regenerated normal wasm; release requests return unsupported with no fields.
  QA scenarios: `node scripts/simulation/release-contract.mjs --output ...`; curl and Weston IPC debug query. Evidence `.omo/evidence/resolve-blockers-wayland/task-8-release/`.
  Commit: Y | `test(release): verify diagnostics isolation`

- [~] 9. Synchronize only verified platform/simulation/release documentation (blocked: Todo 7's source-bound full K7 matrix and final manifest are not sealed; documentation may not claim unverified results)
  What to do / Must NOT do: Update docs after artifacts exist, correct stale lunch-clock text, declare Wayland-first scope, body limit, C06 incompleteness and release absence. Do not claim anything without artifact backing.
  Parallelization: Wave 3 | Blocked by: 4,7,8 | Blocks: 10
  References: parent task 41; `docs/causal-diagnostics.md:27`; Task 3/7/8 evidence; docs architecture/trading/testing/diagnostics.
  Acceptance criteria: documentation contract verifies final claims/commands, old clock sentence absent, Markdown links pass.
  QA scenarios: doc verifier plus deliberate old phrase search. Evidence `.omo/evidence/resolve-blockers-wayland/task-9-docs.txt`.
  Commit: Y | `docs: synchronize verified boundaries`

- [~] 10. Seal final clean-checkout regression and evidence manifest (blocked: Todo 7's fresh source-bound K7 manifests and Todo 9's verified final documentation are incomplete; a final clean seal must fail rather than omit them)
  What to do / Must NOT do: Implement `scripts/simulation/verify-plan.mjs`, make clean worktree at pinned commit, rebuild generated artifacts, validate all manifests/receipts/digests, run CI-equivalent Cargo/Corepack Web/ignored scale/Weston gates. Do not accept stale target/wasm, missing artifact, empty redirected output or skipped stress test.
  Parallelization: Wave 3 | Blocked by: 1-9 | Blocks: F1-F4
  References: parent task 42; CI; package scripts; all task evidence roots.
  Acceptance criteria: machine JSON includes commit, command exits, evidence digest inventory and exclusions; corrupted/missing artifact makes verifier nonzero; all required gates pass clean.
  QA scenarios: `node scripts/simulation/verify-plan.mjs --commit <hash> --output ...`; delete/corrupt copy artifact and observe failure. Evidence `.omo/evidence/resolve-blockers-wayland/task-10-final-seal.*`.
  Commit: Y | `test(verification): seal blocker-resolution evidence`

## Final verification wave
> Runs in parallel after ALL todos. ALL must APPROVE. Surface results and wait for the user's explicit okay before declaring complete.
- [~] F1. Plan compliance audit (blocked by incomplete Todo 7/9/10 evidence; audit must reject incomplete matrices rather than approve)
  Independently inspect every acceptance item, parent-plan debt, receipts and manifests; reject checkbox-only or incomplete-matrix claims.
- [~] F2. Code quality and A-share semantic review (blocked by the unimplemented entity-FIFO engine change and final clean-seal diff)
  Independently inspect changed code/docs for strict boundaries, clean clippy, feature isolation, resource-limit honesty and no A-share semantic drift.
- [~] F3. Real manual QA (blocked by the final rebuilt artifacts and evidence manifest required from Todo 10)
  Independently operate actual Weston Tauri, Server and Web flows; inspect pixel-valid native screenshots, IPC/HTTP/WS, report/save/restore/release flows and X11 fallback separately.
- [~] F4. Scope fidelity and evidence audit (blocked by Todo 7's unsealed fresh matrices and Todo 10 provenance manifest)
  Independently verify commit/digest provenance, rebuilt wasm, no fake screenshot, full seed matrix/resume, and documented C06/8 MiB limitations.

## Commit strategy
- Every Todo begins with red proof and ends with independent review before the named atomic Conventional Commit.
- Todo 1 preserves existing commits and performs reconciliation, not history rewrite. No push or PR is part of this plan.
- Large raw output is committed only after explicit size/privacy review; partial traces and incomplete runs remain precisely ignored.

## Success criteria
- Weston Wayland native capture is ≥1280 wide, pixel-valid, linked to real Tauri runtime/IPC, and cleanup leaves no compositor/application process.
- All current blockers are resolved with reproducible evidence or explicitly retained only as primary-source limitations.
- Clean pinned worktree passes strict CI-equivalent gates and produces final machine manifest.
- F1–F4 independently APPROVE.
