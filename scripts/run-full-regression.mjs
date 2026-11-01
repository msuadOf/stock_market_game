#!/usr/bin/env node
import { createHash } from "node:crypto";
import { constants } from "node:fs";
import fsp from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { StringDecoder } from "node:string_decoder";
import { fileURLToPath } from "node:url";

import { COMMAND_CLEANUP_RESERVE_MAX_MS, LONG_VALIDATION_MAX_MS, ORDINARY_TEST_MAX_MS, runBoundedCommand } from "./run-with-deadline.mjs";
import { prepareWorkspacePaths, validateWorkspaceOutputPath } from "./workspace-paths.mjs";
import { WEB_TEST_INTERNAL_WORKER_ENV } from "./run-web-tests.mjs";
import { discoverScriptTestFiles, runScriptTestBatch } from "./run-script-tests.mjs";
import { hashArtifactFiles } from "./full-regression-artifact-hash.mjs";
import { executeSupervisedRustTestBinaries } from "./rust-test-case-supervisor.mjs";

const DEFAULT_REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const MAX_CONCURRENT_RUST_TEST_BINARIES = 8;
const MIN_CPU_BUDGET_PER_TEST_BINARY = 2;
const FULL_REGRESSION_INVENTORY_SCHEMA = "full-regression-artifact-inventory";
const FULL_REGRESSION_INVENTORY_SCHEMA_VERSION = 1;
const FULL_REGRESSION_SOURCE_ALGORITHM = "full-regression-source-fingerprint";
const FULL_REGRESSION_SOURCE_ALGORITHM_VERSION = 1;
const FULL_REGRESSION_INTERNAL_PHASE_ENV = "STOCK_GAME_FULL_REGRESSION_INTERNAL_PHASE";
export const REQUIRED_LONG_VALIDATIONS = [
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
  "scripts",
];

export function fullRegressionSteps(cpuCount = os.availableParallelism(), repoRoot = DEFAULT_REPO_ROOT, baseEnv = process.env, workspaceRoot = repoRoot, targetDir) {
  if (!Number.isInteger(cpuCount) || cpuCount <= 0) throw new Error("full regression requires a positive CPU count");
  const resolvedRoot = path.resolve(repoRoot);
  const resolvedWorkspaceRoot = path.resolve(workspaceRoot);
  const cargoTargetDir = targetDir === undefined ? path.join(resolvedWorkspaceRoot, ".tmp", "build-cache", "full-regression") : targetDir;
  const processTmpDir = path.join(resolvedWorkspaceRoot, ".tmp", "process-tmp", "full-regression");
  const workspaceEnv = {
    ...baseEnv,
    CARGO_TARGET_DIR: cargoTargetDir,
    TMPDIR: processTmpDir,
    TMP: processTmpDir,
    TEMP: processTmpDir,
  };
  // 批 9 A7：RAYON_NUM_THREADS 导出退役（rayon 全退）。
  const configuredJobs = baseEnv.STOCK_GAME_FULL_REGRESSION_CARGO_BUILD_JOBS;
  let cargoBuildJobs = cpuCount;
  if (configuredJobs !== undefined) {
    // 使用绝对字符串结尾，避免 `$` 接受末尾 LF/CRLF。
    if (typeof configuredJobs !== "string" || !/^[1-9][0-9]*(?![\s\S])/.test(configuredJobs)
      || !Number.isSafeInteger(Number(configuredJobs))
      || Number(configuredJobs) > cpuCount) {
      throw new Error(`STOCK_GAME_FULL_REGRESSION_CARGO_BUILD_JOBS must be a canonical positive integer within the available CPU limit ${cpuCount}`);
    }
    cargoBuildJobs = Number(configuredJobs);
  }
  const cargoEnv = { ...workspaceEnv, CARGO_BUILD_JOBS: String(cargoBuildJobs) };
  return [
    { kind: "rust-test-build", label: "Rust test build", command: "cargo", args: ["test", "--workspace", "--no-run", "--message-format=json-render-diagnostics"], env: cargoEnv, captureOutput: true },
    { kind: "rust-test-binaries", label: "Rust prebuilt test binaries", env: workspaceEnv, cpuCount },
    {
      kind: "rust-doctests",
      label: "Rust workspace doctests",
      command: "cargo",
      args: ["test", "--workspace", "--doc", "--no-fail-fast", "--", `--test-threads=${Math.max(1, cpuCount - 1)}`],
      env: { ...cargoEnv },
    },
    {
      kind: "ordinary-command",
      label: "web ordinary tests",
      command: process.execPath,
      args: [path.join(resolvedRoot, "scripts", "run-web-tests.mjs"), "--internal-worker"],
      env: { ...workspaceEnv, [WEB_TEST_INTERNAL_WORKER_ENV]: "1" },
    },
    { kind: "script-test-batch", label: "scripts ordinary tests", env: workspaceEnv, cpuCount },
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
  // tauri-build 在编译时生成这些 schema；它们属于输出，不属于源码输入。
  if (relativePath.split(path.sep).join("/") === "apps/desktop/src-tauri/gen/schemas") return;
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
  const state = { algorithm: FULL_REGRESSION_SOURCE_ALGORITHM, algorithm_version: FULL_REGRESSION_SOURCE_ALGORITHM_VERSION, files };
  return { ...state, digest: sha256(JSON.stringify(state)) };
}

async function sealTestArtifacts(artifacts, workspacePaths, cpuCount, hashArtifacts) {
  const executables = new Array(artifacts.length);
  const hashes = await hashArtifacts(artifacts, {
    cpuCount,
    prepare: async (artifact, index) => {
      const executable = await fsp.realpath(artifact.executable);
      const relativeToTarget = path.relative(workspacePaths.cargoTargetDir, executable);
      if (!relativeToTarget || relativeToTarget.startsWith("..") || path.isAbsolute(relativeToTarget)) {
        throw new Error(`prebuilt Rust test executable escaped the Cargo target: ${executable}`);
      }
      const stat = await fsp.lstat(executable);
      if (!stat.isFile() || stat.isSymbolicLink()) throw new Error(`prebuilt Rust test executable must be a regular non-symlink file: ${executable}`);
      executables[index] = executable;
      return executable;
    },
  });
  return artifacts.map((artifact, index) => ({
    label: artifact.label,
    executable_relative_path: path.relative(workspacePaths.workspaceRoot, executables[index]).split(path.sep).join("/"),
    ...hashes[index],
  }));
}

function freezeInventoryRecord(value) {
  const pending = [value];
  while (pending.length > 0) {
    const current = pending.pop();
    if (current !== null && typeof current === "object") {
      for (const child of Object.values(current)) pending.push(child);
      Object.freeze(current);
    }
  }
  return value;
}

export class ArtifactInventory {
  #record;

  constructor(record) {
    // 延用 inventory 的 JSON 接受边界，避免 structuredClone 限制合法深层 extra。
    this.#record = freezeInventoryRecord(JSON.parse(JSON.stringify(record)));
  }

  static #computeIdentityDigest(record) {
    const { identity_digest: ignored, ...state } = record;
    return sha256(JSON.stringify(state));
  }

  static fromBuild(record) {
    return new ArtifactInventory({ ...record, identity_digest: this.#computeIdentityDigest(record) });
  }

  static fromDecodedInventory(decoded, { sourceRoot, workspacePaths, fingerprint }) {
    if (decoded === null || typeof decoded !== "object"
      || decoded.identity_digest !== this.#computeIdentityDigest(decoded)
      || decoded.schema !== FULL_REGRESSION_INVENTORY_SCHEMA
      || decoded.schema_version !== FULL_REGRESSION_INVENTORY_SCHEMA_VERSION
      || decoded.source_root !== sourceRoot
      || decoded.workspace_root !== workspacePaths.workspaceRoot
      || decoded.cargo_target_dir !== workspacePaths.cargoTargetDir
      || decoded.process_tmp_dir !== workspacePaths.processTmpDir
      || decoded.source_fingerprint?.algorithm !== fingerprint.algorithm
      || decoded.source_fingerprint?.algorithm_version !== fingerprint.algorithm_version
      || decoded.source_fingerprint?.digest !== fingerprint.digest
      || !Array.isArray(decoded.artifacts)
      || decoded.artifacts.length === 0) {
      throw new Error("full regression artifact inventory is malformed, stale, or bound to another source/workspace");
    }
    return new ArtifactInventory(decoded);
  }

  toJson() {
    return JSON.parse(JSON.stringify(this.#record));
  }

  artifactDescriptors() {
    return this.#record.artifacts;
  }

  validateArtifactDescriptor(index) {
    const entry = this.#record.artifacts[index];
    if (typeof entry?.label !== "string" || entry.label.length === 0
      || typeof entry?.executable_relative_path !== "string" || entry.executable_relative_path.length === 0
      || path.isAbsolute(entry.executable_relative_path)
      || !Number.isSafeInteger(entry.bytes) || entry.bytes < 0
      || typeof entry.sha256 !== "string" || !/^[a-f0-9]{64}$/.test(entry.sha256)) {
      throw new Error("full regression artifact inventory contains an invalid artifact entry");
    }
  }

  validateArtifactMetadata(index, { executable, isFile, isSymbolicLink, bytes }) {
    const entry = this.#record.artifacts[index];
    if (!isFile || isSymbolicLink || bytes !== entry.bytes) {
      throw new Error(`full regression artifact does not match its sealed inventory: ${executable}`);
    }
  }

  validateArtifactDigest(index, { executable, digest }) {
    const entry = this.#record.artifacts[index];
    if (digest !== entry.sha256) throw new Error(`full regression artifact SHA-256 mismatch: ${executable}`);
  }
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

async function prepareFullRegressionPaths(sourceRoot, targetDir) {
  let canonicalTarget;
  if (targetDir !== undefined) {
    if (typeof targetDir !== "string" || !path.isAbsolute(targetDir)) {
      throw new Error("full regression target directory must be an absolute path");
    }
    // 先逐项检查原始路径，避免 resolve 消去 symbolic link 后的 .. 而漏检。
    const root = path.parse(targetDir).root;
    let current = root;
    const separator = path.sep === "\\" ? /[\\/]/ : /\//;
    for (const part of ["", ...targetDir.slice(root.length).split(separator)]) {
      current = path.join(current, part);
      let stat;
      try { stat = await fsp.lstat(current); }
      catch (error) { throw new Error(`full regression target directory is unavailable at ${current}: ${error.message}`, { cause: error }); }
      if (stat.isSymbolicLink()) throw new Error(`full regression target directory must not contain a symbolic link: ${current}`);
      if (!stat.isDirectory()) throw new Error(`full regression target path component is not a directory: ${current}`);
    }
    canonicalTarget = await fsp.realpath(targetDir);
    try { await fsp.access(canonicalTarget, constants.W_OK); }
    catch (error) { throw new Error(`full regression target directory is not writable at ${canonicalTarget}: ${error.message}`, { cause: error }); }
  }
  const workspacePaths = await prepareWorkspacePaths({ sourceRoot, scope: "full-regression" });
  return canonicalTarget === undefined ? workspacePaths : { ...workspacePaths, cargoTargetDir: canonicalTarget };
}

async function readAndValidateInventory(inventoryPath, sourceRoot, workspacePaths, fingerprint, cpuCount, log, hashArtifacts) {
  const stat = await fsp.lstat(inventoryPath);
  if (!stat.isFile() || stat.isSymbolicLink()) throw new Error(`full regression inventory must be a regular non-symlink file: ${inventoryPath}`);
  let inventory;
  try {
    inventory = JSON.parse(await fsp.readFile(inventoryPath, "utf8"));
  } catch (error) {
    throw new Error(`full regression artifact inventory is not valid JSON at ${inventoryPath}: ${error.message}`, { cause: error });
  }
  const sealedInventory = ArtifactInventory.fromDecodedInventory(inventory, { sourceRoot, workspacePaths, fingerprint });
  const entries = sealedInventory.artifactDescriptors();
  const artifacts = new Array(entries.length);
  log(JSON.stringify({ phase: "artifact-hash-start", stage: "inventory_validation", artifact_count: entries.length,
    hash_worker_count: Math.min(cpuCount, entries.length), stream_chunk_bytes: 65536 }));
  await hashArtifacts(entries, {
    cpuCount,
    prepare: async (entry, index) => {
      // 按原 index 收敛错误，后项 descriptor 失败不能掩盖先前 filesystem 或 SHA 失败。
      sealedInventory.validateArtifactDescriptor(index);
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
      sealedInventory.validateArtifactMetadata(index, {
        executable, isFile: stat.isFile(), isSymbolicLink: stat.isSymbolicLink(), bytes: stat.size,
      });
      artifacts[index] = { executable, label: entry.label };
      return executable;
    },
    verify: ({ bytes, sha256 }, _entry, index) => {
      const { executable } = artifacts[index];
      sealedInventory.validateArtifactDigest(index, { executable, digest: sha256 });
      sealedInventory.validateArtifactMetadata(index, { executable, isFile: true, isSymbolicLink: false, bytes });
    },
  });
  return { inventory: sealedInventory, artifacts };
}

export function buildRustTestExecutionPolicy(availableCpuCount, binaryCount) {
  if (!Number.isInteger(availableCpuCount) || availableCpuCount <= 0) throw new Error("Rust test execution requires a positive CPU count");
  if (!Number.isInteger(binaryCount) || binaryCount <= 0) throw new Error("Rust test execution requires at least one prebuilt binary");
  const maxConcurrentBinaries = Math.min(
    MAX_CONCURRENT_RUST_TEST_BINARIES,
    binaryCount,
    Math.max(1, Math.floor(availableCpuCount / MIN_CPU_BUDGET_PER_TEST_BINARY)),
  );
  const cpuBudgetPerBinary = Math.floor(availableCpuCount / maxConcurrentBinaries);
  const remainingCpus = availableCpuCount % maxConcurrentBinaries;
  const workerBudgets = Array.from({ length: maxConcurrentBinaries }, (_, index) => {
    const cpuBudget = cpuBudgetPerBinary + Number(index < remainingCpus);
    return {
      cpu_budget: cpuBudget,
      // 批 9 A7：rayon 分量退役——全额并回 test_threads。
      test_threads: Math.max(1, cpuBudget),
    };
  });
  return {
    available_cpu_count: availableCpuCount,
    binary_count: binaryCount,
    max_concurrent_binaries: maxConcurrentBinaries,
    worker_budgets: workerBudgets,
    aggregate_configured_thread_budget: workerBudgets.reduce((total, budget) => total + budget.test_threads, 0),
  };
}

export function parseRustTestCases(stdout) {
  if (typeof stdout !== "string") throw new Error("Rust test listing did not return output");
  const names = new Set();
  for (const line of stdout.split(/\r?\n/)) {
    if (line.length === 0 || /^\d+ tests?, \d+ benchmarks?$/.test(line) || line.endsWith(": benchmark")) continue;
    if (!line.endsWith(": test")) throw new Error(`malformed Rust test listing: ${line}`);
    const name = line.slice(0, -6);
    if (name.length === 0 || names.has(name)) throw new Error(`invalid or duplicate Rust test case: ${name}`);
    names.add(name);
  }
  return [...names];
}

export async function executeRustTestBinaries({ artifacts, policy, run = runBoundedCommand, cwd, env, remainingMs, now, log = () => undefined, signal }) {
  if (run === runBoundedCommand) return executeSupervisedRustTestBinaries({ artifacts, policy, cwd, env, remainingMs, log, signal });
  let nextIndex = 0;
  let stop = false;
  const failures = [];
  const results = new Array(artifacts.length);
  const controller = new AbortController();
  const combinedSignal = signal ? AbortSignal.any([signal, controller.signal]) : controller.signal;
  async function worker(workerIndex) {
    const budget = policy.worker_budgets[workerIndex];
    while (!stop && !combinedSignal.aborted) {
      const index = nextIndex;
      nextIndex += 1;
      if (index >= artifacts.length) return;
      const artifact = artifacts[index];
      try {
        const lists = [];
        for (const ignored of [false, true]) {
          const timeoutMs = Math.min(ORDINARY_TEST_MAX_MS, remainingMs(`Rust test listing ${artifact.label}`));
          const output = await run({
            command: artifact.executable,
            args: ["--list", "--format=terse", ...(ignored ? ["--ignored"] : [])],
            env: { ...env },
            cwd, timeoutMs, captureOutput: true,
            cleanupReserveMs: Math.min(COMMAND_CLEANUP_RESERVE_MAX_MS, timeoutMs - 1),
            signal: combinedSignal,
          });
          lists.push(parseRustTestCases(output?.stdout));
        }
        const ignoredNames = new Set(lists[1]);
        if (lists[1].some((name) => !lists[0].includes(name))) throw new Error("ignored Rust test listing contains an unknown case");
        results[index] = lists[0].filter((name) => !ignoredNames.has(name)).map((name) => ({ ...artifact, name }));
      } catch (error) {
        stop = true;
        if (!controller.signal.aborted) controller.abort(error);
        failures.push(new Error(`${artifact.label} failed: ${error.message}`, { cause: error }));
      }
    }
  }
  await Promise.all(Array.from({ length: policy.max_concurrent_binaries }, (_, index) => worker(index)));
  if (signal?.aborted && failures.length === 0) throw signal.reason;
  if (failures.length > 0) throw new AggregateError(failures, `${failures.length} prebuilt Rust test binaries failed:\n${failures.map((failure) => failure.message).join("\n")}`);
  const cases = results.flat();
  const concurrency = Math.min(cases.length, Math.max(1, Math.min(32, Math.floor(policy.available_cpu_count / 2))));
  const casePolicy = { case_count: cases.length, max_concurrent_cases: concurrency, test_threads: 1, ordinary_case_timeout_ms: ORDINARY_TEST_MAX_MS };
  log(JSON.stringify({ phase: "rust-ordinary-cases-start", ...casePolicy }));
  const isActorTradeCase = (testCase) => testCase.label.includes("/apps/server#") && testCase.label.endsWith(" test:actor") && testCase.name === "actor_market_goes_live_produces_trade_events";
  const isReportCase = (testCase) => testCase.label.includes("/packages/engine#") && testCase.label.endsWith(" test:session_company_assembly") && testCase.name === "mixed_group_closes_then_publishes_distinct_consolidated_public_report";
  const isIsolatedCase = (testCase) => isActorTradeCase(testCase) || isReportCase(testCase);
  const isolatedCases = cases.filter(isIsolatedCase);
  const remainingCases = cases.filter((testCase) => !isIsolatedCase(testCase));
  const isCalendarCase = (testCase) => /^session::calendar_(?:nonzero_plan_parity|final_boundary)_tests::/.test(testCase.name)
    || /^session::pipeline::authoritative_tick_tests::extreme_phase_geometry_(?:single_tick_day|no_closing_auction|closing_auction)$/.test(testCase.name);
  const calendarCases = remainingCases.filter(isCalendarCase);
  const ordinaryCases = remainingCases.filter((testCase) => !isCalendarCase(testCase));
  async function caseWorker(queue) {
    while (!stop && !combinedSignal.aborted) {
      const testCase = queue.shift();
      if (testCase === undefined) return;
      try {
        const timeoutMs = Math.min(ORDINARY_TEST_MAX_MS, remainingMs(`Rust test case ${testCase.label} ${testCase.name}`));
        await run({
          command: testCase.executable,
          args: [testCase.name, "--exact", "--test-threads=1"],
          env: { ...env },
          cwd, timeoutMs,
          cleanupReserveMs: Math.min(COMMAND_CLEANUP_RESERVE_MAX_MS, timeoutMs - 1),
          signal: combinedSignal,
        });
      } catch (error) {
        stop = true;
        if (!controller.signal.aborted) controller.abort(error);
        failures.push(new Error(`${testCase.label} case ${testCase.name} failed: ${error.message}`, { cause: error }));
      }
    }
  }
  await caseWorker(isolatedCases);
  if (concurrency <= 1) {
    await caseWorker(remainingCases);
  } else {
    const calendarWorkers = Number(calendarCases.length > 0);
    const ordinaryWorkers = Math.min(ordinaryCases.length, concurrency - calendarWorkers);
    await Promise.all([
      ...Array.from({ length: calendarWorkers }, () => caseWorker(calendarCases)),
      ...Array.from({ length: ordinaryWorkers }, () => caseWorker(ordinaryCases)),
    ]);
  }
  if (signal?.aborted && failures.length === 0) throw signal.reason;
  if (failures.length > 0) throw new AggregateError(failures, `ordinary Rust test cases failed:\n${failures.map((failure) => failure.message).join("\n")}`);
  return casePolicy;
}

async function executeRequiredLongValidations({ artifacts, policy, run, cwd, env, remainingMs, now, signal }) {
  const results = [];
  for (const validation of REQUIRED_LONG_VALIDATIONS) {
    signal.throwIfAborted();
    const matches = artifacts.filter((artifact) => artifact.label.includes(validation.targetLabel));
    if (matches.length !== 1) {
      throw new Error(`required long validation target ${validation.targetLabel} resolved to ${matches.length} prebuilt binaries`);
    }
    const artifact = matches[0];
    // 过滤名漂移时 --exact 会命中 0 条用例并静默绿，必须先列出并显式拒绝空集。
    const listTimeoutMs = remainingMs(`required long validation listing ${validation.filter}`);
    const listing = await run({
      command: artifact.executable,
      args: ["--list", "--format=terse", "--exact", "--ignored", validation.filter],
      env,
      cwd,
      timeoutMs: listTimeoutMs,
      captureOutput: true,
      signal,
      cleanupReserveMs: Math.min(COMMAND_CLEANUP_RESERVE_MAX_MS, Math.max(1, listTimeoutMs - 1)),
    });
    const listedCases = parseRustTestCases(listing?.stdout);
    if (listedCases.length !== 1) {
      throw new Error(`required long validation filter ${validation.filter} resolved to ${listedCases.length} test cases in ${artifact.label}`);
    }
    const timeoutMs = remainingMs(`required long validation ${validation.filter}`);
    const startedAt = now();
    await run({
      command: artifact.executable,
      args: [validation.filter, "--exact", "--ignored", `--test-threads=${policy.testThreadsPerBinary}`],
      env: { ...env },
      cwd,
      timeoutMs,
      signal,
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

function cargoBuildProgress({ log, now, startedAt }) {
  const decoder = new StringDecoder("utf8");
  let pending = "";
  let completedTargets = 0;
  let lastCompletedTarget;
  let compilerErrors = 0;
  function parseLine(line) {
    if (line.trim().length === 0) return;
    let message;
    try { message = JSON.parse(line); } catch (error) {
      throw new Error(`Cargo emitted malformed JSON while building test artifacts: ${error.message}`, { cause: error });
    }
    if (message.reason === "compiler-artifact") {
      completedTargets += 1;
      lastCompletedTarget = `${message.package_id ?? "unknown-package"} ${message.target?.name ?? "unknown-target"}`.slice(0, 256);
    } else if (message.reason === "compiler-message" && message.message?.level === "error") {
      compilerErrors += 1;
      if (compilerErrors <= 10) log(JSON.stringify({
        phase: "build", status: "compiler_error", package: String(message.package_id ?? "unknown-package").slice(0, 256),
        diagnostic: String(message.message.rendered ?? message.message.message ?? "missing rendered diagnostic").slice(0, 2048),
      }));
    }
  }
  return {
    onStdout(chunk) {
      pending += decoder.write(chunk);
      let newline;
      while ((newline = pending.indexOf("\n")) !== -1) {
        parseLine(pending.slice(0, newline));
        pending = pending.slice(newline + 1);
      }
    },
    finish() {
      pending += decoder.end();
      parseLine(pending);
      pending = "";
    },
    snapshot(status, extra = {}) {
      log(JSON.stringify({ phase: "build", status, elapsed_ms: now() - startedAt,
        completed_targets: completedTargets, last_completed_target: lastCompletedTarget,
        compiler_errors: compilerErrors, ...extra }));
    },
  };
}

export async function buildFullRegressionArtifacts({
  run = runBoundedCommand,
  now = Date.now,
  cpuCount = os.availableParallelism(),
  cwd = DEFAULT_REPO_ROOT,
  baseEnv = process.env,
  inventoryPath,
  targetDir,
  collectFingerprint = collectFullRegressionSourceFingerprint,
  hashArtifacts = hashArtifactFiles,
  log = () => undefined,
  writeDiagnostic = (chunk) => process.stderr.write(chunk),
  progressIntervalMs = 30_000,
} = {}) {
  const sourceRoot = await fsp.realpath(path.resolve(cwd));
  const workspacePaths = await prepareFullRegressionPaths(sourceRoot, targetDir);
  const resolvedInventoryPath = await prepareInventoryPath(workspacePaths.workspaceRoot, inventoryPath);
  const buildStep = fullRegressionSteps(cpuCount, sourceRoot, baseEnv, workspacePaths.workspaceRoot, workspacePaths.cargoTargetDir)
    .find((step) => step.kind === "rust-test-build");
  const cargoBuildJobs = Number(buildStep.env.CARGO_BUILD_JOBS);
  log(JSON.stringify({ phase: "build-start", available_cpu_count: cpuCount, cargo_build_jobs: cargoBuildJobs }));
  const startedAt = now();
  log(JSON.stringify({ phase: "build", status: "started", stage: "source_fingerprint", elapsed_ms: 0,
    cpu_jobs: cargoBuildJobs, deadline_ms: LONG_VALIDATION_MAX_MS, cargo_target_dir: workspacePaths.cargoTargetDir }));
  const before = await collectFingerprint(sourceRoot);
  const progress = cargoBuildProgress({ log, now, startedAt });
  progress.snapshot("started", { stage: "cargo_test_build", cpu_jobs: cargoBuildJobs });
  const heartbeat = setInterval(() => progress.snapshot("running", { stage: "cargo_test_build", cpu_jobs: cargoBuildJobs }), progressIntervalMs);
  let output;
  try {
    output = await run({
      ...phaseCommandOptions(buildStep, sourceRoot, phaseRemainingMs(startedAt, now, buildStep.label)),
      captureOutput: true,
      onStdout: progress.onStdout,
      onStderr: writeDiagnostic,
    });
  } catch (error) {
    let failure = error;
    try {
      progress.finish();
    } catch (parseError) {
      failure = new AggregateError([error, parseError],
        `Cargo build failed (${error.message.slice(0, 1024)}); final JSON diagnostic was malformed (${parseError.message.slice(0, 512)})`);
    }
    progress.snapshot("failed", { stage: "cargo_test_build", error: failure.message.slice(0, 2048) });
    throw failure;
  } finally {
    clearInterval(heartbeat);
  }
  progress.finish();
  progress.snapshot("completed", { stage: "cargo_test_build" });
  const artifacts = parseCargoTestExecutables(output?.stdout, workspacePaths.cargoTargetDir);
  progress.snapshot("started", { stage: "source_fingerprint_after_build" });
  const after = await collectFingerprint(sourceRoot);
  if (before.digest !== after.digest) {
    throw new Error("full regression source changed during the cold build; refusing to publish an artifact inventory");
  }
  phaseRemainingMs(startedAt, now, "artifact sealing");
  progress.snapshot("started", { stage: "artifact_sealing", artifact_count: artifacts.length,
    hash_worker_count: Math.min(cpuCount, artifacts.length), stream_chunk_bytes: 65536 });
  const sealedArtifacts = await sealTestArtifacts(artifacts, workspacePaths, cpuCount, hashArtifacts);
  const inventory = ArtifactInventory.fromBuild({
    schema: FULL_REGRESSION_INVENTORY_SCHEMA,
    schema_version: FULL_REGRESSION_INVENTORY_SCHEMA_VERSION,
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
  }).toJson();
  phaseRemainingMs(startedAt, now, "atomic artifact inventory publication");
  progress.snapshot("started", { stage: "inventory_publication", artifact_count: artifacts.length });
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

export function regressionLaneBudget(cpuCount) {
  if (!Number.isInteger(cpuCount) || cpuCount < 1) throw new Error("full regression requires a positive CPU count");
  if (cpuCount === 1) return { rust: 1, web: 1 };
  const web = Math.max(1, Math.floor(cpuCount / 4));
  return { rust: cpuCount - web, web };
}

export async function executeRegressionLanes(cpuCount, rust, web) {
  const controller = new AbortController();
  if (cpuCount === 1) {
    await rust(controller.signal);
    await web(controller.signal);
    return;
  }
  let firstFailure;
  const lane = async (work) => {
    try { await work(controller.signal); }
    catch (error) {
      if (firstFailure === undefined) { firstFailure = error; controller.abort(error); }
      throw error;
    }
  };
  const results = await Promise.allSettled([lane(rust), lane(web)]);
  if (firstFailure !== undefined) {
    const errors = results.filter((result) => result.status === "rejected").map((result) => result.reason);
    throw new AggregateError(errors, `full regression lane failed: ${firstFailure.message}`);
  }
}

export async function executeFullRegression({
  run = runBoundedCommand,
  now = Date.now,
  cpuCount = os.availableParallelism(),
  cwd = DEFAULT_REPO_ROOT,
  baseEnv = process.env,
  inventoryPath,
  targetDir,
  collectFingerprint = collectFullRegressionSourceFingerprint,
  hashArtifacts = hashArtifactFiles,
  log = () => undefined,
  runScripts = runScriptTestBatch,
} = {}) {
  const sourceRoot = await fsp.realpath(path.resolve(cwd));
  const workspacePaths = await prepareFullRegressionPaths(sourceRoot, targetDir);
  const resolvedInventoryPath = await prepareInventoryPath(workspacePaths.workspaceRoot, inventoryPath);
  const startedAt = now();
  const fingerprint = await collectFingerprint(sourceRoot);
  phaseRemainingMs(startedAt, now, "artifact inventory validation");
  const { artifacts } = await readAndValidateInventory(resolvedInventoryPath, sourceRoot, workspacePaths, fingerprint, cpuCount, log, hashArtifacts);
  const steps = fullRegressionSteps(cpuCount, sourceRoot, baseEnv, workspacePaths.workspaceRoot, workspacePaths.cargoTargetDir);
  const binaryStep = steps.find((step) => step.kind === "rust-test-binaries");
  const doctestStep = steps.find((step) => step.kind === "rust-doctests");
  const webStep = steps.find((step) => step.kind === "ordinary-command");
  const completed = [];
  const remainingMs = (context) => phaseRemainingMs(startedAt, now, context);

  const budgets = regressionLaneBudget(cpuCount);
  const rustPolicy = buildRustTestExecutionPolicy(budgets.rust, artifacts.length);
  const longValidationPolicy = {
    testThreadsPerBinary: 1,
  };
  log(JSON.stringify({ phase: "execute-start", ...rustPolicy, long_validation: longValidationPolicy }));
  log(JSON.stringify({ phase: "execute-lanes", rust_cpu_count: budgets.rust, web_cpu_count: budgets.web, parallel: cpuCount > 1 }));
  const scriptStep = steps.find((step) => step.kind === "script-test-batch");
  const scriptFiles = await discoverScriptTestFiles(sourceRoot);
  const isolatedFile = path.join(sourceRoot, "scripts", "rust-test-case-supervisor.test.mjs");
  if (scriptFiles.filter((file) => file === isolatedFile).length !== 1) throw new Error(`缺少唯一时序敏感测试：${isolatedFile}`);
  // 此 fixture 的进程就绪与 /proc 清理争用敏感，先独立执行并计入同一期限。
  const isolatedStartedAt = now();
  const isolatedTimeout = Math.min(ORDINARY_TEST_MAX_MS, remainingMs(isolatedFile));
  await run({ command: process.execPath,
    args: ["--test", "--test-timeout=10000", "--test-isolation=none", "--test-concurrency=1", isolatedFile],
    cwd: sourceRoot, env: scriptStep.env, timeoutMs: isolatedTimeout,
    cleanupReserveMs: Math.min(COMMAND_CLEANUP_RESERVE_MAX_MS, isolatedTimeout - 1),
  });
  const isolatedResult = { file: isolatedFile, wall_ms: now() - isolatedStartedAt };
  await executeRegressionLanes(cpuCount, async (signal) => {
    let stepStartedAt = now();
    const casePolicy = await executeRustTestBinaries({ artifacts, policy: rustPolicy, run, cwd: sourceRoot, env: binaryStep.env, remainingMs, now, log, signal });
    completed.push({ label: binaryStep.label, wall_ms: now() - stepStartedAt, binary_count: artifacts.length, ...rustPolicy, ...casePolicy });
    signal.throwIfAborted();
    stepStartedAt = now();
    const cases = await executeRequiredLongValidations({ artifacts, policy: longValidationPolicy, run, cwd: sourceRoot, env: binaryStep.env, remainingMs, now, signal });
    completed.push({ label: "Rust required long validations", wall_ms: now() - stepStartedAt, case_count: cases.length, test_threads: 1, cases });
    signal.throwIfAborted();
    stepStartedAt = now();
    const configuredJobs = Number(doctestStep.env.CARGO_BUILD_JOBS);
    const docStep = { ...doctestStep, env: { ...doctestStep.env, CARGO_BUILD_JOBS: String(Math.min(configuredJobs, budgets.rust)) },
      args: [...doctestStep.args.slice(0, -1), `--test-threads=${Math.max(1, budgets.rust - 1)}`] };
    await run({ ...phaseCommandOptions(docStep, sourceRoot, remainingMs(docStep.label)), signal });
    completed.push({ label: docStep.label, wall_ms: now() - stepStartedAt });
  }, async (signal) => {
    let stepStartedAt = now();
    for (let batchIndex = 0; batchIndex < 12; batchIndex += 1) {
      signal.throwIfAborted();
      const timeoutMs = Math.min(ORDINARY_TEST_MAX_MS, remainingMs(webStep.label));
      const step = { ...webStep, env: { ...webStep.env, STOCK_GAME_WEB_TEST_CPU_BUDGET: String(budgets.web) }, args: [...webStep.args, String(batchIndex), "12"] };
      await run({ ...phaseCommandOptions(step, sourceRoot, timeoutMs), signal });
    }
    completed.push({ label: webStep.label, wall_ms: now() - stepStartedAt });
    signal.throwIfAborted();
    stepStartedAt = now();
    const scripts = await runScripts({ cwd: sourceRoot, env: scriptStep.env, cpuCount: budgets.web, run, now, remainingMs, log, signal, discover: async () => scriptFiles.filter((file) => file !== isolatedFile) });
    completed.push({ ...scripts, label: scriptStep.label, wall_ms: now() - stepStartedAt + isolatedResult.wall_ms,
      file_count: scripts.file_count + 1, files: [...(scripts.files ?? []), isolatedResult].sort((a, b) => a.file.localeCompare(b.file)) });
  });

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
  targetDir,
  log = () => undefined,
  nodeExecutable = process.execPath,
} = {}) {
  const sourceRoot = path.resolve(cwd);
  const startedAt = now();
  const phases = [];
  for (const mode of ["build", "execute"]) {
    const phaseStartedAt = now();
    await runFullRegressionPhase({ mode, run, cwd: sourceRoot, inventoryPath, targetDir, nodeExecutable });
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
  targetDir,
  nodeExecutable = process.execPath,
} = {}) {
  if (!["build", "execute"].includes(mode)) throw new Error(`full regression phase is invalid: ${mode}`);
  const args = [fileURLToPath(import.meta.url), `--internal-${mode}`];
  if (inventoryPath !== undefined) args.push("--inventory", inventoryPath);
  if (targetDir !== undefined) args.push("--target-dir", targetDir);
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
    throw new Error("usage: run-full-regression.mjs [all | build --inventory <workspace-.tmp-path> | execute --inventory <workspace-.tmp-path>] [--target-dir <absolute-existing-real-dir>]");
  }
  const options = { mode, inventoryPath: undefined, ...(internal ? { internal: true } : {}) };
  const seen = new Set();
  for (let index = 1; index < argv.length; index += 2) {
    const option = argv[index];
    if (!["--inventory", "--target-dir"].includes(option)) throw new Error(`unknown full regression option: ${option}`);
    if (seen.has(option)) throw new Error(`duplicate full regression option: ${option}`);
    const value = argv[index + 1];
    if (typeof value !== "string" || value.length === 0 || value.startsWith("--")) throw new Error(`full regression option ${option} requires a non-empty value`);
    seen.add(option);
    options[option === "--inventory" ? "inventoryPath" : "targetDir"] = value;
  }
  if (!internal && mode !== "all" && options.inventoryPath === undefined) throw new Error(`mode ${mode} requires --inventory <workspace-.tmp-path>`);
  return options;
}

export async function main(argv, env = process.env) {
  const options = parseFullRegressionArgs(argv);
  const common = { inventoryPath: options.inventoryPath, targetDir: options.targetDir, log: console.log };
  if (options.internal && env[FULL_REGRESSION_INTERNAL_PHASE_ENV] !== "1") {
    throw new Error("full regression internal phase workers must be started by the external five-minute supervisor");
  }
  if (options.internal && options.mode === "build") return buildFullRegressionArtifacts(common);
  if (options.internal && options.mode === "execute") return executeFullRegression(common);
  if (options.mode === "build" || options.mode === "execute") return runFullRegressionPhase({ mode: options.mode, ...common });
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
