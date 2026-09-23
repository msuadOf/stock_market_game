#!/usr/bin/env node
/** Build the immutable inputs for one paired Task 9 performance run. */
import { lstat, mkdir, readFile, realpath, writeFile } from "node:fs/promises";
import { execFile } from "node:child_process";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { escrowSourceManifest } from "./escrow-source-manifest.mjs";

const WORKSPACE = path.resolve(fileURLToPath(new URL("../..", import.meta.url)));
const execFileAsync = promisify(execFile);

async function verifyExecutable(file, label) {
  const canonical = await realpath(file);
  const relative = path.relative(WORKSPACE, canonical);
  if (!relative || relative === ".." || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative)) {
    throw new Error(`${label} must resolve inside the workspace`);
  }
  const stat = await lstat(canonical);
  if (!stat.isFile() || (stat.mode & 0o111) === 0) throw new Error(`${label} must be an executable regular file`);
  return canonical;
}

async function actualToolchain() {
  const [{ stdout: cargo }, { stdout: rustc }, { stdout: verbose }] = await Promise.all([
    execFileAsync("cargo", ["--version"], { encoding: "utf8" }),
    execFileAsync("rustc", ["--version"], { encoding: "utf8" }),
    execFileAsync("rustc", ["-vV"], { encoding: "utf8" }),
  ]);
  const host = verbose.match(/^host:\s*(\S+)$/m)?.[1];
  if (!host) throw new Error("rustc -vV did not report a host target");
  return { cargo: cargo.trim(), rustc: rustc.trim(), target: host };
}

async function compilePerformanceEndpoint({ sourceRoot, targetRoot, example, sourceFingerprint }) {
  const canonicalSource = await realpath(sourceRoot);
  const resolvedTarget = path.resolve(targetRoot);
  const relativeTarget = path.relative(path.join(WORKSPACE, ".tmp"), resolvedTarget);
  if (!relativeTarget || relativeTarget === ".." || relativeTarget.startsWith(`..${path.sep}`) || path.isAbsolute(relativeTarget)) {
    throw new Error("performance build target must be a strict child of workspace .tmp");
  }
  await mkdir(resolvedTarget, { recursive: true });
  await execFileAsync("cargo", ["build", "--locked", "--release", "-p", "engine", "--example", example], {
    cwd: canonicalSource,
    env: {
      ...process.env,
      CARGO_TARGET_DIR: resolvedTarget,
      ESCROW_SOURCE_FINGERPRINT: sourceFingerprint,
      TMPDIR: path.join(WORKSPACE, ".tmp"),
      TMP: path.join(WORKSPACE, ".tmp"),
      TEMP: path.join(WORKSPACE, ".tmp"),
    },
    encoding: "utf8",
    maxBuffer: 16 * 1024 * 1024,
  });
  return verifyExecutable(path.join(resolvedTarget, "release", "examples", example), `${example} binary`);
}

async function writeNew(file, value) {
  await writeFile(file, `${JSON.stringify(value, null, 2)}\n`, { flag: "wx" });
}

export async function preparePerformanceConfig({
  setupRequestPath,
  baselineRoot,
  currentRoot = WORKSPACE,
  outputRoot,
  completedTicks = 200,
  repetitions = 2,
  warmupRuns = 1,
  sampleCount = 3,
  rssSampleIntervalMs = 5,
  rayonThreads = Math.max(1, Math.min(4, os.availableParallelism())),
  buildEndpoint = compilePerformanceEndpoint,
}) {
  for (const [label, value] of Object.entries({ baselineRoot, currentRoot, outputRoot })) {
    if (!path.isAbsolute(value)) throw new Error(`${label} must be absolute`);
  }
  const toolchain = await actualToolchain();
  await mkdir(outputRoot, { recursive: false });
  const [setupRequest, baselineManifest, currentManifest] = await Promise.all([
    readFile(setupRequestPath, "utf8").then(JSON.parse),
    escrowSourceManifest(await realpath(baselineRoot)),
    escrowSourceManifest(await realpath(currentRoot)),
  ]);
  if (setupRequest.setup?.simulation_policy_id !== "a-share-simulation-v2") {
    throw new Error("performance setup must be an explicit current v2 SessionSetup");
  }
  if (!Number.isSafeInteger(completedTicks) || completedTicks <= 0
    || !Number.isSafeInteger(repetitions) || repetitions <= 0
    || completedTicks % repetitions !== 0) {
    throw new Error("completedTicks must be positive and divisible by repetitions");
  }
  const workload = {
    scenario: "task9-paired-production-ticks-v1",
    seed: String(setupRequest.seed),
    setup_manifest: setupRequest.setup,
    completed_ticks: completedTicks,
    repetitions,
    profile: "release",
    features: [],
  };
  const environmentContract = {
    cargo: toolchain.cargo,
    rustc: toolchain.rustc,
    target: toolchain.target,
    rustflags: process.env.RUSTFLAGS ?? "",
    cargo_jobs: Number(process.env.CARGO_BUILD_JOBS ?? 1),
    rayon_threads: rayonThreads,
  };
  if (!environmentContract.cargo || !environmentContract.rustc || !environmentContract.target) {
    throw new Error("actual Cargo/Rust toolchain probe was incomplete");
  }
  if (!Number.isSafeInteger(environmentContract.cargo_jobs) || environmentContract.cargo_jobs <= 0
    || !Number.isSafeInteger(rayonThreads) || rayonThreads <= 0) {
    throw new Error("cargo_jobs and rayon_threads must be positive safe integers");
  }
  // Build only after both complete source manifests are frozen. The endpoint
  // embeds that fingerprint and refuses a request from any other source tree.
  // Keep the two release builds sequential to honor the single full-build slot.
  const verifiedBaselineBinary = await buildEndpoint({
    sourceRoot: baselineRoot,
    targetRoot: path.join(outputRoot, "baseline-target"),
    example: "escrow_performance_baseline_adapter",
    sourceFingerprint: baselineManifest.sha256,
  });
  const verifiedCurrentBinary = await buildEndpoint({
    sourceRoot: currentRoot,
    targetRoot: path.join(outputRoot, "current-target"),
    example: "escrow_performance_endpoint",
    sourceFingerprint: currentManifest.sha256,
  });
  const endpoint = (sourceFingerprint) => ({
    schema: "escrow-perf-endpoint-request-v1", workload,
    environment_contract: environmentContract, source_fingerprint: sourceFingerprint,
  });
  const beforeRequest = path.join(outputRoot, "before-request.json");
  const afterRequest = path.join(outputRoot, "after-request.json");
  await Promise.all([
    writeNew(beforeRequest, endpoint(baselineManifest.sha256)),
    writeNew(afterRequest, endpoint(currentManifest.sha256)),
    writeNew(path.join(outputRoot, "before-source-manifest.json"), baselineManifest),
    writeNew(path.join(outputRoot, "after-source-manifest.json"), currentManifest),
  ]);
  const config = {
    schema: "escrow-perf-config-v2", workload, environment_contract: environmentContract,
    warmup_runs: warmupRuns, sample_count: sampleCount,
    rss_sample_interval_ms: rssSampleIntervalMs,
    before: { command: [verifiedBaselineBinary, beforeRequest], cwd: baselineRoot,
      source_fingerprint: baselineManifest.sha256 },
    after: { command: [verifiedCurrentBinary, afterRequest], cwd: currentRoot,
      source_fingerprint: currentManifest.sha256 },
  };
  const configPath = path.join(outputRoot, "perf-config.json");
  await writeNew(configPath, config);
  return { configPath, baseline_source_fingerprint: baselineManifest.sha256,
    current_source_fingerprint: currentManifest.sha256 };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [setupRequestPath, baselineRoot, currentRoot, outputRoot] = process.argv.slice(2);
  if (![setupRequestPath, baselineRoot, currentRoot, outputRoot].every(Boolean)) {
    console.error("usage: prepare-escrow-performance-config.mjs <setup-request> <baseline-root> <current-root> <output-root>");
    process.exitCode = 2;
  } else {
    preparePerformanceConfig({ setupRequestPath: path.resolve(setupRequestPath),
      baselineRoot: path.resolve(baselineRoot), currentRoot: path.resolve(currentRoot),
      outputRoot: path.resolve(outputRoot) })
      .then((result) => console.log(JSON.stringify({ status: "PASS", ...result })))
      .catch((error) => { console.error(JSON.stringify({ status: "FAIL", error: error.message })); process.exitCode = 1; });
  }
}
