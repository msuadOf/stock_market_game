import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { chmod, mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { promisify } from "node:util";
import { afterEach, test } from "node:test";
import { fileURLToPath } from "node:url";
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
    return request.targetRoot.endsWith("baseline-target") ? baselineBinary : currentBinary;
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
  // 两侧都使用现行 escrow_performance_endpoint；baseline 语义 = 任意当前代码 checkout。
  assert.deepEqual(builds.map(({ example, sourceFingerprint }) => [example, sourceFingerprint]), [
    ["escrow_performance_endpoint", prepared.baseline_source_fingerprint],
    ["escrow_performance_endpoint", prepared.current_source_fingerprint],
  ]);
  assert.equal(configuration.before.command[0], baselineBinary);
  assert.equal(configuration.after.command[0], currentBinary);
});
