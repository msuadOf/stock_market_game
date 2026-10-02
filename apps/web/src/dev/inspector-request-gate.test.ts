import assert from "node:assert/strict";
import test from "node:test";
import { InspectorRequestGate } from "./inspector-request-gate.ts";

test("a host change or cleanup invalidates an outstanding inspector response", () => {
  const gate = new InspectorRequestGate();
  const firstHost = {};
  const request = gate.begin(firstHost);

  gate.invalidate();

  assert.equal(gate.isCurrent(request, firstHost), false);
});

test("a newer refresh invalidates an older request on the same host", () => {
  const gate = new InspectorRequestGate();
  const host = {};
  const oldRequest = gate.begin(host);
  const currentRequest = gate.begin(host);

  assert.equal(gate.isCurrent(oldRequest, host), false);
  assert.equal(gate.isCurrent(currentRequest, host), true);
  assert.equal(gate.isCurrent(currentRequest, {}), false);
});

test("loading another timeline on the same host rejects every old inspector completion", () => {
  const gate = new InspectorRequestGate();
  const host = {};
  const oldRequest = gate.begin(host, "4");
  assert.equal(gate.isCurrent(oldRequest, host, "5"), false);
  const currentRequest = gate.begin(host, "5");
  assert.equal(gate.isCurrent(currentRequest, host, "5"), true);
  assert.equal(gate.isCurrent(currentRequest, {}, "5"), false);
  gate.invalidate();
  assert.equal(gate.isCurrent(currentRequest, host, "5"), false);
});
