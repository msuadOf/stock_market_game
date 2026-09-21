import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { describe, it } from "node:test";
import { escrowSourceManifest } from "./escrow-source-manifest.mjs";

import {
  assembleTask9Evidence,
  validatePerformanceReport,
  runTask9Matrix,
  sha256Hex,
} from "./run-escrow-task9-matrix.mjs";

async function fixture() {
  const processTemp = process.env.TMPDIR;
  assert.ok(processTemp, "tests require the registered workspace-local TMPDIR");
  const root = await mkdtemp(path.join(processTemp, "task9-matrix-runner-test-"));
  const workspaceRoot = path.join(root, "workspace");
  const tempRoot = path.join(workspaceRoot, ".tmp");
  const sourceRoot = path.join(workspaceRoot, ".worktree", "source");
  const targetDir = path.join(tempRoot, "build-cache", "runner");
  const childTemp = path.join(tempRoot, "process-tmp", "runner");
  const logsDir = path.join(tempRoot, "runner", "logs");
  const outputRoot = path.join(tempRoot, "runner", "evidence");
  await Promise.all([
    mkdir(sourceRoot, { recursive: true }),
    mkdir(targetDir, { recursive: true }),
    mkdir(childTemp, { recursive: true }),
    mkdir(logsDir, { recursive: true }),
  ]);
  return {
    config: {
      workspaceRoot,
      sourceRoot,
      outputRoot,
      logsDir,
      targetDir,
      processTemp: childTemp,
      seed: "17",
      sourceFingerprint: (await escrowSourceManifest(sourceRoot)).sha256,
    },
  };
}

function fakeHarness({ status = "PASS", exitCode = 0, drift = false, determinismDrift = false } = {}) {
  const calls = [];
  const invocations = [];
  const runChild = async ({ command, args, entry, output, env }) => {
    calls.push(entry);
    invocations.push({ command, args, output });
    assert.equal(env.TMPDIR, env.TMP);
    assert.equal(env.TMP, env.TEMP);
    assert.ok(env.TMPDIR.includes(`${path.sep}.tmp${path.sep}`));
    await assert.rejects(readFile(path.join(output, "capture.json")));
    await mkdir(output);
    const negative = entry.mode === "negative-control";
    const values = {
      authoritative_state: Buffer.from(negative ? `negative-${entry.disabledMerge}` : "state"),
      event_stream: Buffer.from(negative
        ? `events-${entry.disabledMerge}`
        : determinismDrift && entry.id === "budget-2-repeat-0-canonical" ? "different-events" : "events"),
      receipts: Buffer.from(negative ? `receipts-${entry.disabledMerge}` : "receipts"),
      save_slot: Buffer.from("save"),
    };
    const artifacts = {};
    for (const [name, bytes] of Object.entries(values)) {
      const file = `${name.replaceAll("_", "-")}.json`;
      await writeFile(path.join(output, file), bytes);
      artifacts[name] = {
        file,
        receipt: {
          byte_length: String(bytes.length),
          sha256: sha256Hex(bytes),
        },
      };
    }
    if (drift && entry.id === "budget-2-repeat-0-canonical") {
      await writeFile(path.join(output, artifacts.event_stream.file), "tampered");
    }
    const capture = {
      schema: "escrow-runtime-evidence-capture-v1",
      status,
      configuration: {
        scenario: "task9-runtime-v1",
        seed: entry.seed,
        budget: entry.budget,
        repeat: entry.repeat,
        mode: entry.mode,
        requested_scheduler_merge_disabled: entry.disabledMerge,
      },
      artifacts,
      scheduler_precanonical_order: { available: true, records: [
        ["P3AccountShards", ["1", "2"]], ["P4AuctionStockShards", ["000812", "600101"]],
        ["P5ReceiptResults", ["receipt-1", "receipt-2"]],
      ].map(([boundary, identities]) => ({ boundary, identities: entry.mode === "canonical" ? identities : [...identities].reverse(), item_counts: [1, 1] })) },
      runtime_coverage: { tick_from: "1", tick_to: "1" },
      producer_readiness: { conservation_snapshots: [{
        schema: "escrow-conservation-snapshot-v1", scenario: entry.scenario, seed: entry.seed, tick: "1", envelopes: [],
        accounts: [{ account_id: "0", cash_cents: "10", positions: [], aggregate: {
          left: { cash_cents: "0", shares: "0" }, right: { cash_cents: "0", shares: "0" },
        } }],
      }] },
      negative_control: negative ? { detected: true, kind: "typed-rejection-with-rollback", disabled_merge: entry.disabledMerge,
        enabled_merge_same_step_passed: true, failed_step_published_events: "0", error: "expected invariant failure",
        before_failed_step: { sha256: sha256Hex("before"), byte_length: "6" },
        after_failed_step: { sha256: sha256Hex("before"), byte_length: "6" },
      } : null,
    };
    await writeFile(path.join(output, "capture.json"), JSON.stringify(capture));
    const captureBytes = await readFile(path.join(output, "capture.json"));
    await writeFile(path.join(output, "capture-receipt.json"), JSON.stringify({
      schema: "escrow-capture-receipt-v1", file: "capture.json",
      sha256: sha256Hex(captureBytes), byte_length: String(captureBytes.length),
    }));
    return {
      code: exitCode,
      signal: null,
      stdout: JSON.stringify({ status, capture: path.join(output, "capture.json") }),
      stderr: "",
    };
  };
  return { calls, invocations, runChild };
}

function validPerformanceReport() {
  const workload = { scenario: "scenario", seed: "1", setup_manifest: { schema: "fixture" }, completed_ticks: 10, repetitions: 1, profile: "release", features: ["verification-harness"] };
  const environment_contract = { cargo: "cargo", rustc: "rustc", target: "target", rustflags: "", cargo_jobs: 1, rayon_threads: 1 };
  const sample = (after) => ({
    wall_ns: "100", peak_process_tree_rss_bytes: 10, completed_ticks: 10, workload, environment_contract,
    ticks_per_second: 100_000_000,
    phase_wall_ns: after ? Object.fromEntries(Array.from({ length: 10 }, (_, i) => [`P${i}`, "1"])) : null,
    process_tree_thread_state: {
      schema: "linux-process-tree-thread-state-v1", sampled_state: "R (running or runnable)", sample_interval_ms: 1, sample_count: 1,
      process_count: { minimum: 1, maximum: 1 }, total_threads: { minimum: 1, maximum: 1 },
      runnable_threads: { minimum: 1, maximum: 1, mean: 1, histogram: { 1: 1 } },
    },
    rayon_registry_capacity_samples: after ? [1] : null, stderr: "",
  });
  const aggregate = { sample_count: 1, ticks_per_second: { minimum: 100_000_000, maximum: 100_000_000, mean: 100_000_000 }, peak_process_tree_rss_bytes: { minimum: 10, maximum: 10, mean: 10 } };
  return {
    schema: "escrow-perf-report-v3", status: "PASS", generated_at: "2026-01-01T00:00:00.000Z", workload, environment_contract,
    environment_manifest: { fixture: true },
    measurement_contract: {
      same_machine_for_both_sides: true, same_workload_for_both_sides: true,
      comparison_key_fields: ["scenario", "seed", "setup_manifest", "completed_ticks", "repetitions", "profile", "features", "environment_contract"],
      rss_scope: "Linux process tree rooted at the configured executable",
      runnable_thread_source: "Linux /proc process-tree task state R (running or runnable)",
      rayon_registry_capacity_is_not_worker_activity: true, throughput_source: "completed_ticks / measured wall time",
      cpu_utilization_used_as_throughput: false, preset_performance_threshold: null, warmup_runs: 0, sample_count: 1,
      alternating_measurement_order: true, source_manifest_verified_before_and_after_every_invocation: true,
    },
    before: { role: "baseline", source_fingerprint: "a".repeat(64), command: ["before"], cwd: "/workspace", samples: [sample(false)], aggregate: structuredClone(aggregate) },
    after: { role: "new-engine", source_fingerprint: "a".repeat(64), command: ["after"], cwd: "/workspace", samples: [sample(true)], aggregate: structuredClone(aggregate) },
    comparison: { conditions_match: true, throughput_mean_ratio_after_over_before: 1, peak_rss_mean_ratio_after_over_before: 1 },
  };
}

describe("Task 9 matrix runner", () => {
  it("rejects a forged PASS performance sample whose throughput is not derived from wall time", () => {
    const workload = { scenario: "s", seed: "1", setup_manifest: { schema: "s" }, completed_ticks: 10, repetitions: 1, profile: "release", features: [] };
    const environment_contract = { cargo: "cargo", rustc: "rustc", target: "target", rustflags: "", cargo_jobs: 1, rayon_threads: 1 };
    const sample = (after) => ({ wall_ns: "100", peak_process_tree_rss_bytes: 10, completed_ticks: 10, workload,
      environment_contract, ticks_per_second: 100_000_000, phase_wall_ns: after ? Object.fromEntries(Array.from({ length: 10 }, (_, i) => [`P${i}`, "1"])) : null,
      process_tree_thread_state: { schema: "linux-process-tree-thread-state-v1", sampled_state: "R (running or runnable)", sample_interval_ms: 1, sample_count: 1, process_count: { minimum: 1, maximum: 1 }, total_threads: { minimum: 1, maximum: 1 }, runnable_threads: { minimum: 1, maximum: 1, mean: 1, histogram: { 1: 1 } } }, rayon_registry_capacity_samples: after ? [1] : null, stderr: "" });
    const report = { schema: "escrow-perf-report-v3", status: "PASS", generated_at: "2026-01-01T00:00:00.000Z", workload, environment_contract,
      environment_manifest: { fixture: true }, measurement_contract: { same_machine_for_both_sides: true, same_workload_for_both_sides: true,
        comparison_key_fields: ["scenario", "seed", "setup_manifest", "completed_ticks", "repetitions", "profile", "features", "environment_contract"],
        rss_scope: "Linux process tree rooted at the configured executable",
        runnable_thread_source: "Linux /proc process-tree task state R (running or runnable)", rayon_registry_capacity_is_not_worker_activity: true,
        throughput_source: "completed_ticks / measured wall time", cpu_utilization_used_as_throughput: false, preset_performance_threshold: null,
        warmup_runs: 0, sample_count: 1, alternating_measurement_order: true, source_manifest_verified_before_and_after_every_invocation: true },
      before: { role: "baseline", source_fingerprint: "a".repeat(64), command: ["before"], cwd: "/workspace", samples: [sample(false)], aggregate: { sample_count: 1, ticks_per_second: { minimum: 100_000_000, maximum: 100_000_000, mean: 100_000_000 }, peak_process_tree_rss_bytes: { minimum: 10, maximum: 10, mean: 10 } } },
      after: { role: "new-engine", source_fingerprint: "a".repeat(64), command: ["after"], cwd: "/workspace", samples: [sample(true)], aggregate: { sample_count: 1, ticks_per_second: { minimum: 100_000_000, maximum: 100_000_000, mean: 100_000_000 }, peak_process_tree_rss_bytes: { minimum: 10, maximum: 10, mean: 10 } } },
      comparison: { conditions_match: true, throughput_mean_ratio_after_over_before: 1, peak_rss_mean_ratio_after_over_before: 1 } };
    report.after.samples[0].ticks_per_second = 1;
    assert.throws(() => validatePerformanceReport(report), /throughput/);
  });

  it("rejects forged nested PASS report fields without relying on throughput changes", () => {
    const histogram = validPerformanceReport();
    histogram.after.samples[0].process_tree_thread_state.runnable_threads.histogram = { 1: 2 };
    assert.throws(() => validatePerformanceReport(histogram), /histogram/);

    const blocked = validPerformanceReport();
    blocked.before.samples[0].stderr = "worker BLOCKED on lock";
    assert.throws(() => validatePerformanceReport(blocked), /BLOCKED/);

    const duplicateFeature = validPerformanceReport();
    duplicateFeature.workload.features.push("verification-harness");
    duplicateFeature.before.samples[0].workload.features.push("verification-harness");
    duplicateFeature.after.samples[0].workload.features.push("verification-harness");
    assert.throws(() => validatePerformanceReport(duplicateFeature), /duplicate/);

    const malformedEnvironment = validPerformanceReport();
    malformedEnvironment.environment_contract.cargo_jobs = 0;
    malformedEnvironment.before.samples[0].environment_contract.cargo_jobs = 0;
    malformedEnvironment.after.samples[0].environment_contract.cargo_jobs = 0;
    assert.throws(() => validatePerformanceReport(malformedEnvironment), /cargo_jobs/);

    const sourceDrift = validPerformanceReport();
    sourceDrift.after.source_fingerprint = "b".repeat(64);
    assert.throws(() => validatePerformanceReport(sourceDrift, "a".repeat(64)), /frozen matrix source/);
  });

  it("fails closed when the complete corpus/perf/bundle evidence is not contract-valid", async () => {
    const { config } = await fixture();
    const inputRoot = path.join(config.workspaceRoot, "inputs");
    await mkdir(inputRoot, { recursive: true });
    await writeFile(path.join(inputRoot, "corpus.json"), JSON.stringify({ schema: "task-9-corpus-diff-v1" }));
    await writeFile(path.join(inputRoot, "perf.json"), JSON.stringify({ schema: "escrow-perf-report-v3", status: "PASS" }));
    await writeFile(path.join(inputRoot, "bundle.json"), JSON.stringify({ schema: "escrow-verification-bundle-v1" }));
    await mkdir(config.outputRoot, { recursive: true });
    await assert.rejects(
      assembleTask9Evidence({
        workspaceRoot: config.workspaceRoot,
        outputRoot: config.outputRoot,
        evidence: {
          corpusDiffPath: path.join(inputRoot, "corpus.json"),
          perfReportPath: path.join(inputRoot, "perf.json"),
          verificationBundlePath: path.join(inputRoot, "bundle.json"),
        },
      }),
      /verification bundle does not satisfy Task 9 contracts/,
    );
  });

  it("rejects complete evidence inputs outside the workspace before reading them", async () => {
    const { config } = await fixture();
    await mkdir(config.outputRoot, { recursive: true });
    await assert.rejects(
      assembleTask9Evidence({
        workspaceRoot: config.workspaceRoot,
        outputRoot: config.outputRoot,
        evidence: {
          corpusDiffPath: path.join(config.workspaceRoot, "corpus.json"),
          perfReportPath: "/tmp/perf-report.json",
          verificationBundlePath: path.join(config.workspaceRoot, "bundle.json"),
        },
      }),
      /perfReportPath must resolve below workspaceRoot/,
    );
  });

  it("accepts a source root equal to the workspace root", async () => {
    const { config } = await fixture();
    config.sourceRoot = config.workspaceRoot;
    const summary = await runTask9Matrix(config, { runChild: fakeHarness().runChild });
    assert.equal(summary.status, "PASS");
  });

  it("runs four budgets, two repeats, two modes and exactly three negative controls", async () => {
    const { config } = await fixture();
    const fake = fakeHarness();
    const summary = await runTask9Matrix(config, { runChild: fake.runChild });

    assert.equal(summary.status, "PASS");
    assert.equal(summary.entries.length, 19);
    assert.equal(fake.calls.filter((entry) => entry.mode === "negative-control").length, 3);
    assert.deepEqual(
      fake.calls.filter((entry) => entry.mode === "negative-control").map((entry) => entry.disabledMerge),
      ["account", "stock", "completion"],
    );
    assert.deepEqual(fake.invocations[0].args, [
      "run", "--quiet", "-p", "engine", "--features", "verification-harness", "--example", "escrow_verification_harness", "--",
      "--scenario", "task9-runtime-v1", "--seed", "17", "--budget", "1", "--repeat", "0",
      "--mode", "canonical", "--output", fake.invocations[0].output,
    ]);
    assert.deepEqual(fake.invocations.at(-1).args.slice(-6), [
      "--mode", "negative-control", "--disable-merge", "completion", "--output", fake.invocations.at(-1).output,
    ]);
    const receipt = await readFile(path.join(config.outputRoot, "determinism.sha256"), "utf8");
    assert.equal(receipt.trim().split("\n").length, 19 * 4);
    assert.equal(JSON.parse(await readFile(path.join(config.outputRoot, "summary.json"), "utf8")).status, "PASS");
  });

  it("never promotes BLOCKED output or a nonzero process exit to PASS", async () => {
    const blocked = await fixture();
    const blockedHarness = fakeHarness({ status: "BLOCKED", exitCode: 3 });
    const blockedSummary = await runTask9Matrix(blocked.config, { runChild: blockedHarness.runChild });
    assert.equal(blockedSummary.status, "FAIL");
    assert.equal(blockedSummary.failure.code, "HARNESS_BLOCKED");

    const failed = await fixture();
    const failedHarness = fakeHarness({ status: "PASS", exitCode: 7 });
    const failedSummary = await runTask9Matrix(failed.config, { runChild: failedHarness.runChild });
    assert.equal(failedSummary.status, "FAIL");
    assert.equal(failedSummary.failure.code, "EXIT_STATUS_MISMATCH");
  });

  it("fails when an artifact no longer matches its declared byte length or SHA-256", async () => {
    const { config } = await fixture();
    const fake = fakeHarness({ drift: true });
    const summary = await runTask9Matrix(config, { runChild: fake.runChild });
    assert.equal(summary.status, "FAIL");
    assert.equal(summary.failure.code, "ARTIFACT_HASH_DRIFT");
  });

  it("fails when individually valid receipts drift across budget/repeat observations", async () => {
    const { config } = await fixture();
    const fake = fakeHarness({ determinismDrift: true });
    const summary = await runTask9Matrix(config, { runChild: fake.runChild });
    assert.equal(summary.status, "FAIL");
    assert.equal(summary.failure.code, "DETERMINISM_DRIFT");
  });

  it("reports a typed FAIL when the harness itself reports FAIL", async () => {
    const { config } = await fixture();
    const fake = fakeHarness({ status: "FAIL", exitCode: 1 });
    const summary = await runTask9Matrix(config, { runChild: fake.runChild });
    assert.equal(summary.status, "FAIL");
    assert.equal(summary.failure.code, "HARNESS_FAIL");
  });

  it("treats an identical duplicate notification as idempotent and does not rerun children", async () => {
    const { config } = await fixture();
    const first = fakeHarness();
    assert.equal((await runTask9Matrix(config, { runChild: first.runChild })).status, "PASS");

    let duplicateCalls = 0;
    const duplicate = await runTask9Matrix(config, {
      runChild: async () => {
        duplicateCalls += 1;
        throw new Error("duplicate notification must not rerun a completed matrix");
      },
    });
    assert.equal(duplicate.status, "PASS");
    assert.equal(duplicate.reused, true);
    assert.equal(duplicateCalls, 0);
  });

  it("refuses to rerun a duplicate whose completed evidence was modified", async () => {
    const { config } = await fixture();
    const first = fakeHarness();
    assert.equal((await runTask9Matrix(config, { runChild: first.runChild })).status, "PASS");
    await writeFile(path.join(config.outputRoot, "runs", "budget-1-repeat-0-canonical", "receipts.json"), "modified");

    let duplicateCalls = 0;
    const duplicate = await runTask9Matrix(config, {
      runChild: async () => {
        duplicateCalls += 1;
        throw new Error("modified duplicate must fail closed without rerunning");
      },
    });
    assert.equal(duplicate.status, "FAIL");
    assert.equal(duplicate.failure.code, "ARTIFACT_HASH_DRIFT");
    assert.equal(duplicateCalls, 0);
  });

  it("refuses to reuse a capture whose own SHA-256 receipt no longer matches", async () => {
    const { config } = await fixture();
    const first = fakeHarness();
    assert.equal((await runTask9Matrix(config, { runChild: first.runChild })).status, "PASS");
    await writeFile(path.join(config.outputRoot, "runs", "budget-1-repeat-0-canonical", "capture.json"), "{}");
    const duplicate = await runTask9Matrix(config, { runChild: async () => { throw new Error("must not rerun"); } });
    assert.equal(duplicate.status, "FAIL");
    assert.equal(duplicate.failure.code, "CAPTURE_HASH_DRIFT");
  });

  it("refuses a duplicate whose persisted complete source manifest was modified", async () => {
    const { config } = await fixture();
    const first = fakeHarness();
    assert.equal((await runTask9Matrix(config, { runChild: first.runChild })).status, "PASS");
    const manifestPath = path.join(config.outputRoot, "source-manifest.json");
    const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
    manifest.absent.push("forged-input");
    await writeFile(manifestPath, JSON.stringify(manifest));

    let duplicateCalls = 0;
    const duplicate = await runTask9Matrix(config, {
      runChild: async () => {
        duplicateCalls += 1;
        throw new Error("tampered source binding must fail closed without rerunning");
      },
    });
    assert.equal(duplicate.status, "FAIL");
    assert.equal(duplicate.failure.code, "SOURCE_HASH_DRIFT");
    assert.equal(duplicateCalls, 0);
  });
});
