import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile, unlink, writeFile } from "node:fs/promises";
import path from "node:path";
import { describe, it } from "node:test";
import { verifySimulationArtifacts } from "./verify-simulation-artifacts.mjs";
import { REPO_ROOT, RESOURCE_POLICY, validAfterRoot, captureAfter, newTempDir, fakeSimulationExec, runNode, cloneRoot, installVerifierFixture } from "./verify-simulation-test-support.mjs";

installVerifierFixture();

describe("simulation acceptance root verifier", () => {
  it("rejects fixture build paths that do not resolve to the sealed workspace cache", async () => {
    const root = await cloneRoot(validAfterRoot, "workspace-path-tamper");
    const manifestPath = path.join(root, "manifest.json");
    const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
    manifest.fixture_binary.process_tmp_dir = "/tmp/escaped-workspace-cache";
    manifest.fixture_build.process_tmp_dir = "/tmp/escaped-workspace-cache";
    await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
    await assert.rejects(verifySimulationArtifacts(root), /workspace paths are invalid/i);
  });

  it("rejects a tampered concurrent resource policy that violates the aggregate budget contract", async () => {
    const root = await cloneRoot(validAfterRoot, "resource-policy-tamper");
    const manifestPath = path.join(root, "manifest.json");
    const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
    manifest.resource_policy.max_concurrent_child_executions += 1;
    await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
    await assert.rejects(verifySimulationArtifacts(root), /concurrent child budget is inconsistent|aggregate Rayon budget exceeds/);
  });

  it("rejects a determinism receipt not bound to the canonical raw bytes", async () => {
    const root = await cloneRoot(validAfterRoot, "determinism-tamper");
    const receiptPath = path.join(root, "primary-behavior-1-event-1-volume-denominator-1", "determinism.checkpoint.json");
    const receipt = JSON.parse(await readFile(receiptPath, "utf8"));
    receipt.rerun_digest = "a".repeat(64);
    await writeFile(receiptPath, `${JSON.stringify(receipt, null, 2)}\n`);
    await assert.rejects(verifySimulationArtifacts(root), /not bound to the canonical and rerun bytes/);
  });

  it("接受自由 rerun 的合法输出差异并保留各自原始证据", async () => {
    const root = await newTempDir("free-rerun-");
    const execute = fakeSimulationExec();
    let invocation = 0;
    await captureAfter({ outputDir: root, repoRoot: REPO_ROOT, resourcePolicy: RESOURCE_POLICY,
      exec: async (...arguments_) => {
        const result = await execute(...arguments_);
        if (arguments_[0].endsWith("simulation_baseline_fixture")) {
          result.stdout = `${result.stdout}${" ".repeat(++invocation)}`;
        }
        return result;
      },
    });
    assert.equal((await verifySimulationArtifacts(root)).determinism_reruns, 2);
    const receipt = JSON.parse(await readFile(path.join(root, "primary-behavior-1-event-1-volume-denominator-1", "determinism.checkpoint.json"), "utf8"));
    assert.equal(receipt.schema, "simulation-determinism-receipt");
    assert.equal(receipt.schema_version, 2);
    assert.equal(receipt.identity.runner.runner_policy_version, 9);
    assert.notEqual(receipt.first_digest, receipt.rerun_digest);
    assert.equal(receipt.identical, false);
    await writeFile(path.join(root, "primary-behavior-1-event-1-volume-denominator-1", "rerun.json"), "{}");
    await assert.rejects(verifySimulationArtifacts(root), /rerun bytes/);
  });

  it("拒绝旧工具收据版本，不建立兼容读取路径", async () => {
    const root = await cloneRoot(validAfterRoot, "old-receipt-version");
    const receiptPath = path.join(root, "primary-behavior-1-event-1-volume-denominator-1", "determinism.checkpoint.json");
    const receipt = JSON.parse(await readFile(receiptPath, "utf8"));
    receipt.schema_version = 1;
    await writeFile(receiptPath, JSON.stringify(receipt));
    await assert.rejects(verifySimulationArtifacts(root), /receipt schema is unsupported/);
    await assert.rejects(captureAfter({ outputDir: root, exec: fakeSimulationExec(), repoRoot: REPO_ROOT,
      resourcePolicy: RESOURCE_POLICY, resume: true }), /receipt schema is unsupported/);
  });

  it("root 与 resume 拒绝重新绑定 hash 但来源非法的 rerun", async () => {
    const root = await cloneRoot(validAfterRoot, "rerun-source-tamper");
    const directory = path.join(root, "primary-behavior-1-event-1-volume-denominator-1");
    const raw = JSON.parse(await readFile(path.join(directory, "rerun.json"), "utf8"));
    raw.build_source_fingerprint = "a".repeat(64);
    const bytes = JSON.stringify(raw);
    const receiptPath = path.join(directory, "determinism.checkpoint.json");
    const receipt = JSON.parse(await readFile(receiptPath, "utf8"));
    receipt.rerun_digest = createHash("sha256").update(bytes).digest("hex");
    receipt.identical = false;
    const { receipt_digest: previousDigest, ...receiptState } = receipt;
    assert.ok(previousDigest);
    receipt.receipt_digest = createHash("sha256").update(JSON.stringify(receiptState)).digest("hex");
    await writeFile(path.join(directory, "rerun.json"), bytes);
    await writeFile(receiptPath, JSON.stringify(receipt));
    await assert.rejects(verifySimulationArtifacts(root), /stale fixture binary/);
    await assert.rejects(captureAfter({ outputDir: root, exec: fakeSimulationExec(), repoRoot: REPO_ROOT,
      resourcePolicy: RESOURCE_POLICY, resume: true }), /source fingerprint|stale.*binary/);
  });

  it("rejects missing and unreferenced artifacts during the root walk", async () => {
    const missing = await cloneRoot(validAfterRoot, "missing-artifact");
    await unlink(path.join(missing, "primary-behavior-1-event-1-volume-denominator-1", "seed-1.checkpoint.json"));
    await assert.rejects(verifySimulationArtifacts(missing), /artifact set mismatch.*missing=.*seed-1\.checkpoint\.json/);

    const extra = await cloneRoot(validAfterRoot, "extra-artifact");
    await writeFile(path.join(extra, "unsealed-result.json"), "{}\n");
    await assert.rejects(verifySimulationArtifacts(extra), /simulation acceptance root artifact set mismatch.*extra=.*unsealed-result\.json/);
  });

  it("returns CLI exit code 1 when an entry digest is corrupted", async () => {
    const root = await cloneRoot(validAfterRoot, "cli-negative");
    const checkpointPath = path.join(root, "primary-behavior-1-event-1-volume-denominator-1", "checkpoint.json");
    const checkpoint = JSON.parse(await readFile(checkpointPath, "utf8"));
    checkpoint.completed[0].sha256 = "0".repeat(64);
    await writeFile(checkpointPath, `${JSON.stringify(checkpoint, null, 2)}\n`);
    const result = await runNode(["scripts/simulation/verify-simulation-artifacts.mjs", root]);
    assert.equal(result.code, 1);
    assert.equal(result.killed, false);
    assert.equal(result.signal, null);
    assert.equal(result.stderr, "simulation acceptance root verification failed: matrix primary-behavior-1-event-1-volume-denominator-1 aggregate checkpoint.completed mismatch\n");
    assert.ok(result.stderr.endsWith("\n"));
    assert.equal(result.stdout, "");
  });
});
