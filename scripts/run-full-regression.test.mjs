import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { mkdir, mkdtemp, readFile, readdir, rm, symlink, writeFile } from "node:fs/promises";
import { it } from "node:test";

import path from "node:path";

import {
  buildFullRegressionArtifacts,
  buildRustTestExecutionPolicy,
  collectFullRegressionSourceFingerprint,
  executeFullRegression,
  executeRustTestBinaries,
  FULL_REGRESSION_SOURCE_INPUTS,
  fullRegressionSteps,
  main,
  parseFullRegressionArgs,
  parseCargoTestExecutables,
  parseRustTestCases,
  runFullRegression,
  runFullRegressionPhase,
} from "./run-full-regression.mjs";
import { prepareWorkspacePaths, resolveWorkspaceRoot } from "./workspace-paths.mjs";
import { runBoundedCommand } from "./run-with-deadline.mjs";
import * as regression from "./run-full-regression.mjs";

it("Rust case discovery rejects malformed output and excludes benchmarks", { timeout: 10000 }, () => {
  assert.deepEqual(parseRustTestCases("suite::case: test\nbench: benchmark\n1 test, 1 benchmark\n"), ["suite::case"]);
  assert.throws(() => parseRustTestCases(undefined), /did not return output/);
  assert.throws(() => parseRustTestCases("unexpected output"), /malformed/);
  assert.throws(() => parseRustTestCases("same: test\nsame: test\n"), /duplicate/);
});

it("Rust ordinary cases in one binary run concurrently with independent deadlines and ignored cases excluded", { timeout: 10000 }, async () => {
  const calls = [];
  let active = 0;
  let peak = 0;
  const result = await executeRustTestBinaries({
    artifacts: [{ executable: "fixture", label: "fixture" }],
    policy: buildRustTestExecutionPolicy(8, 1), cwd: process.cwd(), env: {}, now: Date.now,
    remainingMs: () => 2500,
    run: async (options) => {
      calls.push(options);
      if (options.args.includes("--list")) return { stdout: options.args.includes("--ignored") ? "long: test\n" : "short-a: test\nshort-b: test\nlong: test\n" };
      active += 1;
      peak = Math.max(peak, active);
      await new Promise((resolve) => setTimeout(resolve, 10));
      active -= 1;
    },
  });
  assert.equal(peak, 2);
  assert.equal(result.case_count, 2);
  assert.equal(result.max_concurrent_cases, 2);
  assert.deepEqual(calls.filter((call) => !call.args.includes("--list")).map((call) => call.args[0]), ["short-a", "short-b"]);
  assert.ok(calls.every((call) => call.timeoutMs === 2500));
});

it("Rust ordinary case watchdog terminates a hanging process and reports the exact case", { timeout: 10000 }, async () => {
  const startedAt = Date.now();
  await assert.rejects(executeRustTestBinaries({
    artifacts: [{ executable: "fixture", label: "fixture-label" }],
    policy: buildRustTestExecutionPolicy(4, 1), cwd: process.cwd(), env: process.env, now: Date.now,
    remainingMs: () => 400,
    run: async (options) => {
      assert.equal(options.timeoutMs, 400);
      if (options.args.includes("--list")) return { stdout: options.args.includes("--ignored") ? "" : "hanging::case: test\n" };
      return runBoundedCommand({ ...options, command: process.execPath, args: ["-e", "setInterval(() => {}, 1000)"], cleanupReserveMs: 100 });
    },
  }), /fixture-label case hanging::case failed:.*400ms deadline/);
  assert.ok(Date.now() - startedAt < 1500);
});

it("Rust ordinary case failure aborts active sibling cases and never launches queued cases", { timeout: 10000 }, async () => {
  let siblingStarted;
  const started = new Promise((resolve) => { siblingStarted = resolve; });
  const executed = [];
  let aborted = false;
  await assert.rejects(executeRustTestBinaries({
    artifacts: [{ executable: "fixture", label: "fixture" }],
    policy: buildRustTestExecutionPolicy(4, 1), cwd: process.cwd(), env: {}, now: Date.now,
    remainingMs: () => 300000,
    run: async ({ args, signal, timeoutMs }) => {
      assert.equal(timeoutMs, 10000);
      if (args.includes("--list")) return { stdout: args.includes("--ignored") ? "" : "fail: test\nsibling: test\nqueued: test\n" };
      executed.push(args[0]);
      if (args[0] === "fail") {
        await started;
        throw new Error("injected case failure");
      }
      siblingStarted();
      await new Promise((resolve, reject) => signal.addEventListener("abort", () => {
        aborted = true;
        reject(signal.reason);
      }, { once: true }));
    },
  }), /fixture case fail failed: injected case failure/);
  assert.equal(aborted, true);
  assert.deepEqual(executed, ["fail", "sibling"]);
});

it("Rust case listing fails explicitly instead of skipping malformed or unknown ignored cases", { timeout: 10000 }, async () => {
  for (const ignoredOutput of ["bad output", "unknown: test\n"]) {
    let executions = 0;
    await assert.rejects(executeRustTestBinaries({
      artifacts: [{ executable: "fixture", label: "fixture" }],
      policy: buildRustTestExecutionPolicy(8, 1), cwd: process.cwd(), env: {}, now: Date.now,
      remainingMs: () => 300000,
      run: async ({ args }) => {
        if (!args.includes("--list")) executions += 1;
        return { stdout: args.includes("--ignored") ? ignoredOutput : "ordinary: test\n" };
      },
    }), /malformed Rust test listing|contains an unknown case/);
    assert.equal(executions, 0);
  }
});

it("resolves a linked worktree through the Git common directory", async () => {
  const actualWorkspaceRoot = await resolveWorkspaceRoot(process.cwd());
  const fixtureRoot = await mkdtemp(path.join(actualWorkspaceRoot, ".tmp", "linked-worktree-test-"));
  const workspaceRoot = path.join(fixtureRoot, "workspace");
  const sourceRoot = path.join(workspaceRoot, ".worktree", "candidate");
  const gitDir = path.join(workspaceRoot, ".git", "worktrees", "candidate");
  await Promise.all([mkdir(sourceRoot, { recursive: true }), mkdir(gitDir, { recursive: true })]);
  await Promise.all([
    writeFile(path.join(sourceRoot, ".git"), `gitdir: ${gitDir}\n`),
    writeFile(path.join(gitDir, "commondir"), "../..\n"),
  ]);
  try {
    assert.equal(await resolveWorkspaceRoot(sourceRoot), workspaceRoot);
  } finally {
    await rm(fixtureRoot, { recursive: true, force: true });
  }
});

it("forces one workspace-local Cargo target and process-temp directory for every step", async () => {
  const repoRoot = process.cwd();
  const workspaceRoot = await resolveWorkspaceRoot(repoRoot);
  const hostile = { ...process.env, CARGO_TARGET_DIR: "/tmp/external-target", TMPDIR: "/tmp", TMP: "/tmp", TEMP: "/tmp" };
  const steps = fullRegressionSteps(8, repoRoot, hostile, workspaceRoot);
  const target = path.join(workspaceRoot, ".tmp", "build-cache", "full-regression");
  const processTmp = path.join(workspaceRoot, ".tmp", "process-tmp", "full-regression");
  for (const step of steps) {
    assert.equal(step.env.CARGO_TARGET_DIR, target);
    assert.equal(step.env.TMPDIR, processTmp);
    assert.equal(step.env.TMP, processTmp);
    assert.equal(step.env.TEMP, processTmp);
  }
  assert.deepEqual(steps.map((step) => step.kind), [
    "rust-test-build",
    "rust-test-binaries",
    "rust-doctests",
    "ordinary-command",
  ]);
  assert.deepEqual(steps[0].args, ["test", "--workspace", "--no-run", "--message-format=json-render-diagnostics"]);
  assert.deepEqual(steps[2].args, ["test", "--workspace", "--doc", "--no-fail-fast", "--", "--test-threads=7"]);
});

it("parses, deduplicates, sorts, and contains prebuilt Cargo test executables", () => {
  const target = "/workspace/.tmp/build-cache/full-regression";
  const output = [
    JSON.stringify({ reason: "compiler-message", message: { rendered: "warning" } }),
    JSON.stringify({ reason: "compiler-artifact", package_id: "b 0.1.0", target: { name: "z", kind: ["test"], test: true }, profile: { test: true }, executable: `${target}/debug/deps/z-bbb` }),
    JSON.stringify({ reason: "compiler-artifact", package_id: "a 0.1.0", target: { name: "a", kind: ["lib"], test: true }, profile: { test: true }, executable: `${target}/debug/deps/a-aaa` }),
    JSON.stringify({ reason: "compiler-artifact", package_id: "a 0.1.0", target: { name: "a", kind: ["lib"], test: true }, profile: { test: true }, executable: `${target}/debug/deps/a-aaa` }),
    JSON.stringify({ reason: "compiler-artifact", package_id: "ignored", target: { name: "example", kind: ["example"], test: false }, profile: { test: false }, executable: `${target}/debug/examples/example` }),
  ].join("\n");
  assert.deepEqual(parseCargoTestExecutables(output, target), [
    { executable: `${target}/debug/deps/a-aaa`, label: "a 0.1.0 lib:a" },
    { executable: `${target}/debug/deps/z-bbb`, label: "b 0.1.0 test:z" },
  ]);
  assert.throws(
    () => parseCargoTestExecutables(JSON.stringify({ reason: "compiler-artifact", package_id: "escape", target: { name: "escape", kind: ["test"], test: true }, profile: { test: true }, executable: "/tmp/escape" }), target),
    /outside.*Cargo target/i,
  );
});

it("partitions the detected CPU budget across concurrent binaries without overselling", () => {
  assert.deepEqual(buildRustTestExecutionPolicy(128, 80), {
    available_cpu_count: 128,
    binary_count: 80,
    max_concurrent_binaries: 8,
    worker_budgets: Array.from({ length: 8 }, () => ({ cpu_budget: 16, test_threads: 12, rayon_threads: 4 })),
    aggregate_configured_thread_budget: 128,
  });
  const small = buildRustTestExecutionPolicy(4, 40);
  assert.equal(small.max_concurrent_binaries, 2);
  assert.deepEqual(small.worker_budgets, [
    { cpu_budget: 2, test_threads: 1, rayon_threads: 1 },
    { cpu_budget: 2, test_threads: 1, rayon_threads: 1 },
  ]);
  for (let cpuCount = 2; cpuCount <= 129; cpuCount += 1) {
    const policy = buildRustTestExecutionPolicy(cpuCount, 80);
    assert.equal(policy.aggregate_configured_thread_budget, cpuCount, `CPU ${cpuCount} is misbudgeted by ${JSON.stringify(policy)}`);
    assert.ok(policy.max_concurrent_binaries <= 8);
    assert.ok(policy.worker_budgets.every((budget) => budget.test_threads + budget.rayon_threads === budget.cpu_budget));
  }
  assert.deepEqual(buildRustTestExecutionPolicy(5, 80).worker_budgets.map((budget) => budget.cpu_budget), [3, 2]);
  const singleCpu = buildRustTestExecutionPolicy(1, 80);
  assert.deepEqual(singleCpu.worker_budgets, [{ cpu_budget: 1, test_threads: 1, rayon_threads: 1 }]);
  assert.equal(singleCpu.aggregate_configured_thread_budget, 2, "the one-CPU Rayon and harness minimum must be reported honestly");
});

it("fingerprints Rust, Web, package-manager, and deadline-runner inputs", () => {
  for (const required of [
    "Cargo.lock",
    "packages/engine",
    "package.json",
    "pnpm-lock.yaml",
    "apps/web/package.json",
    "apps/web/src",
    "scripts/run-full-regression.mjs",
    "scripts/run-long-validation.mjs",
    "scripts/run-web-tests.mjs",
    "scripts/run-with-deadline.mjs",
    "scripts/workspace-paths.mjs",
  ]) assert.ok(FULL_REGRESSION_SOURCE_INPUTS.includes(required), `missing fingerprint input ${required}`);
});

it("ignores only Tauri-generated schemas while detecting changes to real desktop inputs", async () => {
  const workspaceRoot = await resolveWorkspaceRoot(process.cwd());
  const sourceRoot = await mkdtemp(path.join(workspaceRoot, ".tmp", "full-regression-source-"));
  const directoryInputs = new Set([
    ".cargo", "packages/engine", "packages/engine-gpu", "apps/server",
    "apps/web/src", "apps/web-wasm", "apps/desktop/src-tauri",
  ]);
  const desktopRoot = path.join(sourceRoot, "apps", "desktop", "src-tauri");
  const desktopInputs = [
    "tauri.conf.json", "Cargo.toml", "build.rs", "capabilities/default.json",
    "icons/icon.ico", "gen/other.rs",
  ];
  try {
    for (const input of FULL_REGRESSION_SOURCE_INPUTS) {
      const target = path.join(sourceRoot, input);
      if (directoryInputs.has(input)) {
        await mkdir(target, { recursive: true });
        await writeFile(path.join(target, "source.txt"), "source");
      } else {
        await mkdir(path.dirname(target), { recursive: true });
        await writeFile(target, "source");
      }
    }
    for (const input of desktopInputs) {
      const target = path.join(desktopRoot, input);
      await mkdir(path.dirname(target), { recursive: true });
      await writeFile(target, "source-A");
    }
    const before = await collectFullRegressionSourceFingerprint(sourceRoot);
    assert.equal(before.algorithm, "full-regression-source-fingerprint");
    assert.equal(before.algorithm_version, 1);
    const generated = path.join(desktopRoot, "gen", "schemas", "capabilities.json");
    await mkdir(path.dirname(generated), { recursive: true });
    await writeFile(generated, "generated");
    const after = await collectFullRegressionSourceFingerprint(sourceRoot);
    assert.deepEqual(after, before, "Tauri's generated schema directory is not source");
    await writeFile(generated, "regenerated");
    assert.deepEqual(await collectFullRegressionSourceFingerprint(sourceRoot), before,
      "regenerating Tauri schemas must not change the source fingerprint");
    for (const input of desktopInputs) {
      const target = path.join(desktopRoot, input);
      await writeFile(target, "source-B");
      const changed = await collectFullRegressionSourceFingerprint(sourceRoot);
      assert.notEqual(changed.digest, before.digest, `${input} must remain source-bound`);
      await writeFile(target, "source-A");
    }
  } finally {
    await rm(sourceRoot, { recursive: true, force: true });
  }
});

it("CI invokes the sealed build/execute regression phases without Corepack", async () => {
  const workflow = await readFile(path.join(process.cwd(), ".github", "workflows", "ci.yml"), "utf8");
  assert.match(workflow, /run-full-regression\.mjs build --inventory \.tmp\/full-regression\/ci-inventory\.json/);
  assert.match(workflow, /run-full-regression\.mjs execute --inventory \.tmp\/full-regression\/ci-inventory\.json/);
  assert.match(workflow, /pnpm\/action-setup@v4/);
  assert.doesNotMatch(workflow, /corepack/i);
  assert.doesNotMatch(workflow, /cargo test --workspace/);
  assert.doesNotMatch(workflow, /pnpm --filter web test(?:\s|$)/);
  const cargoCacheStep = workflow.split("- name: Cache cargo\n")[1]?.split("- name: Get pnpm store dir")[0];
  assert.ok(cargoCacheStep, "CI must configure the Cargo cache");
  assert.match(cargoCacheStep, /workspaces:\s*\|\s*\n\s*\. -> target/);
  assert.doesNotMatch(cargoCacheStep, /\.tmp\/build-cache\/full-regression/);
  assert.match(cargoCacheStep, /cache-on-failure:\s*true/);
  const orderedSteps = [
    "- name: Build shared production UI",
    "- name: Build sealed full-regression artifacts",
    "- name: Execute sealed full regression",
  ];
  const positions = orderedSteps.map((step) => workflow.indexOf(step));
  assert.ok(positions.every((position) => position >= 0), "CI must retain the real WASM, Web, and Rust regression steps");
  assert.ok(positions.every((position, index) => index === 0 || position > positions[index - 1]), "CI must build real WASM and Web assets before Cargo compiles Tauri tests");
  assert.match(workflow, /run: node scripts\/frontend-build\.mjs --jobs/);
  const { createFrontendPlan } = await import("./frontend-build.mjs");
  for (const host of ["windows", "linux"]) {
    const plan = createFrontendPlan({ jobs: 4 }, { host });
    const wasmPosition = plan.commands.findIndex(({ command }) => command === "wasm-pack");
    const sourceCheck = plan.commands.findIndex(({ args }) => args[0].endsWith("check-wasm-threading.mjs"));
    const copyPosition = plan.commands.findIndex(({ action }) => action === "copy-wasm");
    const copiedCheck = plan.commands.findIndex(({ args }) => args[1] === path.join(plan.frontend, "wasm-pkg/web_wasm.js"));
    const uiPosition = plan.commands.findIndex(({ args }) => args[0].endsWith("vite.js"));
    assert.ok(wasmPosition >= 0 && sourceCheck > wasmPosition && copyPosition > sourceCheck && copiedCheck > copyPosition && uiPosition > copiedCheck,
      `${host} must build and verify real WASM before copying, checking the copy and building the UI`);
  }
});

it("CI refreshes the sealed Cargo cache after a started build without caching acceptance evidence", async () => {
  const workflow = await readFile(path.join(process.cwd(), ".github", "workflows", "ci.yml"), "utf8");
  const restore = workflow.split("- name: Restore sealed Cargo cache\n")[1]?.split("- name:")[0];
  const ready = workflow.split("- name: Check sealed Cargo cache directory\n")[1]?.split("- name:")[0];
  const save = workflow.split("- name: Save sealed Cargo cache\n")[1]?.split("- name:")[0];
  assert.ok(restore, "sealed Cargo cache must have a dedicated restore step");
  assert.ok(save, "sealed Cargo cache must have a dedicated save step");
  assert.ok(ready, "only an existing Cargo cache directory may be saved");
  assert.match(restore, /id: sealed-cargo-cache/);
  assert.match(restore, /uses: actions\/cache\/restore@v4/);
  assert.match(save, /uses: actions\/cache\/save@v4/);
  for (const step of [restore, save]) {
    assert.match(step, /path: \.tmp\/build-cache\/full-regression\s*\n/);
    assert.doesNotMatch(step, /ci-inventory|artifact-inventory/);
  }
  const primary = restore.match(/^\s*key: (.+)$/m)?.[1];
  const fallback = restore.match(/restore-keys:\s*\|\s*\n\s*(.+)/)?.[1];
  assert.ok(primary && fallback, "unique save key must have an environment-bound fallback");
  assert.equal(primary, `${fallback}\${{ github.run_id }}-\${{ github.run_attempt }}`);
  assert.match(fallback, /runner\.os/);
  assert.match(fallback, /runner\.arch/);
  for (const input of ["rust-toolchain.toml", ".cargo/config.toml", "Cargo.lock", "Cargo.toml", "packages/*/Cargo.toml", "apps/*/Cargo.toml", "apps/desktop/src-tauri/Cargo.toml"]) {
    assert.ok(fallback.includes(`'${input}'`), `${input} must bind the cache environment`);
  }
  assert.match(save, /key: \$\{\{ steps\.sealed-cargo-cache\.outputs\.cache-primary-key \}\}/);
  assert.match(ready, /if: always\(\) && \(steps\.sealed-build\.outcome == 'success' \|\| steps\.sealed-build\.outcome == 'failure' \|\| steps\.native-prepare\.outcome == 'success' \|\| steps\.native-prepare\.outcome == 'failure'\)/);
  assert.match(ready, /fs\.statSync\(p, \{throwIfNoEntry:false\}\)\?\.isDirectory\(\) === true/);
  assert.match(ready, /const p = '\.tmp\/build-cache\/full-regression'/);
  assert.match(ready, /process\.env\.GITHUB_OUTPUT/);
  assert.match(save, /if: always\(\) && steps\.sealed-cache-ready\.outputs\.exists == 'true'/);
  const buildPosition = workflow.indexOf("- name: Build sealed full-regression artifacts");
  const savePosition = workflow.indexOf("- name: Save sealed Cargo cache");
  const executePosition = workflow.indexOf("- name: Execute sealed full regression");
  assert.ok(workflow.indexOf("- name: Restore sealed Cargo cache") < buildPosition);
  assert.ok(buildPosition < savePosition && savePosition < executePosition);
  assert.match(workflow.slice(buildPosition, savePosition), /id: sealed-build/);
});

it("CI cache directory probe distinguishes a directory, a missing path, and an invalid path", async () => {
  const workflow = await readFile(path.join(process.cwd(), ".github", "workflows", "ci.yml"), "utf8");
  const ready = workflow.split("- name: Check sealed Cargo cache directory\n")[1]?.split("- name:")[0];
  const script = ready?.match(/run: node -e "([^"\n]+)"/)?.[1];
  assert.ok(script, "cache directory probe must be executable with Node on each platform");
  const workspaceRoot = await resolveWorkspaceRoot(process.cwd());
  const fixture = await mkdtemp(path.join(workspaceRoot, ".tmp", "sealed-cache-probe-"));
  const target = path.join(fixture, ".tmp", "build-cache", "full-regression");
  const output = path.join(fixture, "output");
  const runProbe = () => execFileSync(process.execPath, ["-e", script], {
    cwd: fixture, env: { ...process.env, GITHUB_OUTPUT: output }, timeout: 1000, stdio: "pipe",
  });
  try {
    runProbe();
    assert.equal(await readFile(output, "utf8"), "exists=false\n");
    await writeFile(output, "");
    await mkdir(target, { recursive: true });
    runProbe();
    assert.equal(await readFile(output, "utf8"), "exists=true\n");
    await writeFile(output, "");
    await rm(target, { recursive: true });
    await writeFile(target, "not a directory");
    runProbe();
    assert.equal(await readFile(output, "utf8"), "exists=false\n");
    await rm(output);
    await mkdir(output);
    assert.throws(runProbe,
      (error) => Number.isInteger(error.status) && error.status !== 0 && error.stderr.length > 0,
      "output I/O failures must exit unsuccessfully with an explicit diagnostic");
  } finally {
    await rm(fixture, { recursive: true, force: true });
  }
});

async function createSealedFixture({ fingerprint = { algorithm: "test", algorithm_version: 1, files: [], digest: "a".repeat(64) } } = {}) {
  const workspaceRoot = await resolveWorkspaceRoot(process.cwd());
  const target = path.join(workspaceRoot, ".tmp", "build-cache", "full-regression");
  const artifact = path.join(target, "debug", "deps", `full-regression-fixture-${process.pid}-${Date.now()}`);
  const inventoryPath = path.join(workspaceRoot, ".tmp", "full-regression", `inventory-${process.pid}-${Date.now()}.json`);
  await mkdir(path.dirname(artifact), { recursive: true });
  await writeFile(artifact, "sealed test binary");
  const buildStdout = `${JSON.stringify({
    reason: "compiler-artifact",
    package_id: "engine 0.1.0",
    target: { name: "company_scenarios", kind: ["test"], test: true },
    profile: { test: true },
    executable: artifact,
  })}\n`;
  await buildFullRegressionArtifacts({
    cwd: process.cwd(),
    inventoryPath,
    collectFingerprint: async () => fingerprint,
    run: async (options) => {
      assert.equal(options.captureOutput, true);
      return { stdout: buildStdout, stderr: "" };
    },
  });
  return {
    artifact,
    fingerprint,
    inventoryPath,
    cleanup: async () => {
      await Promise.all([
        rm(artifact, { force: true }),
        rm(inventoryPath, { force: true }),
      ]);
    },
  };
}

it("reports Cargo build progress before a real timed-out child is killed", async () => {
  const workspaceRoot = await resolveWorkspaceRoot(process.cwd());
  const inventoryPath = path.join(workspaceRoot, ".tmp", "full-regression", `timeout-inventory-${process.pid}-${Date.now()}.json`);
  const logs = [];
  const diagnostics = [];
  const message = JSON.stringify({ reason: "compiler-artifact", package_id: "slow-package 0.1.0", target: { name: "slow_test" } });
  try {
    await assert.rejects(buildFullRegressionArtifacts({
      cwd: process.cwd(), inventoryPath,
      cpuCount: 4,
      collectFingerprint: async () => ({ algorithm: "test", algorithm_version: 1, files: [], digest: "a".repeat(64) }),
      log: (line) => logs.push(JSON.parse(line)),
      writeDiagnostic: (chunk) => diagnostics.push(chunk.toString()),
      progressIntervalMs: 100,
      run: (options) => runBoundedCommand({
        ...options,
        command: process.execPath,
        args: ["-e", `process.stdout.write(${JSON.stringify(`${message}\n`)}); process.stderr.write('Compiling slow-package\\n'); setInterval(() => {}, 1000)`],
        timeoutMs: 650,
        cleanupReserveMs: 100,
      }),
    }), /total 650ms deadline.*process tree/i);
    assert.ok(logs.some((entry) => entry.phase === "build" && entry.status === "started" && entry.cpu_jobs === 4));
    assert.ok(logs.some((entry) => entry.phase === "build" && entry.status === "running" && entry.last_completed_target === "slow-package 0.1.0 slow_test" && entry.elapsed_ms >= 100));
    assert.ok(logs.some((entry) => entry.phase === "build" && entry.status === "failed" && entry.last_completed_target === "slow-package 0.1.0 slow_test"));
    assert.match(diagnostics.join(""), /Compiling slow-package/);
    await assert.rejects(readFile(inventoryPath), /ENOENT/);
  } finally {
    await rm(inventoryPath, { force: true });
  }
});

it("reports bounded rendered Cargo errors from a real failing child", async () => {
  const workspaceRoot = await resolveWorkspaceRoot(process.cwd());
  const inventoryPath = path.join(workspaceRoot, ".tmp", "full-regression", `failure-inventory-${process.pid}-${Date.now()}.json`);
  const logs = [];
  const message = JSON.stringify({ reason: "compiler-message", package_id: "broken-package 0.1.0", message: { level: "error", rendered: "error[E0001]: deliberate fixture failure" } });
  try {
    await assert.rejects(buildFullRegressionArtifacts({
      cwd: process.cwd(), inventoryPath,
      collectFingerprint: async () => ({ algorithm: "test", algorithm_version: 1, files: [], digest: "a".repeat(64) }),
      log: (line) => logs.push(JSON.parse(line)),
      writeDiagnostic: () => undefined,
      run: (options) => runBoundedCommand({
        ...options,
        command: process.execPath,
        args: ["-e", `process.stdout.write(${JSON.stringify(message)}); process.exit(2)`],
      }),
    }), /exited with 2/i);
    assert.ok(logs.some((entry) => entry.status === "compiler_error" && entry.package === "broken-package 0.1.0" && /deliberate fixture failure/.test(entry.diagnostic)));
    assert.ok(logs.some((entry) => entry.status === "failed" && entry.compiler_errors === 1));
    await assert.rejects(readFile(inventoryPath), /ENOENT/);
  } finally {
    await rm(inventoryPath, { force: true });
  }
});

it("keeps the timeout reason when Cargo leaves an incomplete JSON line", async () => {
  const workspaceRoot = await resolveWorkspaceRoot(process.cwd());
  const inventoryPath = path.join(workspaceRoot, ".tmp", "full-regression", `partial-inventory-${process.pid}-${Date.now()}.json`);
  const logs = [];
  try {
    await assert.rejects(buildFullRegressionArtifacts({
      cwd: process.cwd(), inventoryPath,
      collectFingerprint: async () => ({ algorithm: "test", algorithm_version: 1, files: [], digest: "a".repeat(64) }),
      log: (line) => logs.push(JSON.parse(line)),
      writeDiagnostic: () => undefined,
      run: (options) => runBoundedCommand({
        ...options,
        command: process.execPath,
        args: ["-e", `process.stdout.write(${JSON.stringify('{"reason":')}); setInterval(() => {}, 1000)`],
        timeoutMs: 500,
        cleanupReserveMs: 100,
      }),
    }), (error) => /total 500ms deadline/i.test(error.message) && /malformed JSON/i.test(error.message));
    assert.ok(logs.some((entry) => entry.status === "failed" && /total 500ms deadline/i.test(entry.error) && /malformed JSON/i.test(entry.error)));
    await assert.rejects(readFile(inventoryPath), /ENOENT/);
  } finally {
    await rm(inventoryPath, { force: true });
  }
});

it("seals the source identity and exact binary bytes in an atomic build inventory", async () => {
  const fixture = await createSealedFixture();
  try {
    const inventory = JSON.parse(await readFile(fixture.inventoryPath, "utf8"));
    assert.equal(inventory.schema, "full-regression-artifact-inventory");
    assert.equal(inventory.schema_version, 1);
    assert.equal(inventory.source_fingerprint.digest, fixture.fingerprint.digest);
    assert.match(inventory.identity_digest, /^[a-f0-9]{64}$/);
    assert.equal(inventory.artifacts.length, 1);
    assert.match(inventory.artifacts[0].executable_relative_path, /^\.tmp\/build-cache\/full-regression\//);
    assert.equal(inventory.artifacts[0].bytes, Buffer.byteLength("sealed test binary"));
    assert.match(inventory.artifacts[0].sha256, /^[a-f0-9]{64}$/);
    const temporaryFiles = (await readdir(path.dirname(fixture.inventoryPath)))
      .filter((entry) => entry.startsWith(`${path.basename(fixture.inventoryPath)}.`) && entry.endsWith(".tmp"));
    assert.deepEqual(temporaryFiles, []);
  } finally {
    await fixture.cleanup();
  }
});

function resealInventory(record) {
  const { identity_digest: ignored, ...state } = record;
  return { ...record, identity_digest: createHash("sha256").update(JSON.stringify(state)).digest("hex") };
}

it("owns an immutable inventory while preserving decoded order, extras and duplicate artifacts", async () => {
  assert.equal(typeof regression.ArtifactInventory, "function", "ArtifactInventory must own the sealed record");
  const fixture = await createSealedFixture();
  try {
    const decoded = JSON.parse(await readFile(fixture.inventoryPath, "utf8"));
    const reordered = resealInventory({
      extra: { note: "保留扩展字段" },
      artifacts: [decoded.artifacts[0], decoded.artifacts[0]],
      ...Object.fromEntries(Object.entries(decoded).filter(([key]) => key !== "artifacts")),
    });
    const expectedIdentity = {
      sourceRoot: decoded.source_root,
      workspacePaths: {
        workspaceRoot: decoded.workspace_root,
        cargoTargetDir: decoded.cargo_target_dir,
        processTmpDir: decoded.process_tmp_dir,
      },
      fingerprint: fixture.fingerprint,
    };
    const inventory = regression.ArtifactInventory.fromDecodedInventory(reordered, expectedIdentity);
    const originalJson = JSON.stringify(reordered);
    reordered.extra.note = "调用方修改";
    reordered.artifacts[0].bytes += 1;
    assert.equal(JSON.stringify(inventory.toJson()), originalJson);
    const exported = inventory.toJson();
    exported.artifacts.pop();
    exported.extra.note = "输出修改";
    assert.equal(JSON.stringify(inventory.toJson()), originalJson);
    assert.equal(inventory.artifactDescriptors().length, 2);
    assert.throws(() => inventory.artifactDescriptors().pop(), TypeError);
    assert.throws(() => { inventory.artifactDescriptors()[0].bytes += 1; }, TypeError);
    const staleOrder = { ...decoded };
    delete staleOrder.schema;
    staleOrder.schema = decoded.schema;
    assert.throws(() => regression.ArtifactInventory.fromDecodedInventory(staleOrder, expectedIdentity), /malformed, stale, or bound/i);
    assert.equal(JSON.stringify(regression.ArtifactInventory.fromDecodedInventory(
      resealInventory(staleOrder), expectedIdentity,
    ).toJson()), JSON.stringify(resealInventory(staleOrder)));
  } finally {
    await fixture.cleanup();
  }
});

it("accepts a correctly sealed reordered inventory with extras and duplicate artifacts during execution", async () => {
  const fixture = await createSealedFixture();
  try {
    const decoded = JSON.parse(await readFile(fixture.inventoryPath, "utf8"));
    const reordered = resealInventory({ extra: "保留", ...decoded, artifacts: [...decoded.artifacts, ...decoded.artifacts] });
    await writeFile(fixture.inventoryPath, JSON.stringify(reordered));
    let ordinaryExecutions = 0;
    await assert.rejects(executeFullRegression({
      inventoryPath: fixture.inventoryPath,
      collectFingerprint: async () => fixture.fingerprint,
      run: async ({ command, args }) => {
        assert.equal(command, fixture.artifact);
        if (args.includes("--list")) return { stdout: args.includes("--ignored") ? "" : "ordinary: test\n" };
        ordinaryExecutions += 1;
      },
    }), /required long validation target.*resolved to 2 prebuilt binaries/i);
    assert.equal(ordinaryExecutions, 2, "重复 artifacts 必须保留现有执行语义");
  } finally {
    await fixture.cleanup();
  }
});

it("preserves deeply nested JSON extras accepted by the inventory wire format", async () => {
  const fixture = await createSealedFixture();
  try {
    const decoded = JSON.parse(await readFile(fixture.inventoryPath, "utf8"));
    let extra = { note: "保留深层扩展", empty: null };
    for (let depth = 0; depth < 2000; depth += 1) extra = { nested: extra, empty: null };
    const record = resealInventory({ ...decoded, extra });
    const originalJson = JSON.stringify(record);
    const callerRecord = JSON.parse(originalJson);
    const inventory = regression.ArtifactInventory.fromDecodedInventory(callerRecord, {
      sourceRoot: decoded.source_root,
      workspacePaths: {
        workspaceRoot: decoded.workspace_root,
        cargoTargetDir: decoded.cargo_target_dir,
        processTmpDir: decoded.process_tmp_dir,
      },
      fingerprint: fixture.fingerprint,
    });
    assert.equal(JSON.stringify(inventory.toJson()), originalJson);
    let callerExtra = callerRecord.extra;
    let exportedExtra = inventory.toJson().extra;
    for (let depth = 0; depth < 2000; depth += 1) {
      assert.equal(exportedExtra.empty, null);
      callerExtra = callerExtra.nested;
      exportedExtra = exportedExtra.nested;
    }
    callerExtra.note = "修改输入叶子";
    exportedExtra.note = "修改输出叶子";
    assert.equal(JSON.stringify(inventory.toJson()), originalJson);
    await writeFile(fixture.inventoryPath, originalJson);
    let calls = 0;
    await executeFullRegression({
      inventoryPath: fixture.inventoryPath,
      collectFingerprint: async () => fixture.fingerprint,
      run: async ({ args }) => {
        calls += 1;
        return { stdout: args.includes("--list") && !args.includes("--ignored") ? "ordinary: test\n" : "" };
      },
    });
    assert.equal(calls, 6);
  } finally {
    await fixture.cleanup();
  }
});

it("rejects missing identity fields and changed seals before starting any test", async () => {
  const fixture = await createSealedFixture();
  let runs = 0;
  try {
    const original = JSON.parse(await readFile(fixture.inventoryPath, "utf8"));
    const mutations = [
      ["missing schema", (record) => { delete record.schema; }, /malformed, stale, or bound/i],
      ["legacy schema", (record) => { record.schema = "full-regression-artifact-inventory-v1"; }, /malformed, stale, or bound/i],
      ["missing schema version", (record) => { delete record.schema_version; }, /malformed, stale, or bound/i],
      ["string schema version", (record) => { record.schema_version = "1"; }, /malformed, stale, or bound/i],
      ["unknown schema version", (record) => { record.schema_version = 2; }, /malformed, stale, or bound/i],
      ["missing digest", (record) => { delete record.identity_digest; }, /malformed, stale, or bound/i, false],
      ["changed source", (record) => { record.source_root += "-other"; }, /malformed, stale, or bound/i],
      ["changed workspace", (record) => { record.workspace_root += "-other"; }, /malformed, stale, or bound/i],
      ["changed target", (record) => { record.cargo_target_dir += "-other"; }, /malformed, stale, or bound/i],
      ["changed temp", (record) => { record.process_tmp_dir += "-other"; }, /malformed, stale, or bound/i],
      ["changed fingerprint", (record) => { record.source_fingerprint.digest = "b".repeat(64); }, /malformed, stale, or bound/i],
      ["changed fingerprint algorithm", (record) => { record.source_fingerprint.algorithm = "full-regression-source-v1"; }, /malformed, stale, or bound/i],
      ["missing fingerprint algorithm version", (record) => { delete record.source_fingerprint.algorithm_version; }, /malformed, stale, or bound/i],
      ["changed fingerprint algorithm version", (record) => { record.source_fingerprint.algorithm_version = 2; }, /malformed, stale, or bound/i],
      ["missing artifacts", (record) => { delete record.artifacts; }, /malformed, stale, or bound/i],
      ["empty artifacts", (record) => { record.artifacts = []; }, /malformed, stale, or bound/i],
      ["missing artifact label", (record) => { delete record.artifacts[0].label; }, /invalid artifact entry/i],
      ["invalid artifact bytes", (record) => { record.artifacts[0].bytes = -1; }, /invalid artifact entry/i],
      ["invalid artifact digest", (record) => { record.artifacts[0].sha256 = "invalid"; }, /invalid artifact entry/i],
      ["absolute artifact path", (record) => { record.artifacts[0].executable_relative_path = fixture.artifact; }, /invalid artifact entry/i],
      ["escaped artifact path", (record) => { record.artifacts[0].executable_relative_path = "../escaped"; }, /escaped the Cargo target/i],
      ["changed artifact bytes", (record) => { record.artifacts[0].bytes += 1; }, /does not match its sealed inventory/i],
      ["changed artifact hash", (record) => { record.artifacts[0].sha256 = "0".repeat(64); }, /SHA-256 mismatch/i],
      ["unsealed extra field", (record) => { record.extra = "修改"; }, /malformed, stale, or bound/i, false],
      ["unsealed field order", (record) => { delete record.schema; record.schema = original.schema; }, /malformed, stale, or bound/i, false],
    ];
    for (const [label, mutate, errorPattern, reseal = true] of mutations) {
      const record = structuredClone(original);
      mutate(record);
      await writeFile(fixture.inventoryPath, JSON.stringify(reseal ? resealInventory(record) : record));
      await assert.rejects(executeFullRegression({
        inventoryPath: fixture.inventoryPath,
        collectFingerprint: async () => fixture.fingerprint,
        run: async () => { runs += 1; },
      }), errorPattern, label);
      assert.equal(runs, 0, label);
    }
  } finally {
    await fixture.cleanup();
  }
});

it("executes sealed prebuilt test binaries, then preserves doctests and 10-second web coverage", async () => {
  let nowMs = 0;
  const calls = [];
  const fixture = await createSealedFixture();
  try {
    const result = await executeFullRegression({
      cwd: process.cwd(),
      inventoryPath: fixture.inventoryPath,
      cpuCount: 8,
      now: () => nowMs,
      collectFingerprint: async () => fixture.fingerprint,
      run: async (options) => {
        calls.push(options);
        nowMs += 1;
        return { stdout: options.args.includes("--list") && !options.args.includes("--ignored") ? "ordinary::case: test\n" : "", stderr: "" };
      },
    });
    assert.equal(calls.filter((call) => call.command === fixture.artifact).length, 4);
    const ordinaryBinary = calls.find((call) => call.args.includes("ordinary::case"));
    assert.equal(ordinaryBinary.timeoutMs, 10_000);
    assert.deepEqual(ordinaryBinary.args, ["ordinary::case", "--exact", "--test-threads=1"]);
    assert.equal(ordinaryBinary.env.RAYON_NUM_THREADS, "7");
    const longValidation = calls.find((call) => call.command === fixture.artifact && call.args.includes("--ignored") && !call.args.includes("--list"));
    assert.deepEqual(longValidation.args, [
      "lifecycle::year_boundary_keeps_company_operations_and_disclosure_state_authoritative",
      "--exact",
      "--ignored",
      "--test-threads=1",
    ]);
    assert.equal(longValidation.env.RAYON_NUM_THREADS, "7");
    assert.equal(calls.filter((call) => call.command === "cargo" && call.args.includes("--doc")).length, 1);
    assert.equal(calls.find((call) => call.command === "cargo" && call.args.includes("--doc")).timeoutMs, 299_996);
    const web = calls.find((call) => call.args?.some((argument) => argument.endsWith("scripts/run-web-tests.mjs")));
    assert.equal(web.command, process.execPath);
    assert.equal(web.timeoutMs, 10_000);
    assert.equal(result.steps.find((step) => step.label === "Rust prebuilt test binaries").binary_count, 1);
    assert.equal(result.steps.find((step) => step.label === "Rust required long validations").case_count, 1);
  } finally {
    await fixture.cleanup();
  }
});

it("fails fast and aborts in-flight sibling test binaries", async () => {
  const workspaceRoot = await resolveWorkspaceRoot(process.cwd());
  const target = path.join(workspaceRoot, ".tmp", "build-cache", "full-regression");
  const artifactPaths = [0, 1].map((index) => path.join(target, "debug", "deps", `failure-fixture-${process.pid}-${index}`));
  await mkdir(path.dirname(artifactPaths[0]), { recursive: true });
  await Promise.all(artifactPaths.map((artifact) => writeFile(artifact, `binary-${artifact}`)));
  const inventoryPath = path.join(workspaceRoot, ".tmp", "full-regression", `failure-inventory-${process.pid}.json`);
  const fingerprint = { algorithm: "test", algorithm_version: 1, files: [], digest: "b".repeat(64) };
  const artifacts = artifactPaths.map((artifact, index) => JSON.stringify({
    reason: "compiler-artifact",
    package_id: `pkg-${index} 0.1.0`,
    target: { name: `test-${index}`, kind: ["test"], test: true },
    profile: { test: true },
    executable: artifact,
  })).join("\n");
  await buildFullRegressionArtifacts({
    cwd: process.cwd(), inventoryPath,
    collectFingerprint: async () => fingerprint,
    run: async () => ({ stdout: artifacts, stderr: "" }),
  });
  let releaseSiblingStarted;
  const siblingStarted = new Promise((resolve) => { releaseSiblingStarted = resolve; });
  let siblingAborted = false;
  try {
    await assert.rejects(executeFullRegression({
      cwd: process.cwd(),
      inventoryPath,
      cpuCount: 8,
      collectFingerprint: async () => fingerprint,
      run: async (options) => {
        if (options.command === artifactPaths[0]) {
          await siblingStarted;
          throw new Error("injected test failure");
        }
        if (options.command === artifactPaths[1]) {
          releaseSiblingStarted();
          await new Promise((_, reject) => options.signal.addEventListener("abort", () => {
            siblingAborted = true;
            reject(options.signal.reason);
          }, { once: true }));
        }
        throw new Error(`unexpected post-failure command: ${options.command}`);
      },
    }), (error) => {
      assert.match(error.message, /prebuilt Rust test binaries failed/i);
      assert.match(error.message, /pkg-0 0\.1\.0 test:test-0 failed: injected test failure/);
      return true;
    });
    assert.equal(siblingAborted, true);
  } finally {
    await Promise.all([
      ...artifactPaths.map((artifact) => rm(artifact, { force: true })),
      rm(inventoryPath, { force: true }),
    ]);
  }
});

it("rejects changed binary bytes before starting any test", async () => {
  const fixture = await createSealedFixture();
  let runCount = 0;
  try {
    await writeFile(fixture.artifact, "tampered test binn");
    await assert.rejects(executeFullRegression({
      cwd: process.cwd(), inventoryPath: fixture.inventoryPath,
      collectFingerprint: async () => fixture.fingerprint,
      run: async () => { runCount += 1; },
    }), /SHA-256 mismatch/i);
    assert.equal(runCount, 0);
  } finally {
    await fixture.cleanup();
  }
});

it("rejects source drift before starting any test", async () => {
  const fixture = await createSealedFixture();
  let runCount = 0;
  try {
    await assert.rejects(executeFullRegression({
      cwd: process.cwd(), inventoryPath: fixture.inventoryPath,
      collectFingerprint: async () => ({ ...fixture.fingerprint, digest: "c".repeat(64) }),
      run: async () => { runCount += 1; },
    }), /malformed, stale, or bound/i);
    assert.equal(runCount, 0);
  } finally {
    await fixture.cleanup();
  }
});

it("rejects source drift detected after the shared execution phase", async () => {
  const fixture = await createSealedFixture();
  let fingerprintCall = 0;
  try {
    await assert.rejects(executeFullRegression({
      cwd: process.cwd(), inventoryPath: fixture.inventoryPath,
      collectFingerprint: async () => fingerprintCall++ === 0
        ? fixture.fingerprint
        : { ...fixture.fingerprint, digest: "d".repeat(64) },
      run: async () => ({ stdout: "", stderr: "" }),
    }), /source changed during test execution/i);
  } finally {
    await fixture.cleanup();
  }
});

it("rejects inventory paths outside workspace .tmp", async () => {
  await assert.rejects(buildFullRegressionArtifacts({
    cwd: process.cwd(),
    inventoryPath: path.join(path.parse(process.cwd()).root, "tmp", `inventory-${process.pid}.json`),
    collectFingerprint: async () => ({ algorithm: "test", algorithm_version: 1, files: [], digest: "e".repeat(64) }),
    run: async () => { throw new Error("must reject before build"); },
  }), /strict child of workspace \.tmp/i);
});

it("rejects an artifact redirected outside Cargo target through an intermediate symlink", async () => {
  const workspaceRoot = await resolveWorkspaceRoot(process.cwd());
  const target = path.join(workspaceRoot, ".tmp", "build-cache", "full-regression");
  const linkDirectory = path.join(target, `symlink-chain-${process.pid}-${Date.now()}`);
  const artifact = path.join(linkDirectory, "test-binary");
  const escapedDirectory = path.join(workspaceRoot, ".tmp", `escaped-artifact-${process.pid}-${Date.now()}`);
  const inventoryPath = path.join(workspaceRoot, ".tmp", "full-regression", `symlink-inventory-${process.pid}-${Date.now()}.json`);
  const fingerprint = { algorithm: "test", algorithm_version: 1, files: [], digest: "f".repeat(64) };
  await mkdir(linkDirectory, { recursive: true });
  await writeFile(artifact, "sealed symlink binary");
  try {
    await buildFullRegressionArtifacts({
      cwd: process.cwd(), inventoryPath,
      collectFingerprint: async () => fingerprint,
      run: async () => ({ stdout: `${JSON.stringify({ reason: "compiler-artifact", package_id: "fixture", target: { name: "fixture", kind: ["test"], test: true }, profile: { test: true }, executable: artifact })}\n`, stderr: "" }),
    });
    await rm(linkDirectory, { recursive: true });
    await mkdir(escapedDirectory);
    await writeFile(path.join(escapedDirectory, "test-binary"), "sealed symlink binary");
    await symlink(escapedDirectory, linkDirectory, "dir");
    await assert.rejects(executeFullRegression({
      cwd: process.cwd(), inventoryPath,
      collectFingerprint: async () => fingerprint,
      run: async () => { throw new Error("must reject before execution"); },
    }), /escaped the Cargo target/i);
  } finally {
    await Promise.all([
      rm(linkDirectory, { recursive: true, force: true }),
      rm(escapedDirectory, { recursive: true, force: true }),
      rm(inventoryPath, { force: true }),
    ]);
  }
});

it("does not publish an inventory when source changes during the build", async () => {
  const workspaceRoot = await resolveWorkspaceRoot(process.cwd());
  const target = path.join(workspaceRoot, ".tmp", "build-cache", "full-regression");
  const artifact = path.join(target, "debug", "deps", `drift-fixture-${process.pid}`);
  const inventoryPath = path.join(workspaceRoot, ".tmp", "full-regression", `drift-inventory-${process.pid}.json`);
  await mkdir(path.dirname(artifact), { recursive: true });
  await writeFile(artifact, "binary");
  let fingerprintCall = 0;
  try {
    await assert.rejects(buildFullRegressionArtifacts({
      cwd: process.cwd(), inventoryPath,
      collectFingerprint: async () => ({ algorithm: "test", algorithm_version: 1, files: [], digest: `${++fingerprintCall}`.padStart(64, "0") }),
      run: async () => ({ stdout: `${JSON.stringify({ reason: "compiler-artifact", package_id: "fixture", target: { name: "fixture", kind: ["test"], test: true }, profile: { test: true }, executable: artifact })}\n`, stderr: "" }),
    }), /source changed during.*build/i);
    await assert.rejects(readFile(inventoryPath), /ENOENT/);
  } finally {
    await Promise.all([rm(artifact, { force: true }), rm(inventoryPath, { force: true })]);
  }
});

it("rejects a workspace .tmp symlink before creating cache directories", async () => {
  const workspaceRoot = await resolveWorkspaceRoot(process.cwd());
  const fixtureRoot = await mkdtemp(path.join(workspaceRoot, ".tmp", "workspace-path-test-"));
  const sourceRoot = path.join(fixtureRoot, "source");
  const escaped = path.join(fixtureRoot, "escaped");
  await Promise.all([mkdir(path.join(sourceRoot, ".git"), { recursive: true }), mkdir(escaped)]);
  await symlink(escaped, path.join(sourceRoot, ".tmp"), "dir");
  try {
    await assert.rejects(prepareWorkspacePaths({ sourceRoot, scope: "test" }), /symbolic link|symlink/i);
  } finally {
    await rm(fixtureRoot, { recursive: true, force: true });
  }
});

it("gives cold build and test execution independent five-minute hard deadlines", async () => {
  let nowMs = 1_000;
  const calls = [];
  const result = await runFullRegression({
    now: () => nowMs,
    run: async (options) => {
      calls.push(options);
      nowMs += 120_000;
    },
    cwd: process.cwd(),
  });
  assert.deepEqual(calls.map((call) => call.timeoutMs), [300_000, 300_000]);
  assert.deepEqual(calls.map((call) => call.args[1]), ["--internal-build", "--internal-execute"]);
  assert.equal(result.wall_ms, 240_000);
  assert.deepEqual(result.phases.map((phase) => phase.wall_ms), [120_000, 120_000]);
});

it("wraps an explicit phase around its complete internal workflow with one external five-minute deadline", async () => {
  const calls = [];
  await runFullRegressionPhase({
    mode: "build",
    cwd: process.cwd(),
    inventoryPath: "/workspace/.tmp/inventory.json",
    run: async (options) => { calls.push(options); },
  });
  assert.equal(calls.length, 1);
  assert.equal(calls[0].timeoutMs, 300_000);
  assert.equal(calls[0].env.STOCK_GAME_FULL_REGRESSION_INTERNAL_PHASE, "1");
  assert.deepEqual(calls[0].args.slice(1), ["--internal-build", "--inventory", "/workspace/.tmp/inventory.json"]);
});

it("validates explicit build and execute CLI modes", () => {
  assert.deepEqual(parseFullRegressionArgs(["build", "--inventory", "/workspace/.tmp/inventory.json"]), {
    mode: "build", inventoryPath: "/workspace/.tmp/inventory.json",
  });
  assert.deepEqual(parseFullRegressionArgs(["execute", "--inventory", "/workspace/.tmp/inventory.json"]), {
    mode: "execute", inventoryPath: "/workspace/.tmp/inventory.json",
  });
  assert.deepEqual(parseFullRegressionArgs([]), { mode: "all", inventoryPath: undefined });
  assert.deepEqual(parseFullRegressionArgs(["--internal-build"]), { mode: "build", inventoryPath: undefined, internal: true });
  assert.throws(() => parseFullRegressionArgs(["unknown"]), /usage:/i);
  assert.throws(() => parseFullRegressionArgs(["build"]), /--inventory/i);
});

it("rejects direct invocation of an unsupervised internal phase worker", async () => {
  await assert.rejects(main(["--internal-build"], {}), /external five-minute supervisor/i);
});
