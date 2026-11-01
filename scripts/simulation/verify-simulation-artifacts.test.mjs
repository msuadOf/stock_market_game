import assert from "node:assert/strict";

import { readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { describe, it } from "node:test";
import { verifySimulationArtifacts } from "./verify-simulation-artifacts.mjs";
import { REPO_ROOT, validAfterRoot, validSensitivityRoot, runNode, cloneRoot, installVerifierFixture } from "./verify-simulation-test-support.mjs";

installVerifierFixture({ sensitivity: true });

describe("simulation acceptance root verifier", () => {
  it("marks external market calibration inapplicable for synthetic-only worlds", async () => {
    for (const root of [validAfterRoot, validSensitivityRoot]) {
      const manifest = JSON.parse(await readFile(path.join(root, "manifest.json"), "utf8"));
      assert.equal(manifest.external_market_calibration_scope, "not_applicable_synthetic_history_only");
    }
  });

  it("rejects current manifests claiming a real-data calibration or authorization blocker", async () => {
    for (const status of ["completed", "not_completed_no_authorized_data"]) {
      const root = await cloneRoot(validAfterRoot, `external-calibration-${status}`);
      const manifestPath = path.join(root, "manifest.json");
      const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
      manifest.external_market_calibration_scope = status;
      await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
      await assert.rejects(verifySimulationArtifacts(root), /synthetic-history policy/);
    }
  });

  it("does not report a timed-out child as a successful CLI exit", async () => {
    const result = await runNode(["-e", "setInterval(() => {}, 1_000)"], REPO_ROOT, 50);
    assert.notEqual(result.code, 0);
    assert.equal(result.killed, true);
    assert.ok(result.signal);
  });

  it("prints a verified JSON summary through CLI stdout pipes", async () => {
    const result = await runNode(["scripts/simulation/verify-simulation-artifacts.mjs", validAfterRoot]);
    assert.equal(result.code, 0);
    assert.equal(result.killed, false);
    assert.equal(result.signal, null);
    assert.equal(result.stderr, "");
    assert.ok(result.stdout.endsWith("\n"));
    assert.deepEqual(JSON.parse(result.stdout), { status: "verified", ...await verifySimulationArtifacts(validAfterRoot) });
  });

  it("walks and verifies the complete 17-execution after root", async () => {
    assert.deepEqual(await verifySimulationArtifacts(validAfterRoot), {
      command: "after",
      matrices: 2,
      canonical_runs: 15,
      determinism_reruns: 2,
      total_executions: 17,
      source_fingerprint_digest: (JSON.parse(await readFile(path.join(validAfterRoot, "manifest.json"), "utf8"))).source_fingerprint.digest,
    });
  });

  it("deduplicates the three 1x rows while verifying the complete 77-execution sensitivity root", async () => {
    const summary = await verifySimulationArtifacts(validSensitivityRoot);
    assert.equal(summary.command, "sensitivity");
    assert.equal(summary.matrices, 7);
    assert.equal(summary.canonical_runs, 70);
    assert.equal(summary.determinism_reruns, 7);
    assert.equal(summary.total_executions, 77);
  });

  it("rejects a tampered raw artifact instead of trusting the manifest copy", async () => {
    const root = await cloneRoot(validAfterRoot, "raw-tamper");
    await writeFile(path.join(root, "primary-behavior-1-event-1-volume-denominator-1", "seed-1.json"), "{}\n");
    await assert.rejects(verifySimulationArtifacts(root), /raw artifact digest mismatch/);
  });

  it("rejects a changed aggregate entry digest even when the per-seed receipt remains intact", async () => {
    const root = await cloneRoot(validAfterRoot, "entry-tamper");
    const checkpointPath = path.join(root, "primary-behavior-1-event-1-volume-denominator-1", "checkpoint.json");
    const checkpoint = JSON.parse(await readFile(checkpointPath, "utf8"));
    checkpoint.completed[0].sha256 = "0".repeat(64);
    await writeFile(checkpointPath, `${JSON.stringify(checkpoint, null, 2)}\n`);
    await assert.rejects(verifySimulationArtifacts(root), /aggregate checkpoint\.completed mismatch|checkpoint digest mismatch/);
  });

  it("rejects a source fingerprint that does not self-authenticate", async () => {
    const root = await cloneRoot(validAfterRoot, "source-tamper");
    const manifestPath = path.join(root, "manifest.json");
    const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
    manifest.source_fingerprint.files.find((entry) => entry.path === "scripts/simulation/verify-simulation-artifacts.mjs").sha256 = "f".repeat(64);
    await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
    await assert.rejects(verifySimulationArtifacts(root), /source fingerprint digest mismatch/);
  });

  it("rejects a stale prebuilt binary that is not bound to the sealed source fingerprint", async () => {
    const root = await cloneRoot(validAfterRoot, "stale-binary");
    const manifestPath = path.join(root, "manifest.json");
    const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
    manifest.fixture_binary.embedded_source_fingerprint_digest = "0".repeat(64);
    await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
    await assert.rejects(verifySimulationArtifacts(root), /stale.*source fingerprint|source fingerprint.*stale/i);
  });

});
