import assert from "node:assert/strict";
import { mkdtemp, mkdir, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { execFile } from "node:child_process";
import { promisify } from "node:util";

import { inspectReleaseArtifacts, deriveDiagnosticSymbols } from "./release-contract.mjs";

test("Given a freshly built production Web surface When release gate runs Then diagnostic trace code is absent", { skip: process.env.RELEASE_BUILT_TEST !== "1" }, async () => {
  assert.equal((await inspectReleaseArtifacts("apps/web/dist")).contaminated, false);
});

test("Given a release asset tree with a diagnostics chunk When artifact inspection runs Then it rejects the contaminated surface", async (context) => {
  const root = await mkdtemp(join(tmpdir(), "release-contract-contaminated-"));
  context.after(() => rm(root, { recursive: true, force: true }));
  await mkdir(join(root, "assets"));
  await writeFile(join(root, "assets", "diagnostics.js"), "npc_decision_trace");

  await assert.rejects(inspectReleaseArtifacts(root), /diagnostics artifact/);
});

test("Given a clean release asset tree When artifact inspection runs Then it records a clean inventory", async (context) => {
  const root = await mkdtemp(join(tmpdir(), "release-contract-clean-"));
  context.after(() => rm(root, { recursive: true, force: true }));
  await mkdir(join(root, "assets"));
  await writeFile(join(root, "assets", "app.js"), "export const ready = true;");

  const inventory = await inspectReleaseArtifacts(root);

  assert.equal(inventory.contaminated, false);
  assert.equal(inventory.files.length, 1);
});

for (const marker of ["npc_decision_trace", "NpcDecisionCollector", "npc_decision_diagnostics", "decision_records", "DecisionDiagnosticsCollector", "diagnostics-worker", "simulation-diagnostics"]) {
  test(`Given bundled ${marker} When inspected Then renamed chunks cannot evade rejection`, async (context) => {
    const root = await mkdtemp(join(tmpdir(), "release-contract-marker-"));
    context.after(() => rm(root, { recursive: true, force: true }));
    await writeFile(join(root, "app-a1b2.js"), `const value = ${JSON.stringify(marker)};`);
    await assert.rejects(inspectReleaseArtifacts(root), /diagnostics artifact/);
  });
}

test("Given an empty surface When inspected Then missing artifacts fail closed", async (context) => {
  const root = await mkdtemp(join(tmpdir(), "release-contract-empty-"));
  context.after(() => rm(root, { recursive: true, force: true }));
  await assert.rejects(inspectReleaseArtifacts(root), /empty/);
});

test("Given a clean clone When a diagnostic chunk is injected Then the CLI switches from zero to nonzero", async (context) => {
  const root = await mkdtemp(join(tmpdir(), "release-contract-cli-"));
  context.after(() => rm(root, { recursive: true, force: true }));
  await writeFile(join(root, "app.js"), "export const ready = true;");
  const run = promisify(execFile);
  const args = ["scripts/simulation/release-contract.mjs", "--inspect", root];
  const clean = await run(process.execPath, args);
  assert.equal(JSON.parse(clean.stdout).contaminated, false);
  await writeFile(join(root, "diagnostics.js"), "npc_decision_trace");
  await assert.rejects(run(process.execPath, args), (error) => {
    assert.equal(error.code, 1);
    assert.match(error.stderr, /diagnostics artifact/);
    return true;
  });
});

test("Given a diagnostic binary with one unique symbol When symbols are derived Then a default binary containing it is rejected", async (context) => {
  const root = await mkdtemp(join(tmpdir(), "release-contract-symbols-"));
  context.after(() => rm(root, { recursive: true, force: true }));
  const feature = join(root, "feature");
  const release = join(root, "release");
  await writeFile(feature, "core_symbol\nNpcDecisionCollector\nother_symbol\n");
  await writeFile(release, "core_symbol\n");
  const symbols = await deriveDiagnosticSymbols(feature, release);
  assert.deepEqual(symbols, ["NpcDecisionCollector"]);
});
