import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { buildFullRegressionArtifacts } from "./run-full-regression.mjs";
import { resolveWorkspaceRoot } from "./workspace-paths.mjs";

export const CARGO_BUILD_JOBS_ENV = "STOCK_GAME_FULL_REGRESSION_CARGO_BUILD_JOBS";

export async function createSealedFixture({ buildOptions = {}, fingerprint = { algorithm: "test", algorithm_version: 1, files: [], digest: "a".repeat(64) } } = {}) {
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
    ...buildOptions,
    collectFingerprint: async () => fingerprint,
    run: async (options) => {
      assert.equal(options.captureOutput, true);
      if (buildOptions.run) return buildOptions.run(options, buildStdout);
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

export function resealInventory(record) {
  const { identity_digest: ignored, ...state } = record;
  return { ...record, identity_digest: createHash("sha256").update(JSON.stringify(state)).digest("hex") };
}

export async function createExternalTargetFixture() {
  const root = await mkdtemp(path.join(await resolveWorkspaceRoot(process.cwd()), ".tmp", "external-target-"));
  const cwd = path.join(root, "workspace");
  const targetDir = path.join(root, "target");
  await Promise.all([mkdir(path.join(cwd, ".git"), { recursive: true }), mkdir(targetDir)]);
  return { root, cwd, targetDir, cleanup: () => rm(root, { recursive: true, force: true }) };
}
