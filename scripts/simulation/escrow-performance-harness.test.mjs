import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { after, describe, it } from "node:test";

import {
  main,
  reportPathForOutputDirectory,
  runPerformanceHarness,
  runProcessSample,
  validateMeasuredSample,
  validatePerformanceConfig,
  writeReportIdempotently,
} from "./escrow-performance-harness.mjs";

const BEFORE_FINGERPRINT = "a".repeat(64);
const AFTER_FINGERPRINT = "b".repeat(64);
const SETUP_FINGERPRINT = "c".repeat(64);
const temporaryDirectories = [];

after(async () => {
  await Promise.all(temporaryDirectories.map((directory) => rm(directory, { force: true, recursive: true })));
});

async function newTemporaryDirectory() {
  assert.ok(path.isAbsolute(process.env.TMPDIR ?? ""), "TMPDIR must be an absolute workspace path");
  const directory = await mkdtemp(path.join(process.env.TMPDIR, "perf-runner-test-"));
  temporaryDirectories.push(directory);
  return directory;
}

function config() {
  return {
    schema: "escrow-perf-config-v2",
    workload: {
      scenario: "fixed-multi-stock",
      seed: "7",
      setup_manifest: { schema: "fixture-setup-v1", sha256: SETUP_FINGERPRINT },
      completed_ticks: 1000,
      repetitions: 3,
      profile: "release",
      features: ["simulation-diagnostics"],
    },
    environment_contract: { cargo: "cargo 1.96.0", rustc: "rustc 1.96.0", target: "x86_64-unknown-linux-gnu", rustflags: "", cargo_jobs: 4, rayon_threads: 4 },
    warmup_runs: 1,
    sample_count: 2,
    rss_sample_interval_ms: 10,
    before: { command: ["/fixture/before", "--seed", "7"], cwd: "/fixture/before-tree", source_fingerprint: BEFORE_FINGERPRINT },
    after: { command: ["/fixture/after", "--seed", "7"], cwd: "/fixture/after-tree", source_fingerprint: AFTER_FINGERPRINT },
  };
}

function measured(side, index = 0) {
  const isAfter = side === "after";
  return {
    wall_ns: String((isAfter ? 500_000_000 : 1_000_000_000) + index * 10_000_000),
    peak_process_tree_rss_bytes: (isAfter ? 120 : 100) * 1024 * 1024 + index,
    process_tree_thread_state: {
      schema: "linux-process-tree-thread-state-v1",
      sampled_state: "R (running or runnable)",
      sample_interval_ms: 10,
      sample_count: 4,
      process_count: { minimum: 1, maximum: 1 },
      total_threads: { minimum: 4, maximum: 4 },
      runnable_threads: { minimum: 1, maximum: 2, mean: 1.5, histogram: { 1: 2, 2: 2 } },
    },
    stdout: JSON.stringify({
      schema: "escrow-perf-sample-v2",
      status: "PASS",
      workload: config().workload,
      environment_contract: config().environment_contract,
      completed_ticks: 1000,
      source_fingerprint: isAfter ? AFTER_FINGERPRINT : BEFORE_FINGERPRINT,
      phase_wall_ns: isAfter ? Object.fromEntries(Array.from({ length: 10 }, (_, phase) => [`P${phase}`, String(50 + phase)])) : null,
      runnable_samples: isAfter ? [4, 4, 4, 4] : null,
    }),
    stderr: "",
  };
}

const sourceManifest = async (cwd) => ({
  sha256: cwd === "/fixture/before-tree" ? BEFORE_FINGERPRINT : AFTER_FINGERPRINT,
});

describe("performance harness", () => {
  it("records actual paired samples and ratios without a preset speed threshold", async () => {
    const calls = [];
    const report = await runPerformanceHarness(config(), {
      runSample: async (_endpoint, context) => {
        calls.push({ ...context });
        return measured(context.side, context.index);
      },
      now: () => "2030-01-02T00:00:00.000Z",
      environment: () => ({ fixture: true }),
      sourceManifest,
    });
    assert.deepEqual(calls.map(({ warmup, side }) => [warmup, side]), [
      [true, "before"], [true, "after"],
      [false, "before"], [false, "after"],
      [false, "after"], [false, "before"],
    ]);
    assert.equal(report.before.samples.length, 2);
    assert.equal(report.after.samples.length, 2);
    assert.equal(report.status, "PASS");
    assert.ok(report.comparison.throughput_mean_ratio_after_over_before > 1);
    assert.equal(report.measurement_contract.preset_performance_threshold, null);
    assert.equal(report.measurement_contract.cpu_utilization_used_as_throughput, false);
    assert.equal(report.measurement_contract.source_manifest_verified_before_and_after_every_invocation, true);
    assert.equal(report.measurement_contract.rayon_registry_capacity_is_not_worker_activity, true);
    assert.match(report.measurement_contract.runnable_thread_source, /\/proc/);
    assert.deepEqual(Object.keys(report.after.samples[0].phase_wall_ns), ["P0", "P1", "P2", "P3", "P4", "P5", "P6", "P7", "P8", "P9"]);
    assert.equal(report.after.samples[0].process_tree_thread_state.runnable_threads.maximum, 2);
    assert.deepEqual(report.after.samples[0].rayon_registry_capacity_samples, [4, 4, 4, 4]);
    assert.equal(report.measurement_contract.same_machine_for_both_sides, true);
    assert.deepEqual(report.environment_manifest, { fixture: true });
    assert.equal(report.before.role, "baseline");
    assert.equal(report.after.role, "new-engine");
  });

  it("rejects incomparable tick counts, source fingerprints and absent new-engine instrumentation", () => {
    const configuration = config();
    const wrongTicks = measured("after");
    wrongTicks.stdout = JSON.stringify({ ...JSON.parse(wrongTicks.stdout), completed_ticks: 999 });
    assert.throws(() => validateMeasuredSample(wrongTicks, configuration.after, configuration.workload, "after", true, configuration.environment_contract), /completed tick count|workload/);

    const wrongSource = measured("after");
    wrongSource.stdout = JSON.stringify({ ...JSON.parse(wrongSource.stdout), source_fingerprint: "c".repeat(64) });
    assert.throws(() => validateMeasuredSample(wrongSource, configuration.after, configuration.workload, "after", true, configuration.environment_contract), /source fingerprint/);

    const noStages = measured("after");
    noStages.stdout = JSON.stringify({ ...JSON.parse(noStages.stdout), phase_wall_ns: null });
    assert.throws(() => validateMeasuredSample(noStages, configuration.after, configuration.workload, "after", true, configuration.environment_contract), /phase wall/);

    const noRunnable = measured("after");
    noRunnable.stdout = JSON.stringify({ ...JSON.parse(noRunnable.stdout), runnable_samples: null });
    assert.throws(() => validateMeasuredSample(noRunnable, configuration.after, configuration.workload, "after", true, configuration.environment_contract), /runnable/);

    const wrongCapacity = measured("after");
    wrongCapacity.stdout = JSON.stringify({ ...JSON.parse(wrongCapacity.stdout), runnable_samples: [1, 1] });
    assert.throws(() => validateMeasuredSample(wrongCapacity, configuration.after, configuration.workload, "after", true, configuration.environment_contract), /registry-capacity/);

    const fakeRunnable = measured("after");
    fakeRunnable.process_tree_thread_state.runnable_threads = {
      minimum: 4,
      maximum: 4,
      mean: 4,
      histogram: { 4: 3 },
    };
    assert.throws(() => validateMeasuredSample(fakeRunnable, configuration.after, configuration.workload, "after", true, configuration.environment_contract), /sample count|histogram/);

    const unboundWorkload = measured("after");
    const unboundSample = JSON.parse(unboundWorkload.stdout);
    delete unboundSample.workload;
    delete unboundSample.environment_contract;
    unboundWorkload.stdout = JSON.stringify(unboundSample);
    assert.throws(() => validateMeasuredSample(unboundWorkload, configuration.after, configuration.workload, "after", true, configuration.environment_contract), /workload|scenario|toolchain|environment|keys mismatch/);

    const incompleteStages = measured("after");
    incompleteStages.stdout = JSON.stringify({ ...JSON.parse(incompleteStages.stdout), phase_wall_ns: { P9: "50" } });
    assert.throws(() => validateMeasuredSample(incompleteStages, configuration.after, configuration.workload, "after", true, configuration.environment_contract), /P0.*P9|all phases|phase wall/);

    const wrongScenario = measured("after");
    wrongScenario.stdout = JSON.stringify({ ...JSON.parse(wrongScenario.stdout), workload: { ...configuration.workload, scenario: "different" } });
    assert.throws(() => validateMeasuredSample(wrongScenario, configuration.after, configuration.workload, "after", true, configuration.environment_contract), /workload scenario/);

    const wrongToolchain = measured("after");
    wrongToolchain.stdout = JSON.stringify({ ...JSON.parse(wrongToolchain.stdout), environment_contract: { ...configuration.environment_contract, rustc: "rustc changed" } });
    assert.throws(() => validateMeasuredSample(wrongToolchain, configuration.after, configuration.workload, "after", true, configuration.environment_contract), /toolchain/);

    const wrongSetup = measured("after");
    const wrongSetupSample = JSON.parse(wrongSetup.stdout);
    wrongSetupSample.workload.setup_manifest.sha256 = "d".repeat(64);
    wrongSetup.stdout = JSON.stringify(wrongSetupSample);
    assert.throws(() => validateMeasuredSample(wrongSetup, configuration.after, configuration.workload, "after", true, configuration.environment_contract), /workload.*differ|setup/);

    const wrongRepetitions = measured("after");
    const wrongRepetitionsSample = JSON.parse(wrongRepetitions.stdout);
    wrongRepetitionsSample.workload.repetitions = 2;
    wrongRepetitions.stdout = JSON.stringify(wrongRepetitionsSample);
    assert.throws(() => validateMeasuredSample(wrongRepetitions, configuration.after, configuration.workload, "after", true, configuration.environment_contract), /workload.*differ|repetitions/);
  });

  it("treats explicit BLOCKED/FAIL samples as failures and never emits a ratio", () => {
    const configuration = config();
    for (const status of ["BLOCKED", "FAIL"]) {
      const blocked = measured("after");
      blocked.stdout = JSON.stringify({ ...JSON.parse(blocked.stdout), status });
      assert.throws(
        () => validateMeasuredSample(blocked, configuration.after, configuration.workload, "after", true, configuration.environment_contract),
        new RegExp(status),
      );
    }
  });

  it("rejects malformed or decorated command stdout instead of guessing a sample", () => {
    const configuration = config();
    for (const stdout of ["not-json", `${measured("after").stdout}\ntrailing-log`]) {
      const malformed = { ...measured("after"), stdout };
      assert.throws(
        () => validateMeasuredSample(malformed, configuration.after, configuration.workload, "after", true, configuration.environment_contract),
        /exactly one JSON sample/,
      );
    }
  });

  it("does not produce a report or comparison when a measured condition drifts", async () => {
    const configuration = config();
    await assert.rejects(
      runPerformanceHarness(configuration, {
        runSample: async (_endpoint, context) => {
          const sample = measured(context.side, context.index);
          if (!context.warmup && context.side === "after") {
            const stdout = JSON.parse(sample.stdout);
            stdout.workload.profile = "debug";
            sample.stdout = JSON.stringify(stdout);
          }
          return sample;
        },
        sourceManifest,
      }),
      /workload.*differ/,
    );
  });

  it("fails closed when source bytes do not match or drift during a sample", async () => {
    const configuration = config();
    await assert.rejects(
      runPerformanceHarness(configuration, {
        runSample: async (_endpoint, context) => measured(context.side, context.index),
        sourceManifest: async () => ({ sha256: "f".repeat(64) }),
      }),
      /source bytes.*fingerprint/,
    );

    let afterReads = 0;
    await assert.rejects(
      runPerformanceHarness(configuration, {
        runSample: async (_endpoint, context) => measured(context.side, context.index),
        sourceManifest: async (cwd) => {
          const expected = cwd === "/fixture/before-tree" ? BEFORE_FINGERPRINT : AFTER_FINGERPRINT;
          if (cwd === "/fixture/after-tree" && ++afterReads === 2) return { sha256: "e".repeat(64) };
          return { sha256: expected };
        },
      }),
      /source bytes.*fingerprint|source hash drifted/,
    );
  });

  it("rejects invalid, overflowing, or non-finite measurement numbers", () => {
    const configuration = config();
    for (const wallNs of ["0", "-1", "1.5", `${2n ** 64n}`]) {
      const invalid = measured("after");
      invalid.wall_ns = wallNs;
      assert.throws(
        () => validateMeasuredSample(invalid, configuration.after, configuration.workload, "after", true, configuration.environment_contract),
        /wall_ns/,
      );
    }

    const invalidRss = measured("after");
    invalidRss.peak_process_tree_rss_bytes = Number.POSITIVE_INFINITY;
    assert.throws(
      () => validateMeasuredSample(invalidRss, configuration.after, configuration.workload, "after", true, configuration.environment_contract),
      /RSS/,
    );

    const invalidPhase = measured("after");
    invalidPhase.stdout = JSON.stringify({
      ...JSON.parse(invalidPhase.stdout),
      phase_wall_ns: {
        ...JSON.parse(invalidPhase.stdout).phase_wall_ns,
        P4: `${2n ** 64n}`,
      },
    });
    assert.throws(
      () => validateMeasuredSample(invalidPhase, configuration.after, configuration.workload, "after", true, configuration.environment_contract),
      /phase wall entry P4/,
    );
  });

  it("rejects hidden environment differences and invalid sample-count shortcuts in config", () => {
    const missingEnvironment = config();
    missingEnvironment.environment_contract = {};
    assert.throws(() => validatePerformanceConfig(missingEnvironment), /environment contract/);

    const zeroSamples = config();
    zeroSamples.sample_count = 0;
    assert.throws(() => validatePerformanceConfig(zeroSamples), /sample_count/);

    const shellCommand = config();
    shellCommand.after.command = [];
    assert.throws(() => validatePerformanceConfig(shellCommand), /command/);

    const systemTemporaryCheckout = config();
    systemTemporaryCheckout.before.cwd = "/tmp/baseline";
    assert.throws(() => validatePerformanceConfig(systemTemporaryCheckout), /system temporary/);
  });

  it("stops measured wall time at child close instead of charging the sampler sleep", async () => {
    const measuredSample = await runProcessSample({
      command: [process.execPath, "-e", "setTimeout(() => {}, 100)"],
      cwd: process.cwd(),
      source_fingerprint: BEFORE_FINGERPRINT,
    }, { rssSampleIntervalMs: 500 });
    assert.ok(BigInt(measuredSample.wall_ns) < 400_000_000n, `wall_ns included sampler delay: ${measuredSample.wall_ns}`);
    assert.ok(measuredSample.process_tree_thread_state.sample_count > 0);
    assert.ok(measuredSample.process_tree_thread_state.total_threads.maximum > 0);
  });

  it("fails loudly when the benchmark command exits non-zero", async () => {
    await assert.rejects(
      runProcessSample({
        command: [process.execPath, "-e", "process.stderr.write('fixture failed'); process.exit(7)"],
        cwd: process.cwd(),
        source_fingerprint: BEFORE_FINGERPRINT,
      }, { rssSampleIntervalMs: 1 }),
      /exited 7.*fixture failed/,
    );
  });

  it("uses the fixed perf-report.json name and writes identical evidence idempotently", async () => {
    const outputDirectory = await newTemporaryDirectory();
    const reportPath = reportPathForOutputDirectory(outputDirectory);
    assert.equal(path.basename(reportPath), "perf-report.json");
    const bytes = "{\n  \"status\": \"PASS\"\n}\n";
    assert.equal(await writeReportIdempotently(reportPath, bytes), "created");
    assert.equal(await writeReportIdempotently(reportPath, bytes), "unchanged");
    assert.equal(await readFile(reportPath, "utf8"), bytes);
    await assert.rejects(
      writeReportIdempotently(reportPath, "{\"status\":\"different\"}\n"),
      /different content/,
    );
  });

  it("parses a config file once and reuses a matching PASS report without rerunning commands", async () => {
    const outputDirectory = await newTemporaryDirectory();
    const configPath = path.join(outputDirectory, "perf-config.json");
    await writeFile(configPath, `${JSON.stringify(config(), null, 2)}\n`);
    let runs = 0;
    const summaries = [];
    const runHarness = async (configuration, options) => {
      runs += 1;
      return runPerformanceHarness(configuration, {
        runSample: async (_endpoint, context) => measured(context.side, context.index),
        now: () => "2030-01-02T00:00:00.000Z",
        environment: options.environment,
        sourceManifest: options.sourceManifest,
      });
    };
    const dependencies = { runHarness, environment: () => ({ fixture: true }), sourceManifest, stdout: (line) => summaries.push(JSON.parse(line)) };
    assert.equal((await main([configPath, outputDirectory], dependencies)).write, "created");
    assert.equal((await main([configPath, outputDirectory], dependencies)).write, "unchanged");
    assert.equal(runs, 1);
    assert.deepEqual(summaries.map(({ status, write }) => [status, write]), [["PASS", "created"], ["PASS", "unchanged"]]);
    await assert.rejects(
      main([configPath, outputDirectory], { ...dependencies, environment: () => ({ fixture: "different-machine" }) }),
      /conditions differ/,
    );
    assert.equal(runs, 1);
  });
});
