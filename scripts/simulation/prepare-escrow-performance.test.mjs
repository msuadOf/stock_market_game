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

test("批 9 A6/A10 收尾：性能对照端点退役后准备链显式失败", async () => {
  const root = await temporaryDirectory("config-retired-");
  const request = path.join(root, "request.json");
  const setup = {
    setup: {
      simulation_policy_id: "a-share-simulation",
      stocks: [], groups: [], company_operations: null,
    },
  };
  await writeFile(request, JSON.stringify(setup));
  const baselineRoot = WORKSPACE;
  const currentRoot = WORKSPACE;
  const outputRoot = path.join(root, "out");
  await assert.rejects(
    preparePerformanceConfig({ setupRequestPath: request, baselineRoot, currentRoot, outputRoot, completedTicks: 2 }),
    /escrow 性能对照端点已随批 9 A6 退役/,
    "退役门禁必须显式失败并指明裁定出处（不静默产出半配置）",
  );
});
