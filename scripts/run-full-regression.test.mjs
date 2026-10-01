import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdir, mkdtemp, readFile, readdir, rm, symlink, writeFile } from "node:fs/promises";
import { it } from "node:test";

import path from "node:path";

import {
  buildFullRegressionArtifacts,
  buildRustTestExecutionPolicy,
  collectFullRegressionSourceFingerprint,
  executeFullRegression,
  FULL_REGRESSION_SOURCE_INPUTS,
  fullRegressionSteps,
  main,
  parseFullRegressionArgs,
  parseCargoTestExecutables,
  runFullRegression,
  runFullRegressionPhase,
} from "./run-full-regression.mjs";
import { prepareWorkspacePaths, resolveWorkspaceRoot } from "./workspace-paths.mjs";
import { runBoundedCommand } from "./run-with-deadline.mjs";

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
    "- name: wasm-pack build (web-wasm)",
    "- name: Verify WASM shared-memory threading contract",
    "- name: Verify copied WASM shared-memory threading contract",
    "- name: pnpm --filter web build",
    "- name: Build sealed full-regression artifacts",
    "- name: Execute sealed full regression",
  ];
  const positions = orderedSteps.map((step) => workflow.indexOf(step));
  assert.ok(positions.every((position) => position >= 0), "CI must retain the real WASM, Web, and Rust regression steps");
  assert.ok(positions.every((position, index) => index === 0 || position > positions[index - 1]), "CI must build real WASM and Web assets before Cargo compiles Tauri tests");
  for (const platform of ["Windows", "Linux"]) {
    const copyPosition = workflow.indexOf(`- name: Copy wasm pkg -> apps/web/wasm-pkg (${platform})`);
    assert.ok(copyPosition > positions[1] && copyPosition < positions[2], `${platform} must copy the verified WASM before Web build`);
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
  assert.match(ready, /if: always\(\) && \(steps\.sealed-build\.outcome == 'success' \|\| steps\.sealed-build\.outcome == 'failure'\)/);
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

async function createSealedFixture({ fingerprint = { algorithm: "test", files: [], digest: "a".repeat(64) } } = {}) {
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
      collectFingerprint: async () => ({ algorithm: "test", files: [], digest: "a".repeat(64) }),
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
      collectFingerprint: async () => ({ algorithm: "test", files: [], digest: "a".repeat(64) }),
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
      collectFingerprint: async () => ({ algorithm: "test", files: [], digest: "a".repeat(64) }),
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
    assert.equal(inventory.schema, "full-regression-artifact-inventory-v1");
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
        return { stdout: "", stderr: "" };
      },
    });
    assert.equal(calls.filter((call) => call.command === fixture.artifact).length, 2);
    const ordinaryBinary = calls.find((call) => call.command === fixture.artifact && !call.args.includes("--ignored"));
    assert.equal(ordinaryBinary.timeoutMs, 300_000);
    assert.deepEqual(ordinaryBinary.args, ["--test-threads=6"]);
    assert.equal(ordinaryBinary.env.RAYON_NUM_THREADS, "2");
    const longValidation = calls.find((call) => call.command === fixture.artifact && call.args.includes("--ignored"));
    assert.deepEqual(longValidation.args, [
      "lifecycle::year_boundary_keeps_company_operations_and_disclosure_state_authoritative",
      "--exact",
      "--ignored",
      "--test-threads=1",
    ]);
    assert.equal(longValidation.env.RAYON_NUM_THREADS, "7");
    assert.equal(calls.filter((call) => call.command === "cargo" && call.args.includes("--doc")).length, 1);
    assert.equal(calls.find((call) => call.command === "cargo" && call.args.includes("--doc")).timeoutMs, 299_998);
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
  const fingerprint = { algorithm: "test", files: [], digest: "b".repeat(64) };
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
    collectFingerprint: async () => ({ algorithm: "test", files: [], digest: "e".repeat(64) }),
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
  const fingerprint = { algorithm: "test", files: [], digest: "f".repeat(64) };
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
      collectFingerprint: async () => ({ algorithm: "test", files: [], digest: `${++fingerprintCall}`.padStart(64, "0") }),
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
