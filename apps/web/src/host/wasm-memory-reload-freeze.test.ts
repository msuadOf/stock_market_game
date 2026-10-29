import assert from "node:assert/strict";
import test from "node:test";
import { fixture, snapshot, SNAPSHOT_KEY, MiB } from "./wasm-memory-storage.test-support.ts";
import { WasmMemoryStorageError } from "./wasm-memory-storage.ts";
import { wasmMemoryLedgerKey } from "./wasm-memory-schema.ts";

function handover() {
  return { kind: "frozen", code: "MEMORY_RESIDUAL_RELOAD_HANDOVER", residualBytes: 512 * MiB, carriedResidualBytes: 512 * MiB,
    thresholdEffective: 512 * MiB, reloadCount: 1, pendingIntent: null, last_committed_tick: 0, day: 0, handoverNonce: "ab".repeat(16) };
}
function prepared() {
  const f = fixture(); f.existing(snapshot()); const owner = f.owner(); const opened = owner.open();
  owner.writeSnapshot({ carryBase: 0, sessionResident: 512 * MiB, liveUpperBound: 0, reloadCount: 0 });
  const anchor = { carryBase: 0, sessionResident: 512 * MiB, liveUpperBound: 0, reloadCount: 1 };
  return { ...f, owner, anchor, opened };
}
test("达到阈值按同nonce先ledger后单键carrier，成功只返回可reload事实", () => {
  const f = prepared(); const order: string[] = [];
  f.ledger.onCall((call) => { if (call.operation === "set") order.push("ledger"); });
  f.session.onCall((call) => { if (call.operation === "set") order.push("session"); });
  const result = f.owner.freezeReloadHandover(f.anchor, handover());
  assert.equal(result.kind, "ready-for-reload"); assert.equal(result.degraded, false); assert.deepEqual(order, ["ledger", "session"]);
  const key = wasmMemoryLedgerKey(f.opened.chainId, f.owner.forkId); const entry = JSON.parse(f.ledger.data.get(key)!).at(-1);
  assert.equal(entry.handoverNonce, handover().handoverNonce); assert.equal(entry.outcome, "reload");
  assert.equal(entry.carriedResidualBytes, 512 * MiB); assert.deepEqual(JSON.parse(f.session.data.get(SNAPSHOT_KEY)!).handover, handover());
});
test("ledger两败且session成功仅返回degraded诊断，不能冒黄带已展示", () => {
  const f = prepared(); f.ledger.onCall((call) => { if (call.operation === "set") throw new Error("ledger quota"); });
  const result = f.owner.freezeReloadHandover(f.anchor, handover()); assert.equal(result.degraded, true);
  assert.ok(result.diagnostics.some((entry) => /仅单通道/.test(entry.message) && entry.cause !== undefined));
  assert.deepEqual(JSON.parse(f.session.data.get(SNAPSHOT_KEY)!).handover, handover());
});
test("session两败即使数字ledger备份成功也不得返回ready或清冻结目标", () => {
  const f = prepared(); f.session.onCall((call) => { if (call.operation === "set") throw new Error("session quota"); });
  assert.throws(() => f.owner.freezeReloadHandover(f.anchor, handover()), (error) => error instanceof WasmMemoryStorageError && error.code === "MEMORY_HANDOVER_WRITE_FAILED" && error.context.context === "freeze");
  f.session.onCall(null); f.owner.writeSnapshot(f.anchor);
  assert.deepEqual(JSON.parse(f.session.data.get(SNAPSHOT_KEY)!).handover, handover());
});
test("两通道均败保现gate，不reload不清账", () => {
  const f = prepared(); const before = [...f.session.data];
  f.session.onCall((call) => { if (call.operation === "set") throw new Error("session quota"); });
  f.ledger.onCall((call) => { if (call.operation === "set") throw new Error("ledger quota"); });
  assert.throws(() => f.owner.freezeReloadHandover(f.anchor, handover()), (error) => error instanceof WasmMemoryStorageError && error.context.channel === "both");
  assert.deepEqual([...f.session.data], before); assert.throws(() => f.owner.assertAdmissionWritable(), /禁止开局/);
});
test("同事件失败重试保nonce/seed/计数，不按重试次数重复历史carry", () => {
  const f = prepared(); f.session.onCall((call) => { if (call.operation === "set") throw new Error("session quota"); });
  assert.throws(() => f.owner.freezeReloadHandover(f.anchor, handover()));
  f.session.onCall(null); assert.equal(f.owner.retryReloadHandover().kind, "ready-for-reload");
  const key = wasmMemoryLedgerKey(f.opened.chainId, f.owner.forkId); const rows = JSON.parse(f.ledger.data.get(key)!);
  assert.ok(rows.every((row: { highWater: { carry: number }; reloadCount: number }) => row.highWater.carry === 512 * MiB && row.reloadCount === 1));
  assert.deepEqual(JSON.parse(f.session.data.get(SNAPSHOT_KEY)!).handover, handover());
});
test("既有pending不能被不同事件覆盖，同事件成功后也不能重新抽nonce", () => {
  const f = prepared(); f.owner.freezeReloadHandover(f.anchor, handover()); const before = [...f.session.data];
  assert.throws(() => f.owner.freezeReloadHandover(f.anchor, { ...handover(), handoverNonce: "cd".repeat(16) }), /pending|待确认|事件/);
  assert.deepEqual([...f.session.data], before);
});
test("未达阈值/活动live/账面payload矛盾必须在写入前拒绝", () => {
  for (const patch of [{ residualBytes: 511 * MiB }, { carriedResidualBytes: 768 * MiB }, { thresholdEffective: 256 * MiB }, { reloadCount: 2 }]) {
    const f = prepared(); const before = [f.session.calls.length, f.ledger.calls.length];
    assert.throws(() => f.owner.freezeReloadHandover(f.anchor, { ...handover(), ...patch }));
    assert.equal(f.session.calls.slice(before[0]).some((call) => call.operation === "set"), false);
    assert.equal(f.ledger.calls.slice(before[1]).some((call) => call.operation === "set"), false);
  }
  const f = prepared(); assert.throws(() => f.owner.freezeReloadHandover({ ...f.anchor, liveUpperBound: 1 }, handover()), /live/);
});

test("本人fork被篡改或删除属于CORRUPT，不降级ready也不写主carrier", () => {
  for (const deleted of [false, true]) {
    const f = prepared(); f.owner.append("f5", 256 * MiB, 0);
    const key = wasmMemoryLedgerKey(f.opened.chainId, f.owner.forkId);
    if (deleted) f.ledger.data.delete(key); else f.ledger.data.set(key, "[]");
    const before = [...f.session.data];
    assert.throws(() => f.owner.freezeReloadHandover(f.anchor, handover()), (error) => error instanceof WasmMemoryStorageError && error.code === "MEMORY_HANDOVER_CORRUPT");
    assert.deepEqual([...f.session.data], before);
  }
});
test("ledger读失败不是实际写两败，不能以degraded绕过", () => {
  const f = prepared();
  f.ledger.onCall((call) => { if (call.operation === "get") throw new Error("ledger读取失败"); });
  const before = [...f.session.data]; assert.throws(() => f.owner.freezeReloadHandover(f.anchor, handover()), (error) => error instanceof WasmMemoryStorageError && error.context.stage === "prepare");
  assert.deepEqual([...f.session.data], before);
});
test("freeze不能在当前drop账前新增resident，或在活动live锚上伪造drop0", () => {
  const f = prepared();
  assert.throws(() => f.owner.freezeReloadHandover({ ...f.anchor, sessionResident: 768 * MiB }, { ...handover(), residualBytes: 768 * MiB, carriedResidualBytes: 768 * MiB }), /drop|resident|账/);
  f.owner.writeAdmissionSnapshot({ ...f.anchor, liveUpperBound: 256 * MiB });
  assert.throws(() => f.owner.freezeReloadHandover(f.anchor, handover()), /drop|live|账/);
});
test("未知可信构造上界不准冻结，耗尽drop不准以handover代替终局", () => {
  const f = fixture({ liveUpperBoundBytes: null }); f.existing(snapshot()); const owner = f.owner(); owner.open();
  const anchor = { carryBase: 0, sessionResident: 512 * MiB, liveUpperBound: 0, reloadCount: 1 }; owner.writeSnapshot(anchor);
  assert.throws(() => owner.freezeReloadHandover(anchor, handover()), (error) => error instanceof WasmMemoryStorageError && error.code === "MEMORY_MEASUREMENT_NOT_READY");
  const g = fixture(); g.existing(snapshot(1792 * MiB)); const exhausted = g.owner(); exhausted.open();
  assert.throws(() => exhausted.freezeReloadHandover({ carryBase: 1792 * MiB, sessionResident: 0, liveUpperBound: 0, reloadCount: 1 }, handover()), (error) => error instanceof WasmMemoryStorageError && error.code === "MEMORY_RESIDUAL_EXHAUSTED");
});
test("未捕获或已成功事件没有失败重试入口，不能重放一次成功reload", () => {
  const f = prepared(); assert.throws(() => f.owner.retryReloadHandover(), /重试|事件/);
  f.owner.freezeReloadHandover(f.anchor, handover()); assert.throws(() => f.owner.retryReloadHandover(), /重试|事件/);
});

test("消费为新局成功后旧失败freeze重试入口失效，不把新live写回0", () => {
  const f = prepared(); f.session.onCall((call) => { if (call.operation === "set") throw new Error("session quota"); });
  assert.throws(() => f.owner.freezeReloadHandover(f.anchor, handover())); f.session.onCall(null);
  f.owner.writeAdmissionSnapshot({ ...f.anchor, liveUpperBound: 256 * MiB }); const before = [...f.session.data];
  assert.throws(() => f.owner.retryReloadHandover(), /重试|事件/); assert.deepEqual([...f.session.data], before);
});
test("外部nonce无效在任何ledger读取或写入前拒绝", () => {
  for (const nonce of ["a", { toString: () => "ab".repeat(16) }]) {
    const f = prepared(); const before = f.ledger.calls.length;
    assert.throws(() => f.owner.append("reload", 512 * MiB, 1, nonce as string), /handoverNonce/);
    assert.equal(f.ledger.calls.length, before);
  }
});

test("未达阈值是无需reload，不冒充MEMORY_BUDGET_EXCEEDED", () => {
  const f = fixture(); f.existing(snapshot()); const owner = f.owner(); owner.open();
  const anchor = { carryBase: 0, sessionResident: 256 * MiB, liveUpperBound: 0, reloadCount: 1 }; owner.writeSnapshot({ ...anchor, reloadCount: 0 });
  assert.throws(() => owner.freezeReloadHandover(anchor, handover()), (error) => error instanceof WasmMemoryStorageError && error.code === "MEMORY_RELOAD_NOT_REQUIRED");
});
test("freeze锚访问器不执行，也不能在校验中重入修改owner账", () => {
  const f = prepared(); let reads = 0;
  Object.defineProperty(f.anchor, "sessionResident", { enumerable: true, get() { reads++; return 512 * MiB; } });
  const before = [...f.session.data]; assert.throws(() => f.owner.freezeReloadHandover(f.anchor, handover()), /访问器|data/);
  assert.equal(reads, 0); assert.deepEqual([...f.session.data], before);
});

test("snapshot备份遇到已确认fork损坏仍保CORRUPT，不被双写失败包装降级", () => {
  const f = prepared(); f.owner.append("f5", 256 * MiB, 0);
  const key = wasmMemoryLedgerKey(f.opened.chainId, f.owner.forkId); f.ledger.data.set(key, "[]");
  f.session.onCall((call) => { if (call.operation === "set") throw new Error("session quota"); });
  assert.throws(() => f.owner.writeSnapshot(f.anchor), (error) => error instanceof WasmMemoryStorageError && error.code === "MEMORY_HANDOVER_CORRUPT");
  assert.throws(() => f.owner.assertAdmissionWritable(), (error) => error instanceof WasmMemoryStorageError && error.code === "MEMORY_HANDOVER_CORRUPT");
});

test("ledger冻结已成功后session失败不因冗余backup失败误封双败门", () => {
  const f = prepared(); let writes = 0;
  f.ledger.onCall((call) => { if (call.operation === "set" && ++writes > 1) throw new Error("冗余backup失败"); });
  f.session.onCall((call) => { if (call.operation === "set") throw new Error("session两败"); });
  assert.throws(() => f.owner.freezeReloadHandover(f.anchor, handover()), (error) => error instanceof WasmMemoryStorageError && error.context.channel === "session" && error.context.context === "freeze");
  assert.doesNotThrow(() => f.owner.assertAdmissionWritable());
  assert.equal(writes, 1, "本轮reload ledger已可靠写成，不再冗余写backup");
  f.session.onCall(null); f.ledger.onCall(null);
  assert.equal(f.owner.retryReloadHandover().kind, "ready-for-reload");
});

test("读/getter重抛旧写败对象也不是本轮setter两败，不能误降级", () => {
  for (const phase of ["read", "setter-getter"]) {
    const f = prepared(); let retained: unknown;
    f.ledger.onCall((call) => { if (call.operation === "set") throw new Error("早先写败"); });
    try { f.owner.append("f5", 256 * MiB, 0); } catch (error) { retained = (error as Error).cause; }
    assert.ok(retained instanceof AggregateError); f.ledger.onCall(null);
    if (phase === "read") f.ledger.onCall((call) => { if (call.operation === "get") throw retained; });
    else Object.defineProperty(f.ledger.port, "setItem", { configurable: true, get() { throw retained; } });
    const before = [...f.session.data];
    assert.throws(() => f.owner.freezeReloadHandover(f.anchor, handover()), (error) => error instanceof WasmMemoryStorageError && error.context.stage === "prepare");
    assert.deepEqual([...f.session.data], before);
  }
});

test("setter自身call访问器不能让实际零次写入冒充两败降级", () => {
  const f = prepared(); const setter = f.ledger.port.setItem; let calls = 0; let callReads = 0;
  const wrapped: typeof setter = function (this: typeof f.ledger.port, key, value) { calls++; return Reflect.apply(setter, this, [key, value]); };
  Object.defineProperty(wrapped, "call", { get() { callReads++; throw new Error("call访问器失败，未实际调用setter"); } });
  Object.defineProperty(f.ledger.port, "setItem", { configurable: true, value: wrapped });
  const result = f.owner.freezeReloadHandover(f.anchor, handover());
  assert.equal(result.degraded, false); assert.equal(calls, 1); assert.equal(callReads, 0);
});
