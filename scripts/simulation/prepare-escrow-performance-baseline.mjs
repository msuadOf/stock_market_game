#!/usr/bin/env node
/**
 * Materialize the read-only 7041d35 checkout with the reviewed historical
 * performance adapter.  The checkout itself is never changed: the adapter is
 * compiled in a workspace `.tmp` copy, and its source manifest therefore binds
 * both the exact historical tree bytes and the adapter bytes.
 */
import { cp, mkdir, readFile, realpath, rm, writeFile } from "node:fs/promises";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { createHash } from "node:crypto";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { escrowSourceManifest } from "./escrow-source-manifest.mjs";

const WORKSPACE = path.resolve(fileURLToPath(new URL("../..", import.meta.url)));
const BASELINE_COMMIT = "7041d35dc362ca74f4f3313e6804db9499f0679a";
const BASELINE_ROOT = path.resolve(WORKSPACE, "..", "escrow-baseline-7041");
const TMP_ROOT = path.join(WORKSPACE, ".tmp", "task9-baseline-adapter");
const ADAPTER_SOURCE = path.join(WORKSPACE, "scripts/simulation/fixtures/escrow_performance_baseline_adapter.rs");
const execFileAsync = promisify(execFile);

function digest(bytes) { return createHash("sha256").update(bytes).digest("hex"); }

async function canonicalDirectory(directory, label) {
  const resolved = await realpath(directory);
  if (resolved !== directory) throw new Error(`${label} must be canonical and non-symlinked`);
  return resolved;
}

export async function verifyHistoricalCheckout(root, expectedCommit = BASELINE_COMMIT) {
  const { stdout: head } = await execFileAsync("git", ["-C", root, "rev-parse", "HEAD"], { encoding: "utf8" });
  if (head.trim() !== expectedCommit) throw new Error(`historical baseline HEAD must be ${expectedCommit}, got ${head.trim()}`);
  const { stdout: status } = await execFileAsync("git", ["-C", root, "status", "--porcelain", "--untracked-files=all"], { encoding: "utf8" });
  if (status.trim() !== "") throw new Error("historical baseline worktree must be clean before adapter materialization");
}

export async function prepareBaselineAdapter({
  baselineRoot = BASELINE_ROOT,
  outputRoot = TMP_ROOT,
  adapterSource = ADAPTER_SOURCE,
} = {}) {
  await canonicalDirectory(WORKSPACE, "workspace");
  await canonicalDirectory(baselineRoot, "historical baseline root");
  const canonicalOutputParent = path.dirname(path.resolve(outputRoot));
  const canonicalTmp = path.join(WORKSPACE, ".tmp");
  if (path.resolve(outputRoot) === WORKSPACE || !path.resolve(outputRoot).startsWith(`${canonicalTmp}${path.sep}`)) {
    throw new Error("adapter output must be a strict child of workspace .tmp");
  }
  await canonicalDirectory(canonicalOutputParent, "adapter output parent");
  await verifyHistoricalCheckout(baselineRoot);
  const adapterBytes = await readFile(adapterSource);
  await rm(outputRoot, { recursive: true, force: true });
  await mkdir(path.dirname(outputRoot), { recursive: true });
  // A copy is intentional: adding an example to the immutable historical
  // worktree would invalidate its commit provenance.
  await cp(baselineRoot, outputRoot, { recursive: true, errorOnExist: false, force: true });
  const outputAdapter = path.join(outputRoot, "packages/engine/examples/escrow_performance_baseline_adapter.rs");
  await writeFile(outputAdapter, adapterBytes, { flag: "wx" });
  const manifest = await escrowSourceManifest(await canonicalDirectory(outputRoot, "adapter source root"));
  const provenance = {
    schema: "escrow-historical-performance-adapter-v1",
    baseline_commit: BASELINE_COMMIT,
    baseline_root: BASELINE_ROOT,
    adapter_source: "scripts/simulation/fixtures/escrow_performance_baseline_adapter.rs",
    adapter_sha256: digest(adapterBytes),
    source_manifest_sha256: manifest.sha256,
    source_manifest: manifest,
  };
  const provenancePath = path.join(outputRoot, "baseline-adapter-provenance.json");
  await writeFile(provenancePath, `${JSON.stringify(provenance, null, 2)}\n`, { flag: "wx" });
  return { outputRoot, adapterPath: outputAdapter, provenancePath, provenance };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  prepareBaselineAdapter().then(({ outputRoot, adapterPath, provenancePath, provenance }) => {
    console.log(JSON.stringify({ status: "PASS", outputRoot, adapterPath, provenancePath,
      baseline_commit: provenance.baseline_commit, adapter_sha256: provenance.adapter_sha256,
      source_manifest_sha256: provenance.source_manifest_sha256 }));
  }).catch((error) => {
    console.error(JSON.stringify({ status: "FAIL", error: error.message }));
    process.exitCode = 1;
  });
}
