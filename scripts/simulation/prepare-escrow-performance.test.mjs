import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { chmod, mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { promisify } from "node:util";
import { afterEach, test } from "node:test";
import { fileURLToPath } from "node:url";
import { prepareBaselineAdapter, verifyHistoricalCheckout } from "./prepare-escrow-performance-baseline.mjs";
import { preparePerformanceConfig } from "./prepare-escrow-performance-config.mjs";

const execFileAsync = promisify(execFile);
const WORKSPACE = path.resolve(fileURLToPath(new URL("../..", import.meta.url)));
const TMP_ROOT = path.join(WORKSPACE, ".tmp", "prepare-performance-tests");
const cleanups = [];

afterEach(async () => {
  await Promise.all(cleanups.splice(0).map((directory) => rm(directory, { recursive: true, force: true })));
});

async function temporaryDirectory(prefix) {
  await mkdir(TMP_ROOT, { recursive: true });
  const directory = await mkdtemp(path.join(TMP_ROOT, prefix));
  cleanups.push(directory);
  return directory;
}

test("historical checkout verifier rejects the wrong revision and a dirty tree", async () => {
  const repository = await temporaryDirectory("baseline-");
  await execFileAsync("git", ["-C", repository, "init", "--quiet"]);
  await execFileAsync("git", ["-C", repository, "config", "user.email", "task9@example.test"]);
  await execFileAsync("git", ["-C", repository, "config", "user.name", "Task 9"]);
  await writeFile(path.join(repository, "tracked"), "sealed\n");
  await execFileAsync("git", ["-C", repository, "add", "tracked"]);
  await execFileAsync("git", ["-C", repository, "commit", "--quiet", "-m", "sealed"]);
  const { stdout } = await execFileAsync("git", ["-C", repository, "rev-parse", "HEAD"]);
  const revision = stdout.trim();
  await assert.rejects(() => verifyHistoricalCheckout(repository, "0".repeat(40)), /HEAD must be/);
  await verifyHistoricalCheckout(repository, revision);
  await writeFile(path.join(repository, "untracked"), "drift\n");
  await assert.rejects(() => verifyHistoricalCheckout(repository, revision), /must be clean/);
});

test("performance config records the actual toolchain and builds both source-bound endpoints", async () => {
  const root = await temporaryDirectory("config-");
  const request = path.join(root, "request.json");
  const baselineBinary = path.join(root, "baseline-endpoint");
  const currentBinary = path.join(root, "current-endpoint");
  await writeFile(request, JSON.stringify({ seed: "1", setup: { simulation_policy_id: "a-share-simulation-v2" } }));
  await writeFile(baselineBinary, "#!/bin/sh\nexit 0\n");
  await writeFile(currentBinary, "#!/bin/sh\nexit 0\n");
  await Promise.all([chmod(baselineBinary, 0o755), chmod(currentBinary, 0o755)]);
  const builds = [];
  const buildEndpoint = async (request) => {
    builds.push(request);
    return request.example === "escrow_performance_baseline_adapter" ? baselineBinary : currentBinary;
  };
  const outputRoot = path.join(root, "output");
  const prepared = await preparePerformanceConfig({
    setupRequestPath: request,
    baselineRoot: WORKSPACE,
    currentRoot: WORKSPACE,
    outputRoot,
    completedTicks: 2,
    repetitions: 1,
    warmupRuns: 0,
    sampleCount: 1,
    buildEndpoint,
  });
  const configuration = JSON.parse(await (await import("node:fs/promises")).readFile(prepared.configPath, "utf8"));
  const [{ stdout: cargo }, { stdout: rustc }, { stdout: verbose }] = await Promise.all([
    execFileAsync("cargo", ["--version"]),
    execFileAsync("rustc", ["--version"]),
    execFileAsync("rustc", ["-vV"]),
  ]);
  assert.equal(configuration.environment_contract.cargo, cargo.trim());
  assert.equal(configuration.environment_contract.rustc, rustc.trim());
  assert.equal(configuration.environment_contract.target, verbose.match(/^host:\s*(\S+)$/m)[1]);
  assert.deepEqual(builds.map(({ example, sourceFingerprint }) => [example, sourceFingerprint]), [
    ["escrow_performance_baseline_adapter", prepared.baseline_source_fingerprint],
    ["escrow_performance_endpoint", prepared.current_source_fingerprint],
  ]);
  assert.equal(configuration.before.command[0], baselineBinary);
  assert.equal(configuration.after.command[0], currentBinary);
});

test("baseline materializer refuses any destructive output outside workspace .tmp", async () => {
  await assert.rejects(() => prepareBaselineAdapter({
    baselineRoot: path.resolve(WORKSPACE, "..", "escrow-baseline-7041"),
    outputRoot: WORKSPACE,
  }), /strict child of workspace \.tmp/);
});
