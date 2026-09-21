import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { describe, it } from "node:test";
import { escrowSourceManifest } from "./escrow-source-manifest.mjs";

import {
  runTask9Matrix,
  sha256Hex,
} from "./run-escrow-task9-matrix.mjs";

async function fixture() {
  const processTemp = process.env.TMPDIR;
  assert.ok(processTemp, "tests require the registered workspace-local TMPDIR");
  const root = await mkdtemp(path.join(processTemp, "task9-matrix-runner-test-"));
  const workspaceRoot = path.join(root, "workspace");
  const tempRoot = path.join(workspaceRoot, ".tmp");
  const sourceRoot = path.join(workspaceRoot, ".worktree", "source");
  const targetDir = path.join(tempRoot, "build-cache", "runner");
  const childTemp = path.join(tempRoot, "process-tmp", "runner");
  const logsDir = path.join(tempRoot, "runner", "logs");
  const outputRoot = path.join(tempRoot, "runner", "evidence");
  await Promise.all([
    mkdir(sourceRoot, { recursive: true }),
    mkdir(targetDir, { recursive: true }),
    mkdir(childTemp, { recursive: true }),
    mkdir(logsDir, { recursive: true }),
  ]);
  return {
    config: {
      workspaceRoot,
      sourceRoot,
      outputRoot,
      logsDir,
      targetDir,
      processTemp: childTemp,
      seed: "17",
      sourceFingerprint: (await escrowSourceManifest(sourceRoot)).sha256,
    },
  };
}

function fakeHarness({ status = "PASS", exitCode = 0, drift = false, determinismDrift = false } = {}) {
  const calls = [];
  const invocations = [];
  const runChild = async ({ command, args, entry, output, env }) => {
    calls.push(entry);
    invocations.push({ command, args, output });
    assert.equal(env.TMPDIR, env.TMP);
    assert.equal(env.TMP, env.TEMP);
    assert.ok(env.TMPDIR.includes(`${path.sep}.tmp${path.sep}`));
    await assert.rejects(readFile(path.join(output, "capture.json")));
    await mkdir(output);
    const negative = entry.mode === "negative-control";
    const values = {
      authoritative_state: Buffer.from(negative ? `negative-${entry.disabledMerge}` : "state"),
      event_stream: Buffer.from(negative
        ? `events-${entry.disabledMerge}`
        : determinismDrift && entry.id === "budget-2-repeat-0-canonical" ? "different-events" : "events"),
      receipts: Buffer.from(negative ? `receipts-${entry.disabledMerge}` : "receipts"),
      save_slot: Buffer.from("save"),
    };
    const artifacts = {};
    for (const [name, bytes] of Object.entries(values)) {
      const file = `${name.replaceAll("_", "-")}.json`;
      await writeFile(path.join(output, file), bytes);
      artifacts[name] = {
        file,
        receipt: {
          byte_length: String(bytes.length),
          sha256: sha256Hex(bytes),
        },
      };
    }
    if (drift && entry.id === "budget-2-repeat-0-canonical") {
      await writeFile(path.join(output, artifacts.event_stream.file), "tampered");
    }
    const capture = {
      schema: "escrow-runtime-evidence-capture-v1",
      status,
      configuration: {
        scenario: "task9-runtime-v1",
        seed: entry.seed,
        budget: entry.budget,
        repeat: entry.repeat,
        mode: entry.mode,
        requested_scheduler_merge_disabled: entry.disabledMerge,
      },
      artifacts,
      scheduler_precanonical_order: { available: true, records: [
        ["P3AccountShards", ["1", "2"]], ["P4AuctionStockShards", ["000812", "600101"]],
        ["P5ReceiptResults", ["receipt-1", "receipt-2"]],
      ].map(([boundary, identities]) => ({ boundary, identities: entry.mode === "canonical" ? identities : [...identities].reverse(), item_counts: [1, 1] })) },
      runtime_coverage: { tick_from: "1", tick_to: "1" },
      producer_readiness: { conservation_snapshots: [{
        schema: "escrow-conservation-snapshot-v1", scenario: entry.scenario, seed: entry.seed, tick: "1", envelopes: [],
        accounts: [{ account_id: "0", cash_cents: "10", positions: [], aggregate: {
          left: { cash_cents: "0", shares: "0" }, right: { cash_cents: "0", shares: "0" },
        } }],
      }] },
      negative_control: negative ? { detected: true, kind: "typed-rejection-with-rollback", disabled_merge: entry.disabledMerge,
        enabled_merge_same_step_passed: true, failed_step_published_events: "0", error: "expected invariant failure",
        before_failed_step: { sha256: sha256Hex("before"), byte_length: "6" },
        after_failed_step: { sha256: sha256Hex("before"), byte_length: "6" },
      } : null,
    };
    await writeFile(path.join(output, "capture.json"), JSON.stringify(capture));
    return {
      code: exitCode,
      signal: null,
      stdout: JSON.stringify({ status, capture: path.join(output, "capture.json") }),
      stderr: "",
    };
  };
  return { calls, invocations, runChild };
}

describe("Task 9 matrix runner", () => {
  it("runs four budgets, two repeats, two modes and exactly three negative controls", async () => {
    const { config } = await fixture();
    const fake = fakeHarness();
    const summary = await runTask9Matrix(config, { runChild: fake.runChild });

    assert.equal(summary.status, "PASS");
    assert.equal(summary.entries.length, 19);
    assert.equal(fake.calls.filter((entry) => entry.mode === "negative-control").length, 3);
    assert.deepEqual(
      fake.calls.filter((entry) => entry.mode === "negative-control").map((entry) => entry.disabledMerge),
      ["account", "stock", "completion"],
    );
    assert.deepEqual(fake.invocations[0].args, [
      "run", "--quiet", "-p", "engine", "--features", "verification-harness", "--example", "escrow_verification_harness", "--",
      "--scenario", "task9-runtime-v1", "--seed", "17", "--budget", "1", "--repeat", "0",
      "--mode", "canonical", "--output", fake.invocations[0].output,
    ]);
    assert.deepEqual(fake.invocations.at(-1).args.slice(-6), [
      "--mode", "negative-control", "--disable-merge", "completion", "--output", fake.invocations.at(-1).output,
    ]);
    const receipt = await readFile(path.join(config.outputRoot, "determinism.sha256"), "utf8");
    assert.equal(receipt.trim().split("\n").length, 19 * 4);
    assert.equal(JSON.parse(await readFile(path.join(config.outputRoot, "summary.json"), "utf8")).status, "PASS");
  });

  it("never promotes BLOCKED output or a nonzero process exit to PASS", async () => {
    const blocked = await fixture();
    const blockedHarness = fakeHarness({ status: "BLOCKED", exitCode: 3 });
    const blockedSummary = await runTask9Matrix(blocked.config, { runChild: blockedHarness.runChild });
    assert.equal(blockedSummary.status, "FAIL");
    assert.equal(blockedSummary.failure.code, "HARNESS_BLOCKED");

    const failed = await fixture();
    const failedHarness = fakeHarness({ status: "PASS", exitCode: 7 });
    const failedSummary = await runTask9Matrix(failed.config, { runChild: failedHarness.runChild });
    assert.equal(failedSummary.status, "FAIL");
    assert.equal(failedSummary.failure.code, "EXIT_STATUS_MISMATCH");
  });

  it("fails when an artifact no longer matches its declared byte length or SHA-256", async () => {
    const { config } = await fixture();
    const fake = fakeHarness({ drift: true });
    const summary = await runTask9Matrix(config, { runChild: fake.runChild });
    assert.equal(summary.status, "FAIL");
    assert.equal(summary.failure.code, "ARTIFACT_HASH_DRIFT");
  });

  it("fails when individually valid receipts drift across budget/repeat observations", async () => {
    const { config } = await fixture();
    const fake = fakeHarness({ determinismDrift: true });
    const summary = await runTask9Matrix(config, { runChild: fake.runChild });
    assert.equal(summary.status, "FAIL");
    assert.equal(summary.failure.code, "DETERMINISM_DRIFT");
  });

  it("reports a typed FAIL when the harness itself reports FAIL", async () => {
    const { config } = await fixture();
    const fake = fakeHarness({ status: "FAIL", exitCode: 1 });
    const summary = await runTask9Matrix(config, { runChild: fake.runChild });
    assert.equal(summary.status, "FAIL");
    assert.equal(summary.failure.code, "HARNESS_FAIL");
  });

  it("treats an identical duplicate notification as idempotent and does not rerun children", async () => {
    const { config } = await fixture();
    const first = fakeHarness();
    assert.equal((await runTask9Matrix(config, { runChild: first.runChild })).status, "PASS");

    let duplicateCalls = 0;
    const duplicate = await runTask9Matrix(config, {
      runChild: async () => {
        duplicateCalls += 1;
        throw new Error("duplicate notification must not rerun a completed matrix");
      },
    });
    assert.equal(duplicate.status, "PASS");
    assert.equal(duplicate.reused, true);
    assert.equal(duplicateCalls, 0);
  });

  it("refuses to rerun a duplicate whose completed evidence was modified", async () => {
    const { config } = await fixture();
    const first = fakeHarness();
    assert.equal((await runTask9Matrix(config, { runChild: first.runChild })).status, "PASS");
    await writeFile(path.join(config.outputRoot, "runs", "budget-1-repeat-0-canonical", "receipts.json"), "modified");

    let duplicateCalls = 0;
    const duplicate = await runTask9Matrix(config, {
      runChild: async () => {
        duplicateCalls += 1;
        throw new Error("modified duplicate must fail closed without rerunning");
      },
    });
    assert.equal(duplicate.status, "FAIL");
    assert.equal(duplicate.failure.code, "ARTIFACT_HASH_DRIFT");
    assert.equal(duplicateCalls, 0);
  });

  it("refuses a duplicate whose persisted complete source manifest was modified", async () => {
    const { config } = await fixture();
    const first = fakeHarness();
    assert.equal((await runTask9Matrix(config, { runChild: first.runChild })).status, "PASS");
    const manifestPath = path.join(config.outputRoot, "source-manifest.json");
    const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
    manifest.absent.push("forged-input");
    await writeFile(manifestPath, JSON.stringify(manifest));

    let duplicateCalls = 0;
    const duplicate = await runTask9Matrix(config, {
      runChild: async () => {
        duplicateCalls += 1;
        throw new Error("tampered source binding must fail closed without rerunning");
      },
    });
    assert.equal(duplicate.status, "FAIL");
    assert.equal(duplicate.failure.code, "SOURCE_HASH_DRIFT");
    assert.equal(duplicateCalls, 0);
  });
});
