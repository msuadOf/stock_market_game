# Task 11 K7 execution log

Append-only run log. Times use Asia/Shanghai unless stated otherwise.

## 2026-09-22 — controlled-concurrency runner qualification

- Runner/verifier tests: 74 passed, 0 failed.
- Independent implementation-risk review: APPROVE. This is not the final whole-component review.
- Resource policy: `k7-resource-policy-v4`; 128 process-available CPUs; at most 8 seed
  processes; 16 Rayon threads per seed; aggregate Rayon budget 128.
- Aborted pre-policy after root and bounded probe root are explicitly invalid and are not reused or
  counted as Task 11 evidence.
- Probe and formal first wave both measured approximately 800% aggregate CPU, confirming real
  process-level parallelism without a 128-thread-per-child oversubscription.

Stable source identity before formal execution:

```text
digest: e203de944d6ba3fc1213b567765a534efecce0d26c179c856dab114b29335c24
files: 570
committed_tree: 0d0eb8924730e9e633c2907baeec2456a48528c5
dirty_patch_sha256: c0e2e6278a7b1ab07a98c937a95ca866a2b3649cf579b2679cd50f5396522562
```

Formal roots:

```text
after: /data1/baiyifan/workplace/stock_market_game/.worktree/escrow-main-integration/.tmp/k7-task11-formal-after-20260922T153651773767390
sensitivity: /data1/baiyifan/workplace/stock_market_game/.worktree/escrow-main-integration/.tmp/k7-task11-formal-sensitivity-20260922T153651773767390
```

Formal after command:

```sh
env TMPDIR=$PWD/.tmp/process-tmp/k7-task11-formal-20260922T153651773767390 TMP=$PWD/.tmp/process-tmp/k7-task11-formal-20260922T153651773767390 TEMP=$PWD/.tmp/process-tmp/k7-task11-formal-20260922T153651773767390 CARGO_TARGET_DIR=$PWD/.tmp/build-cache/validation-closeout-task10-release CARGO_BUILD_JOBS=16 CARGO_PROFILE_RELEASE_LTO=off CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 node scripts/simulation/baseline-run.mjs after --output $PWD/.tmp/k7-task11-formal-after-20260922T153651773767390 --max-threads auto
```

First-wave CPU sample:

```text
path: .tmp/task11-k7-formal-20260922T153651773767390/formal-after-first-wave-cpu.log
bytes: 10254
sha256: 8dbdd1d5f799496587158b8bb2bf63374747132c6ce18d16717c0b259ae28c97
pidstat 5-second per-process averages: 100.2, 100.2, 100.2, 100.2, 100.0, 100.4, 100.0, 100.2 percent
aggregate: 801.4 percent
```

Completion and independent root-verifier results must be appended only after the runner exits and
the corresponding command succeeds. This entry does not claim Task 11 completion.

## 2026-09-22 — v4 formal roots invalidated

The v4 formal `after` process was safely interrupted before any canonical artifact completed. Both
v4 formal roots are invalid and must not be resumed, verified, or counted. The reason is resource
policy refinement: although the measured first wave sustained approximately 800% CPU, it left most
of the 128-CPU allocation idle and serialized independent matrices unnecessarily. The replacement
v5 policy uses one global child-execution pool and preserves the original cryptographic artifact
contracts.

## 2026-09-22 — v5 resource-policy qualification probe (invalid as formal evidence)

The following root was used only to qualify the v5 concurrency policy and is permanently invalid
as a Task 11 formal root:

```text
/data1/baiyifan/workplace/stock_market_game/.worktree/escrow-main-integration/.tmp/k7-v5-probe-20260922T155825948892365
```

It was launched as a bounded sensitivity probe with `--batch-size 30 --max-threads auto`. The v5
policy reported 128 process-available CPUs, `max_concurrent_child_executions: 30`, and four Rayon
threads per child, for an aggregate Rayon budget of 120. Two independent five-second `pidstat`
samples each observed 30 child processes doing real work:

```text
path: .tmp/task11-k7-v5-probe-20260922T155825948892365/v5-sensitivity-30-child-cpu.log
bytes: 36154
sha256: 0f63f516e4c29c2107e09cd7e7c82de53488c79738317d2bfd8beca8e68f0491
per-child average: approximately 99.6–100.0 percent CPU
aggregate average: 2994.67 percent CPU

path: .tmp/k7-v5-probe-20260922T155825948892365/pidstat-root-sample.log
bytes: 11331
sha256: 180d83150098a97b38695a4f5b0176c07463dc486eb811e69fdd2d33c82bfebf
per-child average: approximately 99.0–100.0 percent CPU
aggregate average: 2983.68 percent CPU
```

Each sampled child had five Linux threads (one main thread plus four Rayon workers). The probe was
allowed to run for approximately 80 seconds after sampling, then the Node parent PID 26432 received
SIGINT. The parent and all 30 children exited. No manifest or checkpoint was produced. This root is
qualification-only, must not be resumed, verified, or counted toward the required 94 formal
executions. Formal `after` and `sensitivity` runs must use two entirely fresh roots and must carry
their own in-run CPU samples.

## 2026-09-23 — v5 formal source freeze and `after` launch

The zero-execution `captureAfter({ batchSize: 0 })` freeze used this local scratch root only to
calculate source identity; it is not a formal evidence root:

```text
/data1/baiyifan/workplace/stock_market_game/.worktree/escrow-main-integration/.tmp/k7-v5-fingerprint-20260923T000305584274218
```

Frozen source identity:

```text
algorithm: k7-simulation-source-v1
digest: 8f2b3cb1e7e27cf035dc2228af81f26e3c79c5cd703eb151de3fab2630762971
files: 570
committed_tree: 0d0eb8924730e9e633c2907baeec2456a48528c5
dirty_patch_sha256: fc5eb4e7cede534ea1971582c45b500319ed50ad7a4e728a6a5c7aa4e8153690
```

Fresh formal roots reserved for the same source freeze:

```text
after: /data1/baiyifan/workplace/stock_market_game/.worktree/escrow-main-integration/.tmp/k7-task11-v5-formal-after-20260923T0005
sensitivity: /data1/baiyifan/workplace/stock_market_game/.worktree/escrow-main-integration/.tmp/k7-task11-v5-formal-sensitivity-20260923T0005
```

Formal `after` command (PID 41854):

```sh
env TMPDIR=$PWD/.tmp/process-tmp/k7-task11-v5-formal-20260923T0005 TMP=$PWD/.tmp/process-tmp/k7-task11-v5-formal-20260923T0005 TEMP=$PWD/.tmp/process-tmp/k7-task11-v5-formal-20260923T0005 CARGO_TARGET_DIR=$PWD/.tmp/build-cache/validation-closeout-task10-release CARGO_BUILD_JOBS=16 CARGO_PROFILE_RELEASE_LTO=off CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 node scripts/simulation/baseline-run.mjs after --output $PWD/.tmp/k7-task11-v5-formal-after-20260923T0005 --batch-size 17 --max-threads auto
```

Formal canonical-wave CPU evidence:

```text
path: .tmp/task11-k7-v5-formal-20260923T0005/formal-after-15-child-cpu.log
bytes: 7320
sha256: b78f47a6296dc61bc310adb079f8814ca9c26f65b133741a50ad7a0125519437
sample: 15 children, five one-second observations per child
per-child averages: 99.8–100.4 percent CPU
aggregate average: 1500.20 percent CPU
thread shape: each child NLWP=5 in the adjacent before/after process-tree captures
```

Another actor briefly launched
`.tmp/k7-task11-formal-v5-after-20260923T000400` before learning that PID 41854 was already in
flight. That duplicate was immediately interrupted and is permanently invalid: it must not be
resumed, verified, or counted. Only the `k7-task11-v5-formal-after-20260923T0005` root above is the
formal `after` run. Completion and verifier results remain pending and will be appended after the
runner exits successfully.

## 2026-09-23 — first formal `after` invocation hit the default watchdog

The first invocation of the formal `after` root exited 1 after its two-hour per-child watchdog.
The exact runner diagnostic was:

```text
基线采集失败：cargo run -p engine --release --features simulation-diagnostics --example k7_baseline_fixture -- primary 1 30 1 1 1 超过 7200000ms 被强制终止
```

All child processes were cleaned up. The formal root contained no manifest, no matrix checkpoint,
no seed checkpoint, and no raw artifact after the failure. The invocation is not counted as a
completed execution. The same formal root is continued only through the runner's explicit
`--resume` contract, with identical source, TMP, target, business arguments, execution budget, and
v5 resource policy. The only adjusted setting is
`BASELINE_FIXTURE_TIMEOUT_MS=21600000` (six-hour watchdog), because the original two-hour process
supervision limit was demonstrably shorter than this workload. This changes no business input or
artifact content.

The resumed invocation runs as PID 450964:

```sh
env BASELINE_FIXTURE_TIMEOUT_MS=21600000 TMPDIR=$PWD/.tmp/process-tmp/k7-task11-v5-formal-20260923T0005 TMP=$PWD/.tmp/process-tmp/k7-task11-v5-formal-20260923T0005 TEMP=$PWD/.tmp/process-tmp/k7-task11-v5-formal-20260923T0005 CARGO_TARGET_DIR=$PWD/.tmp/build-cache/validation-closeout-task10-release CARGO_BUILD_JOBS=16 CARGO_PROFILE_RELEASE_LTO=off CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 node scripts/simulation/baseline-run.mjs after --resume --output $PWD/.tmp/k7-task11-v5-formal-after-20260923T0005 --batch-size 17 --max-threads auto
```

Fresh CPU evidence from the resumed invocation:

```text
path: .tmp/task11-k7-v5-formal-20260923T0005/formal-after-resume-6h-15-child-cpu.log
bytes: 7320
sha256: a31cacc88cc47bef08511a01e1b2a23b5d33a0e81844aede4e252ef40b8884f4
sample: 15 children, five one-second observations per child
per-child averages: 99.8–100.2 percent CPU
aggregate average: 1500.40 percent CPU
thread shape: each child NLWP=5 in the adjacent before/after process-tree captures
```

## 2026-09-23 — user time-limit ruling permanently invalidates v5 long runs

The user replaced the prior watchdog policy: ordinary commands/cases target at most 10 seconds;
necessary long validation has a hard 300000ms limit for both each child and the entire batch wall
clock, and every parallelizable run must use real multi-process/multi-core execution. The earlier v5
two-hour invocation and six-hour resume were both interrupted with SIGINT, have no live processes,
and are permanently invalid. They must not be resumed, verified, or counted. Their partial roots
remain only as failure history.

## 2026-09-23 — v6 bounded representative qualification

The replacement runner keeps the complete 10-seed primary matrix, five-seed cross-year matrix,
seven unique sensitivity configurations and one deterministic rerun per matrix (17 after + 77
sensitivity = 94 executions), while bounding each real run to a representative profile:

```text
primary: 5 natural days; 64 retail; 30 ticks/day; opening auction 3; closing auction 2
cross-year: 8 natural days; 32 retail; 20 ticks/day; opening auction 3; closing auction 2;
            start 2030-12-27
both: five stocks; institutional and hot-money NPCs retained; opening/continuous/closing phases
scope marker: bounded_representative_not_full_market_scale
```

The Node runner plus independent verifier suite passed 78/78 in 1.90 seconds. The Rust fixture
test binary passed 2/2 in 0.00 seconds after a separate 16-job build (31.65 seconds); the build is
not reported as test execution time. A real primary seed-1 probe completed in 11.47 seconds with
RSS 81884 KiB, so it is classified as necessary long validation rather than an ordinary test.

The v6 resource policy detected 128 available CPUs and selected 30 concurrent child executions,
four Rayon threads per child (aggregate Rayon budget 120), with ordinary/child/batch limits
10000/300000/300000ms. The runner uses one shared absolute deadline and abort signal across child
execution, result validation, finalization, and manifest publication.

## 2026-09-23 — v6 formal `after` root failed and is permanently invalid

Fresh root (never resume or count):

```text
/data1/baiyifan/workplace/stock_market_game/.worktree/escrow-main-integration/.tmp/k7-task11-v6-formal-after-20260923T0306CST
```

Exact command:

```sh
env TMPDIR=$PWD/.tmp/process-tmp/k7-formal-v6 TMP=$PWD/.tmp/process-tmp/k7-formal-v6 TEMP=$PWD/.tmp/process-tmp/k7-formal-v6 CARGO_TARGET_DIR=$PWD/.tmp/build-cache/validation-closeout-task10-release CARGO_BUILD_JOBS=16 CARGO_PROFILE_RELEASE_LTO=off CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 timeout 300s /usr/bin/time -f 'AFTER_WALL_SECONDS=%e MAX_RSS_KB=%M' node scripts/simulation/baseline-run.mjs after --output $PWD/.tmp/k7-task11-v6-formal-after-20260923T0306CST --max-threads auto
```

The run exited 1 after 25.68 seconds and produced no manifest. Its source fingerprint digest was
`2e50225918a58ff2a67a66aa7f779139a93d0a44bf0896c7475bd1d84999d65a` and the failure was:

```text
primary seed 7: packages/engine/src/session/decision_chain.rs:1049:45
allocation failed for AccountId(65): plan PlanId(0) fee reserve 236019090512 exceeds available cash 218852149730
```

The root contains 12 raw artifacts plus their 12 per-seed receipts, but no root manifest; partial
artifacts do not count. In-run process sampling observed 12 simultaneous fixture processes at the
sample instant, each with five threads and approximately 82.7–84.8% instantaneous CPU, proving
real multi-process execution. Log digests:

```text
runner:  d4b9b09960ae799d2384fa7697ce0fe3ff9ef368e42e76dac37f16990459ad1b
process: 7d5402a87894d362f8ee433e925b4813be1231701ba20e6b3e1780faf8c445ef
```

The same unmodified representative fixture was reproduced independently with four Rayon threads:

```sh
env RAYON_NUM_THREADS=4 TMPDIR=$PWD/.tmp/process-tmp/validation-closeout timeout 300s .tmp/build-cache/validation-closeout-task10-release/release/examples/k7_baseline_fixture primary 7 5 1 1 1
```

It exited 101 with the identical panic in 0.73 seconds (stderr SHA-256
`de597b6f875bf43e6fd5585f9ee207290538a1ac898425f0175893d411c0a0b9`). No seed, cash,
strategy-size, or fixture input was changed to evade the failure. Task 11 remains incomplete until
the core defect is fixed and both formal suites pass from entirely new source-fingerprinted roots.
