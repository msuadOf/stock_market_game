import assert from "node:assert/strict";
import test from "node:test";
import { registerHooks } from "node:module";
import { fixture, drain } from "./worker-host-w1.test-support.ts";
import { receiveRestoreChunks } from "./restore-chunk-port.ts";
import { CHUNKED_RESTORE_EXPORTS } from "./wasm-restore-package-capability.ts";
import { WasmDocumentMemoryOwner } from "./wasm-document-memory-owner.ts";
import { WasmMemoryStorage } from "./wasm-memory-storage.ts";
import { fixture as storageFixture, MiB } from "./wasm-memory-storage.test-support.ts";
import { createValidatedArchiveLease, inspectValidatedArchiveLease } from "../save/validated-archive-lease.ts";
import { captureGzipArchiveSource } from "../save/archive-source.ts";
import { gzipSaveCodec } from "../save/save-repository.ts";
import { currentSaveFixture, withSettledDay } from "../save/current-save-fixture.ts";

async function setupLease() { const text = JSON.stringify(withSettledDay(currentSaveFixture(), "2030-01-01", "2030-01-02")); return { text, lease: await createValidatedArchiveLease(captureGzipArchiveSource(`gzip:${await gzipSaveCodec.encode(text)}`)) }; }
const exports = ["restore_json", "facade_slot_init_for_restore_json", "restore_prepared", "clear_prepared_restore", "wasm_tombstone_init_primary", "wasm_tombstone_bind_ingress", "set_step_runtime_override", "libos_host_fault_layout", "shutdown_session", ...CHUNKED_RESTORE_EXPORTS];
const capability = { kind: "verified-package" as const, wasmSha256: "a".repeat(64), glueSha256: "b".repeat(64), sourceSha256: "c".repeat(64), buildManifestSha256: "d".repeat(64), exports };
function owner() { const storage = storageFixture(); return new WasmDocumentMemoryOwner({ maximumBytes: storage.options.maximumBytes, reclaimBytes: storage.options.reclaimBytes, readCapacity: () => ({ kind: "enforced-capacity", initialPages: 1024, maximumPages: 4096, liveUpperBoundBytes: 256 * MiB }), restoreCapabilities: { forArchive: () => capability, forLease: () => capability }, createStorage: liveUpperBoundBytes => new WasmMemoryStorage({ ...storage.options, liveUpperBoundBytes }) }); }

test("专属端口在 configured 前发送同一租约，并只发 restoreInitial", { timeout: 10000 }, async () => {
  const { lease, text } = await setupLease(); const book = owner(); const admission = book.admitChunked(lease); let accepted!: () => void;
  const received = new Promise<void>(resolve => { accepted = resolve; }); const chunks: Uint8Array[] = [];
  const f = fixture();
  try {
    const creating = f.launch({ onSpawnFailure() {}, initialChunkedLease: lease, memoryAdmissionTicket: admission.ticket });
    const worker = f.workers[0]!; const previous = worker.onPost!;
    worker.onPost = (message, transfer) => {
      if (message.type !== "init") { previous(message, transfer); return; }
      const data = message.initialChunked as { port: MessagePort; generation: number; contentDigest: { byteLength: bigint } };
      void receiveRestoreChunks(data.port, data.generation, Number(data.contentDigest.byteLength), async bytes => { chunks.push(bytes); }).then(() => { accepted(); previous(message, transfer); });
    };
    await Promise.race([received, creating.then(() => { throw new Error("配置前不应完成 host"); }, error => { throw error; })]);
    assert.equal(Buffer.concat(chunks).toString("utf8"), text); assert.equal(inspectValidatedArchiveLease(lease).utf8Bytes, Buffer.byteLength(text));
    await drain(); assert.equal(f.workers[0]!.sent.some(message => message.type === "restoreInitial"), true);
    assert.equal(f.workers[0]!.sent.some(message => message.type === "create"), false);
    f.workers[0]!.error(new Error("仅验证传输，关闭实例")); await drain(); f.clock.advance(2001); await drain();
    await assert.rejects(creating); await assert.rejects(book.finish(admission.ticket));
  } finally { await f.cleanup(); }
});

test("启动取消只在 host ready 前生效，ready 后不抢先终止", { timeout: 10000 }, async () => {
  const { lease } = await setupLease(), book = owner(), ticket = book.admitChunked(lease).ticket, abort = new AbortController();
  const f = fixture();
  try {
    const creating = f.launch({ onSpawnFailure() {}, initialChunkedLease: lease, initialChunkedSignal: abort.signal, memoryAdmissionTicket: ticket });
    const worker = f.workers[0]!, previous = worker.onPost!;
    worker.onPost = (message, transfer) => {
      if (message.type !== "init") { previous(message, transfer); return; }
      const data = message.initialChunked as { port: MessagePort; generation: number; contentDigest: { byteLength: bigint } };
      void receiveRestoreChunks(data.port, data.generation, Number(data.contentDigest.byteLength), async () => {}).then(() => previous(message, transfer));
    };
    for (let count = 0; count < 100 && !worker.sent.some(message => message.type === "restoreInitial"); count++) await drain();
    assert.ok(worker.sent.some(message => message.type === "restoreInitial"));
    worker.emit({ type: "created", generation: 1, capabilities: { npcDecisionDiagnostics: false, personalTradeHistory: false, indicatorCapabilities: { intradayAverage: false, macd: false, priceKdj: false, candleKdj: false } } });
    f.ingress(); f.baseline(); await drain(); f.ack(f.workers[1]!, 1); await drain();
    const host = await creating; abort.abort(new Error("effect 清理")); await drain();
    assert.equal(worker.sent.some(message => message.type === "cancelInitialChunked"), false);
    assert.equal(worker.terminations, 0); assert.ok(host);
  } finally { await f.cleanup(); }
});

test("发送完成但 configured 之前取消仍拒绝启动", { timeout: 10000 }, async () => {
  const { lease } = await setupLease(), book = owner(), ticket = book.admitChunked(lease).ticket, abort = new AbortController();
  const f = fixture(); let received!: () => void; const done = new Promise<void>(resolve => { received = resolve; });
  try {
    const creating = f.launch({ onSpawnFailure() {}, initialChunkedLease: lease, initialChunkedSignal: abort.signal, memoryAdmissionTicket: ticket });
    const failed = assert.rejects(creating, /取消/); const worker = f.workers[0]!, previous = worker.onPost!;
    worker.onPost = (message, transfer) => {
      if (message.type === "init") {
        const data = message.initialChunked as { port: MessagePort; generation: number; contentDigest: { byteLength: bigint } };
        void receiveRestoreChunks(data.port, data.generation, Number(data.contentDigest.byteLength), async () => {}).then(received); return;
      }
      previous(message, transfer);
    };
    await done; abort.abort(new Error("用户取消")); await failed; await drain();
    assert.equal(worker.sent.some(message => message.type === "create" || message.type === "restoreInitial"), false);
    assert.equal(worker.sent.some(message => message.type === "cancelInitialChunked"), true);
  } finally { f.clock.advance(2001); await f.cleanup(); }
});

test("源阻塞且 Worker 声称未分配时不得发布成功 release", { timeout: 10000 }, async () => {
  const { lease } = await setupLease(), book = owner(), ticket = book.admitChunked(lease).ticket, abort = new AbortController();
  let sourceStarted!: () => void; const started = new Promise<void>(resolve => { sourceStarted = resolve; });
  const key = Symbol.for("chunk-host-stalled-source"); Object.defineProperty(globalThis, key, { configurable: true, value: sourceStarted });
  const hooks = registerHooks({
    resolve(specifier, context, next) {
      if (specifier === "../save/validated-archive-replay.ts" && context.parentURL?.includes("worker-host.ts?stalled-source")) return { url: "virtual:chunk-host-stalled-source", shortCircuit: true };
      return next(specifier, context);
    },
    load(url, context, next) {
      if (url === "virtual:chunk-host-stalled-source") return { format: "module", shortCircuit: true, source: `export function replayValidatedArchive(){return {[Symbol.asyncIterator](){return {next(){globalThis[Symbol.for("chunk-host-stalled-source")]();return new Promise(()=>{})},return(){return new Promise(()=>{})}}}}}` };
      return next(url, context);
    },
  });
  let f: ReturnType<typeof fixture> | undefined;
  try {
    const moduleUrl: string = "./worker-host.ts?stalled-source";
    const { createWorkerHost } = await import(moduleUrl);
    f = fixture({ hostFactory: createWorkerHost });
    const creating = f.launch({ onSpawnFailure() {}, initialChunkedLease: lease, initialChunkedSignal: abort.signal, memoryAdmissionTicket: ticket });
    const failedHost = assert.rejects(creating); const worker = f.workers[0]!, previous = worker.onPost!;
    worker.onPost = (message, transfer) => {
      if (message.type === "init") return;
      if (message.type === "cancelInitialChunked") {
        worker.emit({ type: "failure", generation: 0, code: "CANCELLED", where: "fixture", message: "取消", engineQuiescent: "initialization-failed", memoryAllocation: "not-allocated" });
        worker.emit({ type: "engineLifecycle", instanceId: worker.instanceId, state: "unallocated" }); return;
      }
      previous(message, transfer);
    };
    await started; abort.abort(new Error("取消")); await failedHost; await drain();
    f.clock.advance(2001);
    await assert.rejects(book.finish(ticket), /清理超时|状态未知/);
  } finally { if (f !== undefined) { f.clock.advance(2001); await f.cleanup(); } hooks.deregister(); Reflect.deleteProperty(globalThis, key); }
});

test("failurePending 立即取消在途分块，不等待 terminal", { timeout: 10000 }, async () => {
  const { lease } = await setupLease(), book = owner(), ticket = book.admitChunked(lease).ticket;
  const f = fixture(); let began!: () => void, release!: () => void;
  const started = new Promise<void>(resolve => { began = resolve; }); const gate = new Promise<void>(resolve => { release = resolve; });
  try {
    const creating = f.launch({ onSpawnFailure() {}, initialChunkedLease: lease, memoryAdmissionTicket: ticket }); const settled = creating.catch(() => {});
    const worker = f.workers[0]!, previous = worker.onPost!;
    worker.onPost = (message, transfer) => {
      if (message.type !== "init") { previous(message, transfer); return; }
      const data = message.initialChunked as { port: MessagePort; generation: number; contentDigest: { byteLength: bigint } };
      void receiveRestoreChunks(data.port, data.generation, Number(data.contentDigest.byteLength), async () => { began(); await gate; }).catch(() => {});
    };
    await started;
    f.controllers[0]!.emit({ type: "failurePending", instanceId: f.controllers[0]!.sent[0]!.instanceId, failure: { code: "FIXTURE_FATAL", where: "fixture", message: "需要终止" } });
    await drain(); assert.ok(worker.sent.some(message => message.type === "cancelInitialChunked"));
    release(); f.clock.advance(2001); await drain(); await settled;
  } finally { release(); f.clock.advance(2001); await f.cleanup(); }
});

test("换租约或启动前取消均在 Worker 前拒绝并登记未分配", { timeout: 10000 }, async () => {
  const first = await setupLease(), second = await setupLease();
  for (const kind of ["wrong-lease", "cancelled"] as const) {
    const book = owner(), ticket = book.admitChunked(first.lease).ticket, abort = new AbortController(), f = fixture();
    try {
      if (kind === "cancelled") abort.abort(new Error("启动前取消"));
      const creating = f.launch({ onSpawnFailure() {}, initialChunkedLease: kind === "wrong-lease" ? second.lease : first.lease, initialChunkedSignal: abort.signal, memoryAdmissionTicket: ticket });
      await assert.rejects(creating, kind === "wrong-lease" ? /不一致/ : /取消/);
      assert.equal(f.workers.length, 0); assert.equal(f.channels.length, 0);
      await book.finish(ticket);
    } finally { await f.cleanup(); }
  }
});

test("Worker 提前 ready 也不能在源摘要完成前发送 restoreInitial", { timeout: 10000 }, async () => {
  const { lease } = await setupLease(), book = owner(), ticket = book.admitChunked(lease).ticket, f = fixture();
  let release!: () => void, started!: () => void; const gate = new Promise<void>(resolve => { release = resolve; }), entering = new Promise<void>(resolve => { started = resolve; });
  try {
    const creating = f.launch({ onSpawnFailure() {}, initialChunkedLease: lease, memoryAdmissionTicket: ticket });
    const failed = creating.catch(() => {}), worker = f.workers[0]!, previous = worker.onPost!;
    worker.onPost = (message, transfer) => {
      if (message.type !== "init") { previous(message, transfer); return; }
      const data = message.initialChunked as { port: MessagePort; generation: number; contentDigest: { byteLength: bigint } };
      void receiveRestoreChunks(data.port, data.generation, Number(data.contentDigest.byteLength), async () => { started(); await gate; }).catch(() => {});
      previous(message, transfer);
    };
    await entering; await drain(); assert.equal(f.configured, 1);
    assert.equal(worker.sent.some(message => message.type === "restoreInitial"), false);
    release(); for (let count = 0; count < 100 && !worker.sent.some(message => message.type === "restoreInitial"); count++) await drain();
    assert.ok(worker.sent.some(message => message.type === "restoreInitial"));
    worker.error(new Error("测试收尾")); await drain(); f.clock.advance(2001); await drain(); await failed;
  } finally { release(); f.clock.advance(2001); await f.cleanup(); }
});

test("分块恢复失败保留真实错误而不是误报通信协议", { timeout: 10000 }, async () => {
  const { lease } = await setupLease(), book = owner(), ticket = book.admitChunked(lease).ticket, f = fixture();
  try {
    const creating = f.launch({ onSpawnFailure() {}, initialChunkedLease: lease, memoryAdmissionTicket: ticket });
    const worker = f.workers[0]!, previous = worker.onPost!;
    worker.onPost = (message, transfer) => {
      if (message.type !== "init") { previous(message, transfer); return; }
      const data = message.initialChunked as { port: MessagePort; generation: number; contentDigest: { byteLength: bigint } };
      void receiveRestoreChunks(data.port, data.generation, Number(data.contentDigest.byteLength), async () => {}).then(() => previous(message, transfer));
    };
    for (let count = 0; count < 100 && !worker.sent.some(message => message.type === "restoreInitial"); count++) await drain();
    assert.ok(worker.sent.some(message => message.type === "restoreInitial"));
    worker.emit({ type: "failure", generation: 0, code: "RESTORE_FIXTURE", where: "restore", message: "原始恢复失败", engineQuiescent: "initial-session-failed" });
    await assert.rejects(creating, /原始恢复失败/);
  } finally { f.clock.advance(2001); await f.cleanup(); }
});
