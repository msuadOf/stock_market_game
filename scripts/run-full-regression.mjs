#!/usr/bin/env node
import { createHash } from "node:crypto";
import fsp from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { COMMAND_CLEANUP_RESERVE_MAX_MS, LONG_VALIDATION_MAX_MS, ORDINARY_TEST_MAX_MS, runBoundedCommand } from "./run-with-deadline.mjs";
import { prepareWorkspacePaths, validateWorkspaceOutputPath } from "./workspace-paths.mjs";

const DEFAULT_REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const MAX_CONCURRENT_RUST_TEST_BINARIES = 8;
const MIN_CPU_BUDGET_PER_TEST_BINARY = 4;
const FULL_REGRESSION_INVENTORY_SCHEMA = "full-regression-artifact-inventory-v1";
const FULL_REGRESSION_SOURCE_ALGORITHM = "full-regression-source-v1";
const FULL_REGRESSION_INTERNAL_PHASE_ENV = "STOCK_GAME_FULL_REGRESSION_INTERNAL_PHASE";
const REQUIRED_LONG_VALIDATIONS = [
  {
    targetLabel: "test:company_scenarios",
    filter: "lifecycle::year_boundary_keeps_company_operations_and_disclosure_state_authoritative",
  },
];
export const FULL_REGRESSION_SOURCE_INPUTS = [
  ".cargo",
  "Cargo.lock",
  "Cargo.toml",
  "package.json",
  "pnpm-lock.yaml",
  "pnpm-workspace.yaml",
  "rust-toolchain.toml",
  "packages/engine",
  "packages/engine-gpu",
  "apps/server",
  "apps/web/package.json",
  "apps/web/src",
  "apps/web/vite.config.ts",
  "apps/web-wasm",
  "apps/desktop/src-tauri",
  "scripts/run-full-regression.mjs",
  "scripts/run-long-validation.mjs",
  "scripts/run-web-tests.mjs",
  "scripts/run-with-deadline.mjs",
  "scripts/workspace-paths.mjs",
];

export function fullRegressionSteps(cpuCount = os.availableParallelism(), repoRoot = DEFAULT_REPO_ROOT, baseEnv = process.env, workspaceRoot = repoRoot) {
  if (!Number.isInteger(cpuCount) || cpuCount <= 0) throw new Error("full regression requires a positive CPU count");
  const resolvedRoot = path.resolve(repoRoot);
  const resolvedWorkspaceRoot = path.resolve(workspaceRoot);
  const cargoTargetDir = path.join(resolvedWorkspaceRoot, ".tmp", "build-cache", "full-regression");
  const processTmpDir = path.join(resolvedWorkspaceRoot, ".tmp", "process-tmp", "full-regression");
  const workspaceEnv = {
    ...baseEnv,
    CARGO_TARGET_DIR: cargoTargetDir,
    TMPDIR: processTmpDir,
    TMP: processTmpDir,
    TEMP: processTmpDir,
  };
  const cargoEnv = { ...workspaceEnv, CARGO_BUILD_JOBS: String(cpuCount), RAYON_NUM_THREADS: String(cpuCount) };
  return [
    { kind: "rust-test-build", label: "Rust test build", command: "cargo", args: ["test", "--workspace", "--no-run", "--message-format=json-render-diagnostics"], env: cargoEnv, captureOutput: true },
    { kind: "rust-test-binaries", label: "Rust prebuilt test binaries", env: workspaceEnv, cpuCount },
    {
      kind: "rust-doctests",
      label: "Rust workspace doctests",
      command: "cargo",
      args: ["test", "--workspace", "--doc", "--no-fail-fast", "--", `--test-threads=${Math.max(1, cpuCount - 1)}`],
      env: { ...cargoEnv, RAYON_NUM_THREADS: "1" },
    },
    {
      kind: "ordinary-command",
      label: "web ordinary tests",
      command: process.execPath,
      args: [path.join(resolvedRoot, "scripts", "run-web-tests.mjs")],
      env: workspaceEnv,
    },
  ];
}

export function parseCargoTestExecutables(stdout, cargoTargetDir) {
  if (typeof stdout !== "string") throw new Error("Cargo test build did not return JSON output");
  const targetRoot = path.resolve(cargoTargetDir);
  const executables = new Map();
  for (const [index, line] of stdout.split(/\r?\n/).entries()) {
    if (line.trim().length === 0) continue;
    let message;
    try {
      message = JSON.parse(line);
    } catch (error) {
      throw new Error(`Cargo test build emitted malformed JSON on line ${index + 1}: ${error.message}`);
    }
    if (message.reason !== "compiler-artifact"
      || message.profile?.test !== true
      || message.target?.test !== true
      || typeof message.executable !== "string") continue;
    const executable = path.resolve(message.executable);
    const relative = path.relative(targetRoot, executable);
    if (!relative || relative.startsWith("..") || path.isAbsolute(relative)) {
      throw new Error(`Cargo test executable is outside the workspace Cargo target: ${executable}`);
    }
    const kind = Array.isArray(message.target.kind) ? message.target.kind.join("+") : "unknown";
    executables.set(executable, {
      executable,
      label: `${message.package_id ?? "unknown-package"} ${kind}:${message.target.name ?? "unknown-target"}`,
    });
  }
  const sorted = [...executables.values()].sort((left, right) => left.executable.localeCompare(right.executable));
  if (sorted.length === 0) throw new Error("Cargo test build produced no prebuilt test executables");
  return sorted;
}

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

async function collectInputFiles(sourceRoot, relativePath, files) {
  const absolutePath = path.join(sourceRoot, relativePath);
  const stat = await fsp.lstat(absolutePath);
  if (stat.isSymbolicLink()) throw new Error(`full regression source input must not be a symbolic link: ${absolutePath}`);
  if (stat.isDirectory()) {
    const entries = await fsp.readdir(absolutePath);
    entries.sort();
    for (const entry of entries) await collectInputFiles(sourceRoot, path.join(relativePath, entry), files);
    return;
  }
  if (!stat.isFile()) throw new Error(`full regression source input must be a regular file or directory: ${absolutePath}`);
  const bytes = await fsp.readFile(absolutePath);
  files.push({ path: relativePath.split(path.sep).join("/"), bytes: bytes.length, sha256: sha256(bytes) });
}

export async function collectFullRegressionSourceFingerprint(sourceRoot) {
  const resolvedSourceRoot = await fsp.realpath(path.resolve(sourceRoot));
  const files = [];
  for (const relativePath of FULL_REGRESSION_SOURCE_INPUTS) {
    await collectInputFiles(resolvedSourceRoot, relativePath, files);
  }
  files.sort((left, right) => left.path.localeCompare(right.path));
  const state = { algorithm: FULL_REGRESSION_SOURCE_ALGORITHM, files };
  return { ...state, digest: sha256(JSON.stringify(state)) };
}

async function sealTestArtifacts(artifacts, workspacePaths) {
  const sealed = [];
  for (const artifact of artifacts) {
    const executable = await fsp.realpath(artifact.executable);
    const relativeToTarget = path.relative(workspacePaths.cargoTargetDir, executable);
    if (!relativeToTarget || relativeToTarget.startsWith("..") || path.isAbsolute(relativeToTarget)) {
      throw new Error(`prebuilt Rust test executable escaped the Cargo target: ${executable}`);
    }
    const stat = await fsp.lstat(executable);
    if (!stat.isFile() || stat.isSymbolicLink()) throw new Error(`prebuilt Rust test executable must be a regular non-symlink file: ${executable}`);
    const bytes = await fsp.readFile(executable);
    sealed.push({
      label: artifact.label,
      executable_relative_path: path.relative(workspacePaths.workspaceRoot, executable).split(path.sep).join("/"),
      bytes: bytes.length,
      sha256: sha256(bytes),
    });
  }
  return sealed;
}

function inventoryDigest(inventory) {
  const { identity_digest: ignored, ...state } = inventory;
  return sha256(JSON.stringify(state));
}

async function writeJsonAtomically(filePath, value) {
  const temporaryPath = `${filePath}.${process.pid}.${Date.now()}.tmp`;
  try {
    await fsp.writeFile(temporaryPath, `${JSON.stringify(value, null, 2)}\n`, { flag: "wx" });
    await fsp.rename(temporaryPath, filePath);
  } finally {
    await fsp.rm(temporaryPath, { force: true });
  }
}

async function prepareInventoryPath(workspaceRoot, inventoryPath) {
  const candidate = path.resolve(inventoryPath ?? path.join(workspaceRoot, ".tmp", "full-regression", "artifact-inventory.json"));
  const parent = path.dirname(candidate);
  await validateWorkspaceOutputPath(workspaceRoot, parent);
  await fsp.mkdir(parent, { recursive: true });
  const validatedParent = await validateWorkspaceOutputPath(workspaceRoot, parent);
  return path.join(validatedParent, path.basename(candidate));
}

async function readAndValidateInventory(inventoryPath, sourceRoot, workspacePaths, fingerprint) {
  const stat = await fsp.lstat(inventoryPath);
  if (!stat.isFile() || stat.isSymbolicLink()) throw new Error(`full regression inventory must be a regular non-symlink file: ${inventoryPath}`);
  let inventory;
  try {
    inventory = JSON.parse(await fsp.readFile(inventoryPath, "utf8"));
  } catch (error) {
    throw new Error(`full regression artifact inventory is not valid JSON at ${inventoryPath}: ${error.message}`, { cause: error });
  }
  if (inventory?.schema !== FULL_REGRESSION_INVENTORY_SCHEMA
    || inventory.source_root !== sourceRoot
    || inventory.workspace_root !== workspacePaths.workspaceRoot
    || inventory.cargo_target_dir !== workspacePaths.cargoTargetDir
    || inventory.process_tmp_dir !== workspacePaths.processTmpDir
    || inventory.source_fingerprint?.digest !== fingerprint.digest
    || inventory.identity_digest !== inventoryDigest(inventory)
    || !Array.isArray(inventory.artifacts)
    || inventory.artifacts.length === 0) {
    throw new Error("full regression artifact inventory is malformed, stale, or bound to another source/workspace");
  }
  const artifacts = [];
  for (const entry of inventory.artifacts) {
    if (typeof entry?.label !== "string" || entry.label.length === 0
      || typeof entry?.executable_relative_path !== "string" || entry.executable_relative_path.length === 0
      || path.isAbsolute(entry.executable_relative_path)
      || !Number.isSafeInteger(entry.bytes) || entry.bytes < 0
      || typeof entry.sha256 !== "string" || !/^[a-f0-9]{64}$/.test(entry.sha256)) {
      throw new Error("full regression artifact inventory contains an invalid artifact entry");
    }
    const lexicalExecutable = path.resolve(workspacePaths.workspaceRoot, ...entry.executable_relative_path.split("/"));
    const lexicalRelativeToTarget = path.relative(workspacePaths.cargoTargetDir, lexicalExecutable);
    if (!lexicalRelativeToTarget || lexicalRelativeToTarget.startsWith("..") || path.isAbsolute(lexicalRelativeToTarget)) {
      throw new Error(`full regression artifact inventory executable escaped the Cargo target: ${lexicalExecutable}`);
    }
    const lexicalStat = await fsp.lstat(lexicalExecutable);
    if (lexicalStat.isSymbolicLink()) throw new Error(`full regression artifact must not be a symbolic link: ${lexicalExecutable}`);
    const executable = await fsp.realpath(lexicalExecutable);
    const relativeToTarget = path.relative(workspacePaths.cargoTargetDir, executable);
    if (!relativeToTarget || relativeToTarget.startsWith("..") || path.isAbsolute(relativeToTarget)) {
      throw new Error(`full regression artifact inventory executable escaped the Cargo target: ${executable}`);
    }
    const stat = await fsp.lstat(executable);
    if (!stat.isFile() || stat.isSymbolicLink() || stat.size !== entry.bytes) {
      throw new Error(`full regression artifact does not match its sealed inventory: ${executable}`);
    }
    const digest = sha256(await fsp.readFile(executable));
    if (digest !== entry.sha256) throw new Error(`full regression artifact SHA-256 mismatch: ${executable}`);
    artifacts.push({ executable, label: entry.label });
  }
  return { inventory, artifacts };
}

export function buildRustTestExecutionPolicy(availableCpuCount, binaryCount) {
  if (!Number.isInteger(availableCpuCount) || availableCpuCount <= 0) throw new Error("Rust test execution requires a positive CPU count");
  if (!Number.isInteger(binaryCount) || binaryCount <= 0) throw new Error("Rust test execution requires at least one prebuilt binary");
  const maxConcurrentBinaries = Math.min(
    MAX_CONCURRENT_RUST_TEST_BINARIES,
    binaryCount,
    Math.max(1, Math.floor(availableCpuCount / MIN_CPU_BUDGET_PER_TEST_BINARY)),
  );
  const cpuBudgetPerBinary = Math.max(1, Math.floor(availableCpuCount / maxConcurrentBinaries));
  const rayonThreadsPerBinary = Math.max(1, Math.floor(cpuBudgetPerBinary / 4));
  const testThreadsPerBinary = cpuBudgetPerBinary === 1
    ? 1
    : cpuBudgetPerBinary - rayonThreadsPerBinary;
  const effectiveBudgetPerBinary = cpuBudgetPerBinary === 1
    ? 1
    : testThreadsPerBinary + rayonThreadsPerBinary;
  return {
    available_cpu_count: availableCpuCount,
    binary_count: binaryCount,
    max_concurrent_binaries: maxConcurrentBinaries,
    cpu_budget_per_binary: cpuBudgetPerBinary,
    test_threads_per_binary: testThreadsPerBinary,
    rayon_threads_per_binary: rayonThreadsPerBinary,
    aggregate_configured_thread_budget: maxConcurrentBinaries * effectiveBudgetPerBinary,
  };
}

async function executeRustTestBinaries({ artifacts, policy, run, cwd, env, remainingMs, now }) {
  let nextIndex = 0;
  let stop = false;
  const failures = [];
  const results = new Array(artifacts.length);
  const controller = new AbortController();
  async function worker() {
    while (!stop) {
      const index = nextIndex;
      nextIndex += 1;
      if (index >= artifacts.length) return;
      const artifact = artifacts[index];
      const timeoutMs = remainingMs(`Rust test binary ${artifact.label}`);
      const startedAt = now();
      try {
        await run({
          command: artifact.executable,
          args: [`--test-threads=${policy.test_threads_per_binary}`],
          env: { ...env, RAYON_NUM_THREADS: String(policy.rayon_threads_per_binary) },
          cwd,
          timeoutMs,
          cleanupReserveMs: Math.min(COMMAND_CLEANUP_RESERVE_MAX_MS, Math.max(1, timeoutMs - 1)),
          signal: controller.signal,
        });
        results[index] = { label: artifact.label, wall_ms: now() - startedAt };
      } catch (error) {
        stop = true;
        if (!controller.signal.aborted) controller.abort(error);
        failures.push(new Error(`${artifact.label} failed: ${error.message}`, { cause: error }));
      }
    }
  }
  await Promise.all(Array.from({ length: policy.max_concurrent_binaries }, () => worker()));
  if (failures.length > 0) throw new AggregateError(failures, `${failures.length} prebuilt Rust test binaries failed`);
  return results;
}

async function executeRequiredLongValidations({ artifacts, policy, run, cwd, env, remainingMs, now }) {
  const results = [];
  for (const validation of REQUIRED_LONG_VALIDATIONS) {
    const matches = artifacts.filter((artifact) => artifact.label.includes(validation.targetLabel));
    if (matches.length !== 1) {
      throw new Error(`required long validation target ${validation.targetLabel} resolved to ${matches.length} prebuilt binaries`);
    }
    const artifact = matches[0];
    const timeoutMs = remainingMs(`required long validation ${validation.filter}`);
    const startedAt = now();
    await run({
      command: artifact.executable,
      args: [validation.filter, "--exact", "--ignored", `--test-threads=${policy.testThreadsPerBinary}`],
      env: { ...env, RAYON_NUM_THREADS: String(policy.rayonThreadsPerBinary) },
      cwd,
      timeoutMs,
      cleanupReserveMs: Math.min(COMMAND_CLEANUP_RESERVE_MAX_MS, Math.max(1, timeoutMs - 1)),
    });
    results.push({ filter: validation.filter, label: artifact.label, wall_ms: now() - startedAt });
  }
  return results;
}

function phaseRemainingMs(startedAt, now, context) {
  const remaining = Math.floor(startedAt + LONG_VALIDATION_MAX_MS - now());
  if (remaining <= 1) throw new Error(`full regression phase exhausted its ${LONG_VALIDATION_MAX_MS}ms deadline before ${context}`);
  return remaining;
}

function phaseCommandOptions(step, cwd, timeoutMs) {
  return {
    command: step.command,
    args: step.args,
    env: step.env,
    cwd,
    timeoutMs,
    cleanupReserveMs: Math.min(COMMAND_CLEANUP_RESERVE_MAX_MS, timeoutMs - 1),
  };
}

export async function buildFullRegressionArtifacts({
  run = runBoundedCommand,
  now = Date.now,
  cpuCount = os.availableParallelism(),
  cwd = DEFAULT_REPO_ROOT,
  inventoryPath,
  collectFingerprint = collectFullRegressionSourceFingerprint,
  log = () => undefined,
} = {}) {
  const sourceRoot = await fsp.realpath(path.resolve(cwd));
  const workspacePaths = await prepareWorkspacePaths({ sourceRoot, scope: "full-regression" });
  const resolvedInventoryPath = await prepareInventoryPath(workspacePaths.workspaceRoot, inventoryPath);
  const buildStep = fullRegressionSteps(cpuCount, sourceRoot, process.env, workspacePaths.workspaceRoot)
    .find((step) => step.kind === "rust-test-build");
  const startedAt = now();
  const before = await collectFingerprint(sourceRoot);
  const output = await run({
    ...phaseCommandOptions(buildStep, sourceRoot, phaseRemainingMs(startedAt, now, buildStep.label)),
    captureOutput: true,
  });
  const artifacts = parseCargoTestExecutables(output?.stdout, workspacePaths.cargoTargetDir);
  const after = await collectFingerprint(sourceRoot);
  if (before.digest !== after.digest) {
    throw new Error("full regression source changed during the cold build; refusing to publish an artifact inventory");
  }
  phaseRemainingMs(startedAt, now, "artifact sealing");
  const sealedArtifacts = await sealTestArtifacts(artifacts, workspacePaths);
  const inventory = {
    schema: FULL_REGRESSION_INVENTORY_SCHEMA,
    source_root: sourceRoot,
    workspace_root: workspacePaths.workspaceRoot,
    cargo_target_dir: workspacePaths.cargoTargetDir,
    process_tmp_dir: workspacePaths.processTmpDir,
    source_fingerprint: before,
    build: {
      argv: [buildStep.command, ...buildStep.args],
      available_cpu_count: cpuCount,
      wall_ms: now() - startedAt,
    },
    artifacts: sealedArtifacts,
  };
  inventory.identity_digest = inventoryDigest(inventory);
  phaseRemainingMs(startedAt, now, "atomic artifact inventory publication");
  await writeJsonAtomically(resolvedInventoryPath, inventory);
  const wallMs = now() - startedAt;
  if (wallMs > LONG_VALIDATION_MAX_MS) {
    await fsp.rm(resolvedInventoryPath, { force: true });
    throw new Error(`full regression cold build exceeded its ${LONG_VALIDATION_MAX_MS}ms deadline`);
  }
  const result = { phase: "build", wall_ms: wallMs, artifact_count: sealedArtifacts.length, inventory_path: resolvedInventoryPath };
  log(JSON.stringify(result));
  return { ...result, inventory };
}

export async function executeFullRegression({
  run = runBoundedCommand,
  now = Date.now,
  cpuCount = os.availableParallelism(),
  cwd = DEFAULT_REPO_ROOT,
  inventoryPath,
  collectFingerprint = collectFullRegressionSourceFingerprint,
  log = () => undefined,
} = {}) {
  const sourceRoot = await fsp.realpath(path.resolve(cwd));
  const workspacePaths = await prepareWorkspacePaths({ sourceRoot, scope: "full-regression" });
  const resolvedInventoryPath = await prepareInventoryPath(workspacePaths.workspaceRoot, inventoryPath);
  const startedAt = now();
  const fingerprint = await collectFingerprint(sourceRoot);
  phaseRemainingMs(startedAt, now, "artifact inventory validation");
  const { artifacts } = await readAndValidateInventory(resolvedInventoryPath, sourceRoot, workspacePaths, fingerprint);
  const steps = fullRegressionSteps(cpuCount, sourceRoot, process.env, workspacePaths.workspaceRoot);
  const binaryStep = steps.find((step) => step.kind === "rust-test-binaries");
  const doctestStep = steps.find((step) => step.kind === "rust-doctests");
  const webStep = steps.find((step) => step.kind === "ordinary-command");
  const completed = [];
  const remainingMs = (context) => phaseRemainingMs(startedAt, now, context);

  const policy = buildRustTestExecutionPolicy(binaryStep.cpuCount, artifacts.length);
  let stepStartedAt = now();
  await executeRustTestBinaries({ artifacts, policy, run, cwd: sourceRoot, env: binaryStep.env, remainingMs, now });
  completed.push({ label: binaryStep.label, wall_ms: now() - stepStartedAt, binary_count: artifacts.length, ...policy });

  stepStartedAt = now();
  const longValidationResults = await executeRequiredLongValidations({
    artifacts,
    policy: {
      testThreadsPerBinary: policy.test_threads_per_binary,
      rayonThreadsPerBinary: policy.rayon_threads_per_binary,
    },
    run,
    cwd: sourceRoot,
    env: binaryStep.env,
    remainingMs,
    now,
  });
  completed.push({
    label: "Rust required long validations",
    wall_ms: now() - stepStartedAt,
    case_count: longValidationResults.length,
    cases: longValidationResults,
  });

  stepStartedAt = now();
  let timeoutMs = remainingMs(doctestStep.label);
  await run(phaseCommandOptions(doctestStep, sourceRoot, timeoutMs));
  completed.push({ label: doctestStep.label, wall_ms: now() - stepStartedAt });

  stepStartedAt = now();
  timeoutMs = Math.min(ORDINARY_TEST_MAX_MS, remainingMs(webStep.label));
  await run(phaseCommandOptions(webStep, sourceRoot, timeoutMs));
  completed.push({ label: webStep.label, wall_ms: now() - stepStartedAt });

  const after = await collectFingerprint(sourceRoot);
  if (after.digest !== fingerprint.digest) throw new Error("full regression source changed during test execution");
  const wallMs = now() - startedAt;
  if (wallMs > LONG_VALIDATION_MAX_MS) throw new Error(`full regression test execution exceeded its ${LONG_VALIDATION_MAX_MS}ms deadline`);
  const result = { phase: "execute", wall_ms: wallMs, steps: completed, inventory_path: resolvedInventoryPath };
  log(JSON.stringify(result));
  return result;
}

export async function runFullRegression({
  run = runBoundedCommand,
  now = Date.now,
  cwd = DEFAULT_REPO_ROOT,
  inventoryPath,
  log = () => undefined,
  nodeExecutable = process.execPath,
} = {}) {
  const sourceRoot = path.resolve(cwd);
  const startedAt = now();
  const phases = [];
  for (const mode of ["build", "execute"]) {
    const phaseStartedAt = now();
    await runFullRegressionPhase({ mode, run, cwd: sourceRoot, inventoryPath, nodeExecutable });
    const record = { phase: mode, wall_ms: now() - phaseStartedAt };
    phases.push(record);
    log(JSON.stringify(record));
  }
  return { wall_ms: now() - startedAt, phases, inventory_path: inventoryPath };
}

export async function runFullRegressionPhase({
  mode,
  run = runBoundedCommand,
  cwd = DEFAULT_REPO_ROOT,
  inventoryPath,
  nodeExecutable = process.execPath,
} = {}) {
  if (!["build", "execute"].includes(mode)) throw new Error(`full regression phase is invalid: ${mode}`);
  const args = [fileURLToPath(import.meta.url), `--internal-${mode}`];
  if (inventoryPath !== undefined) args.push("--inventory", inventoryPath);
  return run({
    command: nodeExecutable,
    args,
    cwd: path.resolve(cwd),
    env: { ...process.env, [FULL_REGRESSION_INTERNAL_PHASE_ENV]: "1" },
    timeoutMs: LONG_VALIDATION_MAX_MS,
    cleanupReserveMs: COMMAND_CLEANUP_RESERVE_MAX_MS,
  });
}

export function parseFullRegressionArgs(argv) {
  if (!Array.isArray(argv)) throw new Error("full regression CLI arguments must be an array");
  if (argv.length === 0) return { mode: "all", inventoryPath: undefined };
  const rawMode = argv[0];
  const internal = rawMode.startsWith("--internal-");
  const mode = internal ? rawMode.slice("--internal-".length) : rawMode;
  if (!["all", "build", "execute"].includes(mode) || (internal && mode === "all")) {
    throw new Error("usage: run-full-regression.mjs [all | build --inventory <workspace-.tmp-path> | execute --inventory <workspace-.tmp-path>]");
  }
  if (argv.length === 1 && mode === "all") return { mode, inventoryPath: undefined };
  if (internal && argv.length === 1) return { mode, inventoryPath: undefined, internal: true };
  if (argv.length !== 3 || argv[1] !== "--inventory" || argv[2].length === 0) {
    throw new Error(`mode ${mode} requires exactly --inventory <workspace-.tmp-path>`);
  }
  return { mode, inventoryPath: argv[2], ...(internal ? { internal: true } : {}) };
}

export async function main(argv, env = process.env) {
  const options = parseFullRegressionArgs(argv);
  const common = { inventoryPath: options.inventoryPath, log: console.log };
  if (options.internal && env[FULL_REGRESSION_INTERNAL_PHASE_ENV] !== "1") {
    throw new Error("full regression internal phase workers must be started by the external five-minute supervisor");
  }
  if (options.internal && options.mode === "build") return buildFullRegressionArtifacts(common);
  if (options.internal && options.mode === "execute") return executeFullRegression(common);
  if (options.mode === "build" || options.mode === "execute") return runFullRegressionPhase({ mode: options.mode, inventoryPath: options.inventoryPath });
  return runFullRegression(common);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).then((result) => {
    console.log(JSON.stringify(result));
  }).catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
