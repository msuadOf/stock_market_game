import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { cp, mkdtemp, readFile, rm, unlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { after, before, describe, it } from "node:test";

import { buildK7ResourcePolicy, captureAfter, captureSensitivity } from "./baseline-run.mjs";
import { verifyK7Root } from "./verify-k7-root.mjs";

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const TEST_REVISION = "6ad461e7f735ee1a0b4090497c58380af3422761";
const TEST_TREE = "d6e7c4a051f231a789ef589c4ec33580ad082109";
const RESOURCE_POLICY = buildK7ResourcePolicy(2, "verification_test");
const tempDirs = [];
let validAfterRoot;
let validSensitivityRoot;

async function newTempDir(prefix = "verify-k7-root-test-") {
  const directory = await mkdtemp(path.join(tmpdir(), prefix));
  tempDirs.push(directory);
  return directory;
}

function fakeK7Exec() {
  return async (file, args) => {
    if (file === "git" && args[0] === "rev-parse" && args[1] === "HEAD") return { code: 0, stdout: `${TEST_REVISION}\n`, stderr: "" };
    if (file === "git" && args[0] === "rev-parse" && args[1] === "HEAD^{tree}") return { code: 0, stdout: `${TEST_TREE}\n`, stderr: "" };
    if (file === "git" && args[0] === "branch") return { code: 0, stdout: "verification-fixture\n", stderr: "" };
    if (file === "git" && (args[0] === "status" || args[0] === "diff" || args[0] === "ls-files")) return { code: 0, stdout: "", stderr: "" };
    if (file === "cargo" && args.includes("k7_baseline_fixture")) {
      const marker = args.indexOf("--");
      const scenario = args[marker + 1];
      const seed = args[marker + 2];
      const naturalDays = Number(args[marker + 3]);
      const behavior = Number(args[marker + 4]);
      const event = Number(args[marker + 5]);
      const c01 = Number(args[marker + 6]);
      return {
        code: 0,
        stdout: JSON.stringify({
          tool: "k7_baseline_fixture",
          source: "fresh_current_k7_setup",
          scenario,
          seed,
          natural_days: naturalDays,
          calendar: { natural_days: naturalDays, trading_days: naturalDays, closed_days: 0, policy_id: "a-share-simulation-v1" },
          multipliers: { behavior, event, c01_denominator_assumption: c01 },
          price_volume: { runs: [{ seed, stocks: {}, retail_execution: { filled_share_ratio: 1 } }], stocks: {} },
          causal: { ratio_absent_reason: null },
        }),
        stderr: "",
      };
    }
    throw new Error(`unexpected fixture command: ${file} ${args.join(" ")}`);
  };
}

function runNode(args, cwd = REPO_ROOT) {
  return new Promise((resolve) => {
    execFile(process.execPath, args, { cwd }, (error, stdout, stderr) => {
      resolve({ code: error?.code ?? 0, stdout, stderr });
    });
  });
}

async function cloneRoot(source, name) {
  const parent = await newTempDir();
  const target = path.join(parent, name);
  await cp(source, target, { recursive: true });
  return target;
}

before(async () => {
  const fixtureParent = await newTempDir("verify-k7-root-valid-");
  validAfterRoot = path.join(fixtureParent, "after");
  validSensitivityRoot = path.join(fixtureParent, "sensitivity");
  const exec = fakeK7Exec();
  await captureAfter({ outputDir: validAfterRoot, exec, repoRoot: REPO_ROOT, resourcePolicy: RESOURCE_POLICY });
  await captureSensitivity({ outputDir: validSensitivityRoot, exec, repoRoot: REPO_ROOT, resourcePolicy: RESOURCE_POLICY });
});

after(async () => {
  await Promise.all(tempDirs.map((directory) => rm(directory, { force: true, recursive: true })));
});

describe("K7 root verifier", () => {
  it("walks and verifies the complete 17-execution after root", async () => {
    assert.deepEqual(await verifyK7Root(validAfterRoot), {
      command: "after",
      matrices: 2,
      canonical_runs: 15,
      determinism_reruns: 2,
      total_executions: 17,
      source_fingerprint_digest: (JSON.parse(await readFile(path.join(validAfterRoot, "manifest.json"), "utf8"))).source_fingerprint.digest,
    });
  });

  it("deduplicates the three 1x rows while verifying the complete 77-execution sensitivity root", async () => {
    const summary = await verifyK7Root(validSensitivityRoot);
    assert.equal(summary.command, "sensitivity");
    assert.equal(summary.matrices, 7);
    assert.equal(summary.canonical_runs, 70);
    assert.equal(summary.determinism_reruns, 7);
    assert.equal(summary.total_executions, 77);
  });

  it("rejects a tampered raw artifact instead of trusting the manifest copy", async () => {
    const root = await cloneRoot(validAfterRoot, "raw-tamper");
    await writeFile(path.join(root, "primary-b1-e1-c1", "seed-1.json"), "{}\n");
    await assert.rejects(verifyK7Root(root), /raw artifact digest mismatch/);
  });

  it("rejects a changed aggregate entry digest even when the per-seed receipt remains intact", async () => {
    const root = await cloneRoot(validAfterRoot, "entry-tamper");
    const checkpointPath = path.join(root, "primary-b1-e1-c1", "checkpoint.json");
    const checkpoint = JSON.parse(await readFile(checkpointPath, "utf8"));
    checkpoint.completed[0].sha256 = "0".repeat(64);
    await writeFile(checkpointPath, `${JSON.stringify(checkpoint, null, 2)}\n`);
    await assert.rejects(verifyK7Root(root), /aggregate checkpoint\.completed mismatch|checkpoint digest mismatch/);
  });

  it("rejects a source fingerprint that does not self-authenticate", async () => {
    const root = await cloneRoot(validAfterRoot, "source-tamper");
    const manifestPath = path.join(root, "manifest.json");
    const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
    manifest.source_fingerprint.files.find((entry) => entry.path === "scripts/simulation/verify-k7-root.mjs").sha256 = "f".repeat(64);
    await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
    await assert.rejects(verifyK7Root(root), /source fingerprint digest mismatch/);
  });

  it("rejects a determinism receipt not bound to the canonical raw bytes", async () => {
    const root = await cloneRoot(validAfterRoot, "determinism-tamper");
    const receiptPath = path.join(root, "primary-b1-e1-c1", "determinism.checkpoint.json");
    const receipt = JSON.parse(await readFile(receiptPath, "utf8"));
    receipt.rerun_digest = "a".repeat(64);
    await writeFile(receiptPath, `${JSON.stringify(receipt, null, 2)}\n`);
    await assert.rejects(verifyK7Root(root), /not bound to the canonical and rerun bytes/);
  });

  it("rejects missing and unreferenced artifacts during the root walk", async () => {
    const missing = await cloneRoot(validAfterRoot, "missing-artifact");
    await unlink(path.join(missing, "primary-b1-e1-c1", "seed-1.checkpoint.json"));
    await assert.rejects(verifyK7Root(missing), /artifact set mismatch.*missing=.*seed-1\.checkpoint\.json/);

    const extra = await cloneRoot(validAfterRoot, "extra-artifact");
    await writeFile(path.join(extra, "unsealed-result.json"), "{}\n");
    await assert.rejects(verifyK7Root(extra), /K7 root artifact set mismatch.*extra=.*unsealed-result\.json/);
  });

  it("returns CLI exit code 1 when an entry digest is corrupted", async () => {
    const root = await cloneRoot(validAfterRoot, "cli-negative");
    const checkpointPath = path.join(root, "primary-b1-e1-c1", "checkpoint.json");
    const checkpoint = JSON.parse(await readFile(checkpointPath, "utf8"));
    checkpoint.completed[0].sha256 = "0".repeat(64);
    await writeFile(checkpointPath, `${JSON.stringify(checkpoint, null, 2)}\n`);
    const result = await runNode(["scripts/simulation/verify-k7-root.mjs", root]);
    assert.equal(result.code, 1);
    assert.match(result.stderr, /K7 root verification failed/);
    assert.equal(result.stdout, "");
  });
});
