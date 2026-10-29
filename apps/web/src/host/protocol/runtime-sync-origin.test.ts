import assert from "node:assert/strict";
import test from "node:test";
import { defaultCatalogFixture } from "../catalog-test-fixture.ts";
import { snapshot } from "../protocol-test-fixtures.ts";
import { parseProtocolSnapshot } from "./parse.ts";
import { parseRuntimeDelta } from "./runtime-delta.ts";
import type { WireAccountSnap } from "./stock-identity.ts";

function origin() {
  return {
    authority_epoch: 0,
    install_sequence: 0,
    source_tick: 0,
    source_generation: 0,
    day_end_stage: "tick",
    non_tick_revision: 0,
    mirror_phase: "Continuous",
  };
}

function delta(mirror_origin: unknown = origin()) {
  return {
    seq_from: 0,
    seq_to: 1,
    tick: 0,
    day: 0,
    phase: "Continuous",
    mirror_origin,
    accounts: {},
    working_orders: { reset: true, upserts: [], removed: [] },
  };
}

function parseDelta(value: unknown) {
  return parseRuntimeDelta(value, "test.delta", (account) => account as WireAccountSnap);
}

test("snapshot requires the complete mirror origin and accepts zero safe cursors", () => {
  const parsed = parseProtocolSnapshot({ ...snapshot(0, 0), mirror_origin: origin() }, "test.snapshot", defaultCatalogFixture);
  assert.deepEqual(parsed.mirror_origin, origin());
  for (const key of Object.keys(origin())) {
    const missing = origin();
    Reflect.deleteProperty(missing, key);
    assert.throws(() => parseProtocolSnapshot({ ...snapshot(0, 0), mirror_origin: missing }, "test.snapshot", defaultCatalogFixture));
  }
});

test("snapshot rejects unknown, unsafe, future, and invalid mirror origin values", () => {
  const cases = [
    { ...origin(), unexpected: 1 },
    { ...origin(), authority_epoch: Number.MAX_SAFE_INTEGER + 1 },
    { ...origin(), install_sequence: -1 },
    { ...origin(), source_tick: 1 },
    { ...origin(), source_generation: Number.NaN },
    { ...origin(), day_end_stage: "uncommitted" },
    { ...origin(), mirror_phase: "Unknown" },
  ];
  for (const mirror_origin of cases) {
    assert.throws(() => parseProtocolSnapshot({ ...snapshot(0, 0), mirror_origin }, "test.snapshot", defaultCatalogFixture));
  }
});

test("snapshot and runtime delta reject malformed values for every mirror origin u64", () => {
  const fields = ["authority_epoch", "install_sequence", "source_tick", "source_generation", "non_tick_revision"] as const;
  for (const field of fields) {
    for (const invalid of [-1, Number.MAX_SAFE_INTEGER + 1, Number.NaN, Number.POSITIVE_INFINITY]) {
      const mirror_origin = { ...origin(), [field]: invalid };
      assert.throws(() => parseProtocolSnapshot({ ...snapshot(0, 0), mirror_origin }, "test.snapshot", defaultCatalogFixture), `${field}=${invalid} snapshot`);
      assert.throws(() => parseDelta(delta(mirror_origin)), `${field}=${invalid} delta`);
    }
  }
  for (const field of Object.keys(origin())) {
    const missing = origin();
    Reflect.deleteProperty(missing, field);
    assert.throws(() => parseProtocolSnapshot({ ...snapshot(0, 0), mirror_origin: missing }, "test.snapshot", defaultCatalogFixture), `missing ${field} snapshot`);
    assert.throws(() => parseDelta(delta(missing)), `missing ${field} delta`);
  }
});

test("runtime delta requires exact mirror origin fields, valid enums, and source tick not after runtime tick", () => {
  const cases = [
    { ...origin(), unexpected: 1 },
    { ...origin(), day_end_stage: "uncommitted" },
    { ...origin(), mirror_phase: "Unknown" },
    { ...origin(), source_tick: 1 },
  ];
  for (const mirror_origin of cases) assert.throws(() => parseDelta(delta(mirror_origin)));
  assert.doesNotThrow(() => parseDelta(delta()));
});
