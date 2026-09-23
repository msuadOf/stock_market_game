import assert from "node:assert/strict";
import test from "node:test";
import { CompanyQueryCoordinator } from "./company-query-coordinator.ts";
import { frame, civilUpdate } from "./protocol-test-fixtures.ts";
import { parseNormalizedEngineUpdate } from "./protocol/index.ts";

const host = {
  capabilities: { deliveryModes: [], targetUiHz: 60, sharedMemory: false, reconnect: false, publicCompanyReports: false, npcDecisionDiagnostics: false },
};

test("Given a normalized tick frame, when public metadata advances, then company coverage follows its protocol cursor", () => {
  const actions: unknown[] = [];
  const coordinator = new CompanyQueryCoordinator(host, (action) => actions.push(action));
  coordinator.installBaseline({ civilDate: "2030-01-01", revision: "1", seq: 0 });
  const normalized = parseNormalizedEngineUpdate({ TickBatch: { frames: [frame(1, 0, ["600000"])], runtime_snapshot: null } });
  if (normalized.kind !== "tick-batch") throw new Error("test fixture invalid");
  coordinator.acceptFrame(normalized.frames[0]!, { civilDate: "2030-01-01", revision: "2" });
  assert.match(JSON.stringify(actions), /advanceCompanyEventCoverage/);
  assert.match(JSON.stringify(actions), /"revision":"2"/);
});

test("Given a CivilUpdate refresh, when accepted, then the coordinator installs its authoritative public date", () => {
  const actions: unknown[] = [];
  const coordinator = new CompanyQueryCoordinator(host, (action) => actions.push(action));
  coordinator.installBaseline({ civilDate: "2030-01-01", revision: "1", seq: 0 });
  const normalized = parseNormalizedEngineUpdate(civilUpdate());
  if (normalized.kind !== "civil-update") throw new Error("test fixture invalid");
  coordinator.acceptCivil(normalized, { civilDate: "2030-01-03", revision: "3" });
  assert.match(JSON.stringify(actions), /recordCivilDateAdvanced/);
  assert.match(JSON.stringify(actions), /2030-01-03/);
});
