import assert from "node:assert/strict";
import test from "node:test";
import { currentSaveFixture } from "../save/current-save-fixture.ts";
import { parseSaveSlot } from "../save/save-schema.ts";
import { drain, fixture } from "./worker-host-w1.test-support.ts";

function archive() { const raw = currentSaveFixture(); return { ...raw, civil_clock: { ...(raw.civil_clock as Record<string, unknown>), settled_through: "2029-12-31" } } as Record<string, unknown>; }
const capabilities = { npcDecisionDiagnostics: false, personalTradeHistory: false, indicatorCapabilities: { intradayAverage: false, macd: false, priceKdj: false, candleKdj: false } };

test("WorkerHost构造前冻结实际日终档X，init仅交X且ready后只发restoreInitial", async () => {
  const f = fixture(); const selected = archive(); const expected = parseSaveSlot(structuredClone(selected));
  try {
    f.launch({ onSpawnFailure() {}, initialArchive: selected }); selected.seed = "999";
    await drain(); const engine = f.workers[0]; const init = engine.sent[0];
    assert.deepEqual(init.initialArchive, expected); assert.equal(Object.hasOwn(init, "setup"), false);
    assert.deepEqual(engine.sent.filter((item) => ["create", "restoreInitial"].includes(String(item.type))), [{ type: "restoreInitial" }]);
    engine.emit({ type: "created", generation: 1, capabilities }); f.ingress(); f.baseline(); await drain();
    assert.equal(f.readOutcome().status, "pending", "未确认真实ingress不得ready");
    f.ack(f.workers[1], 1); await drain(); assert.equal(f.readOutcome().status, "fulfilled");
    assert.equal(f.workers.length, 2);
  } finally { await f.cleanup(); }
});
for (const invalid of ["schema", "intraday"] as const) {
  test(`初始档${invalid}失败在构造Worker/Channel之前显式拒绝`, async () => {
    const f = fixture();
    try {
      f.launch({ onSpawnFailure() {}, initialArchive: invalid === "schema" ? { selected: "bad" } : currentSaveFixture() });
      await drain(); assert.equal(f.readOutcome().status, "rejected");
      assert.equal(f.workers.length, 0); assert.equal(f.channels.length, 0);
    } finally { await f.cleanup(); }
  });
}
test("ready重复时主线程不得再发送首建命令，故障显式关闭整组", async () => {
  const f = fixture();
  try {
    f.launch({ onSpawnFailure() {}, initialArchive: archive() }); await drain();
    const engine = f.workers[0]; engine.emit({ type: "ready" }); await drain();
    assert.equal(engine.sent.filter((item) => ["create", "restoreInitial"].includes(String(item.type))).length, 1);
    assert.equal(engine.terminations, 1); assert.equal(f.readOutcome().status, "rejected");
  } finally { await f.cleanup(); }
});
