import assert from "node:assert/strict";
import test from "node:test";
import { persistClientCivilDayEnd, type ClientCivilDayEndPorts } from "./client-day-end-persistence.ts";
import { DayEndPersistence } from "./day-end-persistence.ts";
import { commandDayEndArchiveFixture } from "../app/command-host-test-fixture.ts";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((accept, fail) => { resolve = accept; reject = fail; });
  return { promise, resolve, reject };
}

function fixture() {
  const original = commandDayEndArchiveFixture();
  const archive = { ...original, snapshot: { ...original.snapshot, seq: 2 } };
  const calls: string[] = [];
  let hostCurrent = true;
  let generation = "current";
  const persistence = new DayEndPersistence();
  persistence.install("current");
  const ports: ClientCivilDayEndPorts = {
    hostKind: "remote", generation: "current", persistence,
    update: { seq_to: archive.snapshot.seq, boundary: {
      settled_date: "2029-12-31", settled_phase: "ClosedDay", next_date: "2030-01-01", next_status: "Trading",
    } },
    save: async reference => {
      calls.push("save");
      assert.deepEqual(reference, { seq: archive.snapshot.seq, settledDate: "2029-12-31" });
      return archive;
    },
    browserQuickSlot: () => { calls.push("indexeddb"); return { label: "浏览器快速槽", write: async () => { calls.push("quick-write"); } }; },
    authorizedFile: null,
    isCurrent: () => hostCurrent && generation === "current",
    refreshPlayerOrders: () => { calls.push("refresh"); },
    exportSubscription: {
      afterSeq: 0, read: reference => ports.save(reference),
      acknowledge: async () => { calls.push("ack"); },
      close: async () => {},
    },
  };
  return { archive, calls, ports, stale(kind: "host" | "generation" = "generation") {
    if (kind === "host") hostCurrent = false;
    else { generation = "next"; persistence.install("next"); }
  } };
}

test("remote CivilUpdate 未授权文件时不提取候选、不访问 IndexedDB，仍刷新挂单", { timeout: 10000 }, async () => {
  const f = fixture();
  f.ports.save = async () => { throw new Error("remote 默认不能拉取完整候选"); };
  f.ports.browserQuickSlot = () => { throw new Error("remote 不能访问 IndexedDB"); };
  assert.equal(await persistClientCivilDayEnd(f.ports), null);
  assert.deepEqual(f.calls, ["refresh"]);
});

test("remote 授权文件后先捕获精确 CivilUpdate 候选，再刷新并只写授权文件", { timeout: 10000 }, async () => {
  const f = fixture(), capture = deferred<unknown>();
  f.ports.save = reference => { f.calls.push("save"); assert.deepEqual(reference, { seq: f.archive.snapshot.seq, settledDate: "2029-12-31" }); return capture.promise; };
  f.ports.authorizedFile = { label: "授权文件", write: async slot => { assert.deepEqual(slot, f.archive); f.calls.push("file-write"); } };
  const pending = persistClientCivilDayEnd(f.ports);
  await Promise.resolve();
  assert.deepEqual(f.calls, ["save"]);
  capture.resolve(f.archive);
  assert.deepEqual(await pending, ["授权文件"]);
  assert.deepEqual(f.calls, ["save", "refresh", "file-write", "ack"]);
});

test("wasm 与当前 tauri CivilUpdate 保留快速槽及授权文件的完整编排", { timeout: 10000 }, async () => {
  for (const kind of ["wasm", "tauri"] as const) {
    const f = fixture(); f.ports.hostKind = kind;
    f.ports.authorizedFile = { label: "授权文件", write: async () => { f.calls.push("file-write"); } };
    assert.deepEqual(await persistClientCivilDayEnd(f.ports), ["浏览器快速槽", "授权文件"]);
    assert.deepEqual(f.calls, ["indexeddb", "save", "refresh", "quick-write", "file-write"]);
  }
});

test("候选请求失败或不属于当前 CivilUpdate 时明确失败，不刷新或写入", { timeout: 10000 }, async () => {
  for (const reason of ["capture", "wrong-day"] as const) {
    const f = fixture(), failure = new Error("候选提取失败");
    f.ports.authorizedFile = { label: "授权文件", write: async () => { f.calls.push("file-write"); } };
    f.ports.save = async () => { f.calls.push("save"); if (reason === "capture") throw failure; return { ...f.archive, snapshot: { ...f.archive.snapshot, seq: f.archive.snapshot.seq + 1 } }; };
    await assert.rejects(persistClientCivilDayEnd(f.ports), error => reason === "capture" ? error === failure : error instanceof Error && /日终候选不属于请求/.test(error.message));
    assert.deepEqual(f.calls, ["save", "ack"]);
  }
});

test("授权文件写入失败保留原因和目标名称，不报告成功", { timeout: 10000 }, async () => {
  const f = fixture(), failure = new Error("磁盘已满");
  f.ports.authorizedFile = { label: "授权文件", write: async () => { throw failure; } };
  await assert.rejects(persistClientCivilDayEnd(f.ports), error => error instanceof AggregateError && error.errors[0] === failure && /授权文件更新失败.*磁盘已满/.test(error.message));
  assert.deepEqual(f.calls, ["save", "refresh", "ack"]);
});

test("捕获期间宿主或 generation 变化后不刷新、不写入、不报告成功", { timeout: 10000 }, async () => {
  for (const kind of ["host", "generation"] as const) {
    const f = fixture(), capture = deferred<unknown>();
    f.ports.save = () => { f.calls.push("save"); return capture.promise; };
    f.ports.authorizedFile = { label: "授权文件", write: async () => { f.calls.push("file-write"); } };
    const pending = persistClientCivilDayEnd(f.ports);
    await Promise.resolve();
    f.stale(kind); capture.resolve(f.archive);
    assert.equal(await pending, null);
    assert.deepEqual(f.calls, ["save", "ack"]);
  }
});

test("授权文件写入期间 generation 变化会取消旧目标且不报告成功", { timeout: 10000 }, async () => {
  const f = fixture(), entered = deferred<void>(), commit = deferred<void>();
  f.ports.authorizedFile = { label: "授权文件", write: async (_slot, isCurrent) => {
    entered.resolve(); await commit.promise;
    if (!isCurrent()) return false;
    f.calls.push("file-write");
  } };
  const pending = persistClientCivilDayEnd(f.ports);
  await entered.promise; f.stale(); commit.resolve();
  assert.equal(await pending, null);
  assert.deepEqual(f.calls, ["save", "refresh", "ack"]);
});

test("已过期的 remote CivilUpdate 不触发候选、存储或挂单刷新", { timeout: 10000 }, async () => {
  const f = fixture(); f.stale();
  assert.equal(await persistClientCivilDayEnd(f.ports), null);
  assert.deepEqual(f.calls, []);
});

test("remote 文件未订阅时明确拒绝，文件或 ACK 同时失败时保留两个错误", { timeout: 10000 }, async () => {
  const f = fixture();
  f.ports.authorizedFile = { label: "授权文件", write: async () => { throw new Error("文件失败"); } };
  const subscription = f.ports.exportSubscription!;
  f.ports.exportSubscription = undefined;
  await assert.rejects(persistClientCivilDayEnd(f.ports), /未订阅/);
  assert.deepEqual(f.calls, []);
  f.ports.exportSubscription = { ...subscription, acknowledge: async () => { throw new Error("ACK失败"); } };
  await assert.rejects(persistClientCivilDayEnd(f.ports), error => error instanceof AggregateError && /文件失败/.test(String(error.errors[0])) && /ACK失败/.test(String(error.errors[1])));
});

test("文件已写入但 ACK 等待期间 generation 变化，不向新时间线报告保存成功", { timeout: 10000 }, async () => {
  const f = fixture(), entered = deferred<void>(), release = deferred<void>();
  f.ports.authorizedFile = { label: "授权文件", write: async () => { f.calls.push("file-write"); } };
  f.ports.exportSubscription = { ...f.ports.exportSubscription!, acknowledge: async () => { entered.resolve(); await release.promise; } };
  const pending = persistClientCivilDayEnd(f.ports); await entered.promise;
  f.stale(); release.resolve();
  assert.equal(await pending, null);
});

test("本地更换授权文件目标仍保存浏览器快速槽，仅报告真实写入的快速槽", { timeout: 10000 }, async () => {
  for (const kind of ["wasm", "tauri"] as const) {
    const f = fixture(); f.ports.hostKind = kind;
    f.ports.isFileTargetCurrent = () => false;
    f.ports.authorizedFile = { label: "授权文件", write: async () => { throw new Error("不得写入旧文件"); } };
    assert.deepEqual(await persistClientCivilDayEnd(f.ports), ["浏览器快速槽"]);
    assert.deepEqual(f.calls, ["indexeddb", "save", "refresh", "quick-write"]);
  }
});

test("remote 前一天写入或 ACK 未完成时，下一天不读取完整候选", { timeout: 10000 }, async () => {
  const f = fixture(), entered = deferred<void>(), write = deferred<void>(), ackEntered = deferred<void>(), ack = deferred<void>();
  let reads = 0, acknowledgements = 0;
  f.ports.exportSubscription = {
    afterSeq: 0, read: async () => { reads++; return f.archive; },
    acknowledge: async () => { if (++acknowledgements === 1) { ackEntered.resolve(); await ack.promise; } },
    close: async () => {},
  };
  f.ports.authorizedFile = { label: "授权文件", write: async () => { if (reads === 1) { entered.resolve(); await write.promise; } } };
  const first = persistClientCivilDayEnd(f.ports); await entered.promise;
  const second = persistClientCivilDayEnd(f.ports);
  await Promise.resolve(); assert.equal(reads, 1);
  write.resolve(); await ackEntered.promise;
  assert.equal(reads, 1);
  ack.resolve(); await Promise.all([first, second]);
  assert.equal(reads, 2); assert.equal(acknowledgements, 2);
});

test("订阅确认水位之前的 CivilUpdate backlog 只刷新挂单，不读取或 ACK 非租约候选", { timeout: 10000 }, async () => {
  const f = fixture();
  f.ports.authorizedFile = { label: "授权文件", write: async () => { throw new Error("不应写入订阅前候选"); } };
  f.ports.exportSubscription = { ...f.ports.exportSubscription!, afterSeq: f.archive.snapshot.seq };
  assert.equal(await persistClientCivilDayEnd(f.ports), null);
  assert.deepEqual(f.calls, ["refresh"]);
});
