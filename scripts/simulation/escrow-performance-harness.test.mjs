import assert from "node:assert/strict";
import { describe, it } from "node:test";

import {
  runPerformanceHarness,
  runProcessSample,
  validateMeasuredSample,
  validatePerformanceConfig,
} from "./escrow-performance-harness.mjs";

const BEFORE_FINGERPRINT = "a".repeat(64);
const AFTER_FINGERPRINT = "b".repeat(64);

function config() {
  return {
    schema: "escrow-perf-config-v1",
    workload: { scenario: "fixed-multi-stock", seed: "7", completed_ticks: 1000, profile: "release", features: ["simulation-diagnostics"] },
    environment_contract: { cargo: "cargo 1.96.0", rustc: "rustc 1.96.0", target: "x86_64-unknown-linux-gnu", rustflags: "", cargo_jobs: 4 },
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
    stdout: JSON.stringify({
      schema: "escrow-perf-sample-v1",
      workload: config().workload,
      environment_contract: config().environment_contract,
      completed_ticks: 1000,
      source_fingerprint: isAfter ? AFTER_FINGERPRINT : BEFORE_FINGERPRINT,
      phase_wall_ns: isAfter ? Object.fromEntries(Array.from({ length: 10 }, (_, phase) => [`P${phase}`, String(50 + phase)])) : null,
      runnable_samples: isAfter ? [1, 2, 2, 1] : null,
    }),
    stderr: "",
  };
}

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
    });
    assert.deepEqual(calls.map(({ warmup, side }) => [warmup, side]), [
      [true, "before"], [true, "after"],
      [false, "before"], [false, "after"],
      [false, "after"], [false, "before"],
    ]);
    assert.equal(report.before.samples.length, 2);
    assert.equal(report.after.samples.length, 2);
    assert.ok(report.comparison.throughput_mean_ratio_after_over_before > 1);
    assert.equal(report.measurement_contract.preset_performance_threshold, null);
    assert.equal(report.measurement_contract.cpu_utilization_used_as_throughput, false);
    assert.deepEqual(Object.keys(report.after.samples[0].phase_wall_ns), ["P0", "P1", "P2", "P3", "P4", "P5", "P6", "P7", "P8", "P9"]);
    assert.deepEqual(report.runtime_environment, { fixture: true });
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
  });

  it("stops measured wall time at child close instead of charging the sampler sleep", async () => {
    const measuredSample = await runProcessSample({
      command: [process.execPath, "-e", "setTimeout(() => {}, 100)"],
      cwd: process.cwd(),
      source_fingerprint: BEFORE_FINGERPRINT,
    }, { rssSampleIntervalMs: 500 });
    assert.ok(BigInt(measuredSample.wall_ns) < 400_000_000n, `wall_ns included sampler delay: ${measuredSample.wall_ns}`);
  });
});
