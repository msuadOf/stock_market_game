import assert from "node:assert/strict";
import test from "node:test";

import {
  assertParityArtifacts,
  createEvidenceSummary,
  assertHostScenarioManifest,
  assertRealHostIdentity,
  assertScenarioManifest,
  REQUIRED_SCENARIOS,
} from "./host-parity.mjs";
import { CLOSED_DAY_PARITY_SETUP, stop } from "./host-parity-core.mjs";
import { spawn } from "node:child_process";

test("Given a direct engine identity When parity requires a host Then it rejects the substitute", () => {
  assert.throws(
    () => assertRealHostIdentity({ transport: "direct-engine" }),
    /real host transport/,
  );
});

test("Given an incomplete scenario manifest When parity validates coverage Then omitted behavior cannot pass", () => {
  const manifest = Object.fromEntries(REQUIRED_SCENARIOS.map((scenario) => [scenario, true]));
  delete manifest["concurrency"];

  assert.throws(() => assertScenarioManifest(manifest), /scenario is missing: concurrency/);
});

test("Given real-host artifacts without one required scenario When parity runs Then it rejects the matrix", () => {
  const scenarios = Object.fromEntries(REQUIRED_SCENARIOS.map((scenario) => [scenario, true]));
  const artifacts = {
    worker: { scenarios },
    server: { scenarios: { ...scenarios, "server-ws-resync": false } },
    tauriDefault: { scenarios },
    tauriFeature: { scenarios },
  };

  assert.throws(
    () => assertHostScenarioManifest(artifacts),
    /scenario is missing: server-ws-resync/,
  );
});

test("Given a Tauri feature artifact without default diagnostics When parity validates Then it accepts the explicit paired contract", () => {
  const scenarios = Object.fromEntries(REQUIRED_SCENARIOS.map((scenario) => [scenario, true]));
  assert.doesNotThrow(() => assertHostScenarioManifest({
    worker: { scenarios },
    server: { scenarios },
    tauriDefault: { scenarios: { ...scenarios, "diagnostics-feature": false } },
    tauriFeature: { scenarios: { ...scenarios, "diagnostics-default": false } },
  }));
});

test("Given one returned artifact with a fake identity When parity validates hosts Then the matrix rejects it", () => {
  const artifact = {
    identity: { transport: "worker-postmessage" },
    publicHash: "state",
    restoreHash: "state",
    normalEventHash: "normal",
    closedEventHash: "closed",
    disclosureHash: "disclosure",
    corruptRestoreAtomic: true,
    staleRequestRejected: true,
    stalePrivateRecordsExposed: false,
  };
  assert.throws(() => assertParityArtifacts({
    worker: { ...artifact, identity: { transport: "direct-engine" } },
    server: { ...artifact, identity: { transport: "server-http-ws" } },
    tauriDefault: { ...artifact, identity: { transport: "tauri-invoke-ipc" } },
    tauriFeature: { ...artifact, identity: { transport: "tauri-invoke-ipc" } },
  }), /real host transport/);
});

test("Given one host with a different normalized event stream When parity compares artifacts Then the matrix rejects it", () => {
  const artifact = {
    identity: { transport: "worker-postmessage" },
    publicHash: "state",
    restoreHash: "state",
    normalEventHash: "normal",
    closedEventHash: "closed",
    disclosureHash: "disclosure",
    corruptRestoreAtomic: true,
    staleRequestRejected: true,
    stalePrivateRecordsExposed: false,
  };
  assert.throws(() => assertParityArtifacts({
    worker: artifact,
    server: { ...artifact, identity: { transport: "server-http-ws" }, normalEventHash: "different" },
    tauriDefault: { ...artifact, identity: { transport: "tauri-invoke-ipc" } },
    tauriFeature: { ...artifact, identity: { transport: "tauri-invoke-ipc" } },
  }), /normal event stream/);
});

test("Given the closed-day fixture When the matrix runs Then it targets the deterministic Saturday Q1 disclosure", () => {
  assert.equal(CLOSED_DAY_PARITY_SETUP.start_date, "2030-04-19");
});

test("Given a child that ignores SIGTERM When cleanup stops it Then cleanup escalates and observes exit", async () => {
  const child = spawn(process.execPath, ["-e", "process.on('SIGTERM',()=>{});setInterval(()=>{},1000)"], {
    detached: true,
    stdio: "ignore",
  });
  await new Promise((resolve) => setTimeout(resolve, 100));
  const receipt = await stop(child, 50);
  assert.deepEqual({ exited: receipt.exited, forced: receipt.forced }, { exited: true, forced: true });
});

test("Given complete host cleanup and capabilities When evidence is summarized Then differences and cleanup remain machine-readable", () => {
  const artifact = (diagnostics, pid) => ({
    capabilities: { npcDecisionDiagnostics: diagnostics },
    cleanup: { pid, exited: true, forced: false, exitCode: 0, signalCode: null },
  });
  const tauriArtifact = (diagnostics, pid) => ({
    capabilities: { npcDecisionDiagnostics: diagnostics },
    cleanup: {
      app: { pid, exited: true, forced: false, exitCode: 0, signalCode: null },
      weston: { pid: pid + 10, exited: true, forced: false, exitCode: 0, signalCode: null },
    },
  });
  const summary = createEvidenceSummary({
    worker: artifact(false, 1),
    server: artifact(false, 2),
    tauriDefault: tauriArtifact(false, 3),
    tauriFeature: tauriArtifact(true, 4),
  });
  assert.deepEqual(summary.capabilityDifferences.npcDecisionDiagnostics, {
    worker: false,
    server: false,
    tauriDefault: false,
    tauriFeature: true,
  });
  assert.equal(summary.cleanup.every((receipt) => receipt.exited), true);
});
