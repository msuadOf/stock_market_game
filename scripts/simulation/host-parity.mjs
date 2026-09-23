import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";

import { serverScenario } from "./host-parity-server.mjs";
import { NORMAL_DAY_PARITY_SETUP } from "./host-parity-core.mjs";
import { tauriScenario } from "./host-parity-tauri.mjs";
import { workerScenario } from "./host-parity-worker.mjs";
import { assertPublicationFreshness, buildParityInputs } from "./host-parity-build.mjs";
export { assertArtifactHashes, assertPublicationFreshness } from "./host-parity-build.mjs";

const REAL_TRANSPORTS = new Set(["worker-postmessage", "server-http-ws", "tauri-invoke-ipc"]);
export const REQUIRED_SCENARIOS = [
  "identity",
  "civil-day",
  "no-trade-disclosure",
  "sequence-rejection",
  "corrupt-restore",
  "reload",
  "concurrency",
  "stale-privacy",
  "server-ws-resync",
  "diagnostics-default",
  "diagnostics-feature",
];

export function assertScenarioManifest(manifest) {
  for (const scenario of REQUIRED_SCENARIOS) {
    if (manifest[scenario] !== true) throw new Error(`host parity scenario is missing: ${scenario}`);
  }
}

const HOST_SCENARIOS = REQUIRED_SCENARIOS.filter((scenario) => !["server-ws-resync", "diagnostics-default", "diagnostics-feature"].includes(scenario));

export function assertHostScenarioManifest({ worker, server, tauriDefault, tauriFeature }) {
  for (const [name, artifact] of Object.entries({ worker, server, tauriDefault, tauriFeature })) {
    for (const scenario of HOST_SCENARIOS) {
      if (artifact.scenarios?.[scenario] !== true) throw new Error(`${name} scenario is missing: ${scenario}`);
    }
  }
  const matrix = {
    ...Object.fromEntries(HOST_SCENARIOS.map((scenario) => [scenario, true])),
    "server-ws-resync": server.scenarios?.["server-ws-resync"] === true,
    "diagnostics-default": worker.scenarios?.["diagnostics-default"] === true
      && server.scenarios?.["diagnostics-default"] === true
      && tauriDefault.scenarios?.["diagnostics-default"] === true,
    "diagnostics-feature": tauriFeature.scenarios?.["diagnostics-feature"] === true,
  };
  assertScenarioManifest(matrix);
  return matrix;
}

export function assertRealHostIdentity(identity) {
  if (!identity || !REAL_TRANSPORTS.has(identity.transport)) {
    throw new Error("host parity requires a real host transport; direct engine calls and mocks are forbidden");
  }
}

export function assertParityArtifacts({ worker, server, tauriDefault, tauriFeature }) {
  const artifacts = { worker, server, tauriDefault, tauriFeature };
  for (const artifact of Object.values(artifacts)) {
    assertRealHostIdentity(artifact.identity);
    assert.equal(artifact.publicHash, artifact.restoreHash, `${artifact.identity.transport} reload must preserve public state`);
    assert.equal(artifact.corruptRestoreAtomic, true, `${artifact.identity.transport} corrupt restore must be atomic`);
    assert.equal(artifact.staleRequestRejected, true, `${artifact.identity.transport} must reject a stale request`);
    assert.equal(artifact.stalePrivateRecordsExposed, false, `${artifact.identity.transport} stale request must not expose private records`);
  }
  for (const [field, description] of [
    ["publicHash", "public state"],
    ["normalEventHash", "normal event stream"],
    ["closedEventHash", "closed event stream"],
    ["disclosureHash", "public disclosure"],
  ]) {
    const expected = worker[field];
    for (const [name, artifact] of Object.entries(artifacts)) {
      assert.equal(artifact[field], expected, `${name} ${description} must match Worker`);
    }
  }
}

export function createEvidenceSummary({ worker, server, tauriDefault, tauriFeature }) {
  return {
    capabilityDifferences: {
      npcDecisionDiagnostics: {
        worker: worker.capabilities.npcDecisionDiagnostics,
        server: server.capabilities.npcDecisionDiagnostics,
        tauriDefault: tauriDefault.capabilities.npcDecisionDiagnostics,
        tauriFeature: tauriFeature.capabilities.npcDecisionDiagnostics,
      },
    },
    cleanup: [worker.cleanup, server.cleanup, tauriDefault.cleanup.app, tauriDefault.cleanup.weston, tauriFeature.cleanup.app, tauriFeature.cleanup.weston],
  };
}

function parseOutput(argv) {
  if (argv.length !== 2 || argv[0] !== "--output" || !argv[1]) {
    throw new Error("usage: node scripts/simulation/host-parity.mjs --output <new-directory>");
  }
  return resolve(argv[1]);
}

function canonical(value) {
  if (Array.isArray(value)) return value.map(canonical);
  if (value && typeof value === "object") return Object.fromEntries(Object.keys(value).sort().map((key) => [key, canonical(value[key])]));
  return value;
}

function digest(value) {
  return createHash("sha256").update(JSON.stringify(canonical(value))).digest("hex");
}

async function main() {
  const output = parseOutput(process.argv.slice(2));
  await mkdir(output, { recursive: false });
  const provenance = await buildParityInputs(output);
  await assertPublicationFreshness(provenance, output);
  for (const transport of REAL_TRANSPORTS) assertRealHostIdentity({ transport });
  const worker = await workerScenario(output, 32_000 + (process.pid % 1_000), digest, NORMAL_DAY_PARITY_SETUP);
  const server = await serverScenario(output, 33_000 + (process.pid % 1_000), digest);
  const tauriDefault = await tauriScenario(output, 34_000 + (process.pid % 1_000), digest, NORMAL_DAY_PARITY_SETUP, false, "default");
  const tauriFeature = await tauriScenario(output, 35_000 + (process.pid % 1_000), digest, NORMAL_DAY_PARITY_SETUP, true, "feature");
  const tauri = {
    ...tauriFeature,
    scenarios: {
      ...tauriFeature.scenarios,
      "diagnostics-default": tauriDefault.scenarios["diagnostics-default"],
    },
    raw: { default: tauriDefault.raw, feature: tauriFeature.raw },
  };
  const scenarioMatrix = assertHostScenarioManifest({ worker, server, tauriDefault, tauriFeature });
  assertParityArtifacts({ worker, server, tauriDefault, tauriFeature });
  const evidence = createEvidenceSummary({ worker, server, tauriDefault, tauriFeature });
  assert.equal(evidence.cleanup.every((receipt) => receipt.exited), true, "all host processes must exit before evidence publication");
  await assertPublicationFreshness(provenance, output);
  await writeFile(`${output}/cleanup-receipt.json`, `${JSON.stringify(evidence.cleanup, null, 2)}\n`);
  await writeFile(`${output}/provenance.json`, `${JSON.stringify(provenance, null, 2)}\n`);
  await writeFile(`${output}/comparison.json`, `${JSON.stringify({ scenarioMatrix, capabilityDifferences: evidence.capabilityDifferences, provenance, worker, server, tauriDefault, tauriFeature, tauri }, null, 2)}\n`);
  console.log(JSON.stringify({ worker: worker.publicHash, server: server.publicHash, tauri: tauri.publicHash }));
}

if (import.meta.main) await main();
