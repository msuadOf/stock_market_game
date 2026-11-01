import assert from "node:assert/strict";

import { execFileSync } from "node:child_process";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { it } from "node:test";

import path from "node:path";

import { buildRustTestExecutionPolicy, collectFullRegressionSourceFingerprint, FULL_REGRESSION_SOURCE_INPUTS, fullRegressionSteps, parseCargoTestExecutables } from "./run-full-regression.mjs";
import { resolveWorkspaceRoot } from "./workspace-paths.mjs";

import * as regression from "./run-full-regression.mjs";

import { CARGO_BUILD_JOBS_ENV } from "./full-regression-test-support.mjs";

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
  const { [CARGO_BUILD_JOBS_ENV]: ignoredCargoBuildJobs, ...fixtureEnv } = process.env;
  const hostile = { ...fixtureEnv, CARGO_TARGET_DIR: "/tmp/external-target", TMPDIR: "/tmp", TMP: "/tmp", TEMP: "/tmp" };
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
    "script-test-batch",
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
    // 批 9 A7：rayon_threads 分量退役——CPU 预算全额给 test_threads。
    worker_budgets: Array.from({ length: 8 }, () => ({ cpu_budget: 16, test_threads: 16 })),
    aggregate_configured_thread_budget: 128,
  });
  const small = buildRustTestExecutionPolicy(4, 40);
  assert.equal(small.max_concurrent_binaries, 2);
  assert.deepEqual(small.worker_budgets, [
    { cpu_budget: 2, test_threads: 2 },
    { cpu_budget: 2, test_threads: 2 },
  ]);
  for (let cpuCount = 2; cpuCount <= 129; cpuCount += 1) {
    const policy = buildRustTestExecutionPolicy(cpuCount, 80);
    assert.equal(policy.aggregate_configured_thread_budget, cpuCount, `CPU ${cpuCount} is misbudgeted by ${JSON.stringify(policy)}`);
    assert.ok(policy.max_concurrent_binaries <= 8);
    assert.ok(policy.worker_budgets.every((budget) => budget.test_threads === budget.cpu_budget));
  }
  assert.deepEqual(buildRustTestExecutionPolicy(5, 80).worker_budgets.map((budget) => budget.cpu_budget), [3, 2]);
  const singleCpu = buildRustTestExecutionPolicy(1, 80);
  assert.deepEqual(singleCpu.worker_budgets, [{ cpu_budget: 1, test_threads: 1 }]);
  assert.equal(singleCpu.aggregate_configured_thread_budget, 1, "批 9 A7：rayon 最低 1 线程的加计退役——1 CPU 预算如实上报 1");
});

it("fingerprints Rust, Web, package-manager, and deadline-runner inputs", () => {
  for (const required of [
    "Cargo.lock",
    "packages/engine",
    "package.json",
    "pnpm-lock.yaml",
    "apps/web/package.json",
    "apps/web/src",
    "scripts",
  ]) assert.ok(FULL_REGRESSION_SOURCE_INPUTS.includes(required), `missing fingerprint input ${required}`);
});

it("ignores only Tauri-generated schemas while detecting changes to real desktop inputs", async () => {
  const workspaceRoot = await resolveWorkspaceRoot(process.cwd());
  const sourceRoot = await mkdtemp(path.join(workspaceRoot, ".tmp", "full-regression-source-"));
  const directoryInputs = new Set([
    ".cargo", "packages/engine", "packages/engine-gpu", "apps/server",
    "apps/web/src", "apps/web-wasm", "apps/desktop/src-tauri", "scripts",
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
    const nestedScript = path.join(sourceRoot, "scripts", "simulation", "nested.test.mjs");
    await mkdir(path.dirname(nestedScript), { recursive: true });
    await writeFile(nestedScript, "test source");
    const withScript = await collectFullRegressionSourceFingerprint(sourceRoot);
    assert.notEqual(withScript.digest, before.digest, "新增深层 scripts 测试必须进入指纹");
    await writeFile(nestedScript, "changed test source");
    assert.notEqual((await collectFullRegressionSourceFingerprint(sourceRoot)).digest, withScript.digest);
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
    // 批 9 A10 收尾：check-wasm-threading 步骤退役——复制后仍经
    // check-web-release-wasm 校验。
    const copyPosition = plan.commands.findIndex(({ action }) => action === "copy-wasm");
    const uiPosition = plan.commands.findIndex(({ args }) => args[0].endsWith("vite.js"));
    // 批 9 A10 收尾：复制后校验改由 dist 级 check-web-release-wasm 承载
    ///（wasm-pkg 级 threading 检查退役）。
    const distCheck = plan.commands.findIndex(({ args }) => args[1] === path.join(plan.frontend, "dist"));
    assert.ok(wasmPosition >= 0 && copyPosition > wasmPosition && uiPosition > copyPosition && distCheck > uiPosition,
      `${host} must build real WASM before copying, building the UI and verifying dist`);
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
