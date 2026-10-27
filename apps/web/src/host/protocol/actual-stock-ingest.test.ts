import assert from "node:assert/strict";
import test from "node:test";
import { prepareCatalogResolver } from "../catalog-resolver.ts";
import { translateSnapshot, translateEngineUpdate, translateRuntimeDelta } from "./stock-identity.ts";
import { parseSnapshotRaw, parseEngineUpdate } from "./parse.ts";
import { frame, snapshot } from "../protocol-test-fixtures.ts";

const identity = prepareCatalogResolver([{ id: 0, code: "600101" }], [{ id: 8, code: "C-600101", name: "公司", listed_stock: 0 }]);

test("单股本代 StockId0 在 baseline、日内 facts/timeseries 和 runtime delta 保持600101", () => {
  const raw = parseSnapshotRaw(snapshot(0, 0));
  assert.equal(Object.keys(translateSnapshot(raw, "actual.baseline", identity).markets)[0], "600101");
  const next = translateEngineUpdate(parseEngineUpdate({ TickBatch: { frames: [frame(1, 0, [0])], runtime_snapshot: raw } }), "actual.tick", identity);
  assert.ok("TickBatch" in next);
  assert.equal(Object.keys(next.TickBatch.frames[0].timeseries_payload.markets)[0], "600101");
  const delta = translateRuntimeDelta({ seq_from: 0, seq_to: 1, tick: 1, day: 0, phase: "Continuous", accounts: {}, working_orders: { reset: true, upserts: [{ id: 1, code: 0, side: "Buy", price: "100", remainingQty: 100, venue: "Continuous", frozen: "Cash" }], removed: [] } }, "actual.delta", identity);
  assert.equal(delta.working_orders.upserts[0].code, "600101");
  assert.throws(() => translateSnapshot({ ...raw, markets: { "3": Object.values(raw.markets)[0] } }, "actual.unknown", identity), /StockId/);
});

test("两个宿主 resolver 交错翻译不共享当前目录", () => {
  const other = prepareCatalogResolver([{ id: 0, code: "002156" }], [{ id: 0, code: "C-002156", name: "另一个宿主", listed_stock: 0 }]);
  const raw = parseSnapshotRaw(snapshot(0, 0));
  assert.deepEqual(Object.keys(translateSnapshot(raw, "host.one", identity).markets), ["600101"]);
  assert.deepEqual(Object.keys(translateSnapshot(raw, "host.two", other).markets), ["002156"]);
  assert.deepEqual(Object.keys(translateSnapshot(raw, "host.one.again", identity).markets), ["600101"]);
});

test("单股baseline→下一tick→CivilUpdate refresh与日内历史始终同代600101", async () => {
  const { createProtocolState } = await import("./types.ts");
  const { reduceEngineUpdate } = await import("./reduce.ts");
  const { civilUpdate, tickBatch } = await import("../protocol-test-fixtures.ts");
  const initial = createProtocolState(translateSnapshot(parseSnapshotRaw(snapshot(0, 0)), "actual.baseline", identity), "1", identity);
  const tick = reduceEngineUpdate(initial, "1", tickBatch([frame(1, 0, [0])], snapshot(1, 1)));
  assert.equal(tick.update.kind, "tick-batch");
  assert.deepEqual(Object.keys(tick.state.snapshot.markets), ["600101"]);
  const raw = parseEngineUpdate(civilUpdate());
  assert.ok("CivilUpdate" in raw);
  const wire = { CivilUpdate: { ...raw.CivilUpdate, refresh: { ...raw.CivilUpdate.refresh,
    securities: raw.CivilUpdate.refresh.securities.map(stock => ({ ...stock, code: "600101" })),
  } } };
  const civil = reduceEngineUpdate(tick.state, "1", wire);
  assert.equal(civil.update.kind, "civil-update");
  assert.deepEqual(Object.keys(civil.state.snapshot.markets), ["600101"]);
  assert.equal(civil.state.securities[0].code, "600101");
  assert.deepEqual(Object.keys(civil.state.intraday[0].markets), ["600101"]);
  assert.throws(() => reduceEngineUpdate(civil.state, "0", wire), /代际/);
  assert.equal(civil.state.catalogResolver, identity);
});
