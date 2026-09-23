import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { cp, mkdtemp, readFile, rm, unlink, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { after, before, describe, it } from "node:test";

import {
  buildK7ResourcePolicy,
  captureAfter as captureAfterWithPreparedFixture,
  captureSensitivity as captureSensitivityWithPreparedFixture,
} from "./baseline-run.mjs";
import { verifyK7Root } from "./verify-k7-root.mjs";
import { prepareWorkspacePaths } from "../workspace-paths.mjs";

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const TEST_REVISION = "6ad461e7f735ee1a0b4090497c58380af3422761";
const TEST_TREE = "d6e7c4a051f231a789ef589c4ec33580ad082109";
const RESOURCE_POLICY = buildK7ResourcePolicy(2, "verification_test");
const FAKE_K7_EXECUTABLE = ".tmp/build-cache/k7/release/examples/k7_baseline_fixture";
const tempDirs = [];
let validAfterRoot;
let validSensitivityRoot;
const TEST_WORKSPACE_PATHS = await prepareWorkspacePaths({ sourceRoot: REPO_ROOT, scope: "k7" });

async function fakePrepareFixture({ sourceFingerprint, workspacePaths }) {
  return {
    executable_path: path.join(workspacePaths.workspaceRoot, ...FAKE_K7_EXECUTABLE.split("/")),
    executable_relative_path: FAKE_K7_EXECUTABLE,
    binary_sha256: "b".repeat(64),
    binary_bytes: 2048,
    embedded_source_fingerprint_digest: sourceFingerprint.digest,
    build_argv: ["cargo", "build", "-p", "engine", "--release", "--features", "simulation-diagnostics", "--example", "k7_baseline_fixture", "--message-format=json-render-diagnostics"],
    cargo_build_jobs: RESOURCE_POLICY.available_cpu_count,
    workspace_root: workspacePaths.workspaceRoot,
    cargo_target_dir: workspacePaths.cargoTargetDir,
    process_tmp_dir: workspacePaths.processTmpDir,
    build_wall_ms: 1,
  };
}

function captureAfter(options) {
  return captureAfterWithPreparedFixture({ prepareFixture: fakePrepareFixture, workspacePaths: TEST_WORKSPACE_PATHS, ...options });
}

function captureSensitivity(options) {
  return captureSensitivityWithPreparedFixture({ prepareFixture: fakePrepareFixture, workspacePaths: TEST_WORKSPACE_PATHS, ...options });
}

async function newTempDir(prefix = "verify-k7-root-test-") {
  assert.ok(path.isAbsolute(process.env.TMPDIR ?? ""), "TMPDIR must be an absolute workspace path");
  const directory = await mkdtemp(path.join(process.env.TMPDIR, prefix));
  tempDirs.push(directory);
  return directory;
}

function fakeK7Exec() {
  return async (file, args, options) => {
    if (file === "git" && args[0] === "rev-parse" && args[1] === "HEAD") return { code: 0, stdout: `${TEST_REVISION}\n`, stderr: "" };
    if (file === "git" && args[0] === "rev-parse" && args[1] === "HEAD^{tree}") return { code: 0, stdout: `${TEST_TREE}\n`, stderr: "" };
    if (file === "git" && args[0] === "branch") return { code: 0, stdout: "verification-fixture\n", stderr: "" };
    if (file === "git" && (args[0] === "status" || args[0] === "diff" || args[0] === "ls-files")) return { code: 0, stdout: "", stderr: "" };
    if (file === FAKE_K7_EXECUTABLE || file.endsWith(`/${FAKE_K7_EXECUTABLE}`)) {
      const scenario = args[0];
      const seed = args[1];
      const naturalDays = Number(args[2]);
      const behavior = Number(args[3]);
      const event = Number(args[4]);
      const c01 = Number(args[5]);
      const crossYear = scenario === "cross-year";
      const ticks = crossYear ? 20 : 30;
      return {
        code: 0,
        stdout: JSON.stringify({
          tool: "k7_baseline_fixture",
          source: "fresh_current_k7_setup",
          build_source_fingerprint: options.env.K7_SOURCE_FINGERPRINT_DIGEST,
          scenario,
          seed,
          natural_days: naturalDays,
          calendar: { natural_days: naturalDays, trading_days: naturalDays, closed_days: 0, policy_id: "a-share-simulation-v1" },
          verification_profile: {
            schema: "k7-bounded-representative-profile-v1",
            profile_id: `${scenario}-bounded-representative-v1`,
            scope: "bounded_representative_not_full_market_scale",
            retail_count: crossYear ? 32 : 64,
            inst_count: 5,
            hot_count: 2,
            stock_count: 5,
            ticks_per_trading_day: ticks,
            opening_auction_ticks: 3,
            continuous_ticks: ticks - 5,
            closing_auction_ticks: 2,
            start_date: crossYear ? "2030-12-27" : "2030-01-01",
            market_phases: ["opening_auction", "continuous", "closing_auction"],
          },
          // Rust's serde_json map order is not a wire-contract requirement.
          // Keep this deliberately different from the verifier's expected key
          // construction order so semantic object equality is exercised.
          multipliers: { behavior, c01_denominator_assumption: c01, event },
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

  it("rejects a stale prebuilt binary that is not bound to the sealed source fingerprint", async () => {
    const root = await cloneRoot(validAfterRoot, "stale-binary");
    const manifestPath = path.join(root, "manifest.json");
    const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
    manifest.fixture_binary.embedded_source_fingerprint_digest = "0".repeat(64);
    await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
    await assert.rejects(verifyK7Root(root), /stale.*source fingerprint|source fingerprint.*stale/i);
  });

  it("rejects fixture build paths that do not resolve to the sealed workspace cache", async () => {
    const root = await cloneRoot(validAfterRoot, "workspace-path-tamper");
    const manifestPath = path.join(root, "manifest.json");
    const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
    manifest.fixture_binary.process_tmp_dir = "/tmp/escaped-k7";
    manifest.fixture_build.process_tmp_dir = "/tmp/escaped-k7";
    await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
    await assert.rejects(verifyK7Root(root), /workspace paths are invalid/i);
  });

  it("rejects a tampered concurrent resource policy that violates the aggregate budget contract", async () => {
    const root = await cloneRoot(validAfterRoot, "resource-policy-tamper");
    const manifestPath = path.join(root, "manifest.json");
    const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
    manifest.resource_policy.max_concurrent_child_executions += 1;
    await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
    await assert.rejects(verifyK7Root(root), /concurrent child budget is inconsistent|aggregate Rayon budget exceeds/);
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
