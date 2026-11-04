import { catalogWorkerFixture } from "./catalog-test-fixture.ts";
import assert from "node:assert/strict";
import { registerHooks } from "node:module";
import test from "node:test";
import { setImmediate } from "node:timers/promises";
import { commandDayEndArchiveFixture } from "../app/command-host-test-fixture.ts";
import { currentSaveFixture } from "../save/current-save-fixture.ts";
import { civilUpdate, frame, snapshot, tickBatch } from "./protocol-test-fixtures.ts";
import { CAPACITY, GEN_BITS, FREE_HEAD, FREE_HEAD_EMPTY, HEADER_WORDS, SLOT_STRIDE, JOIN_STRIDE } from "./facade-slot-protocol.ts";

type Message = Record<string, unknown> & { type: string };
let fixtureId = 0;
const bindingsKey = Symbol.for("stock-game-wasm-owner-tests");

async function withWorker(run: (fixture: Awaited<ReturnType<typeof createFixture>>) => Promise<void>, missingWrites: boolean | "malformed" | "initial-archive" = false) {
  const globals = new Map<PropertyKey, PropertyDescriptor | undefined>();
  const replace = (key: PropertyKey, value: unknown) => {
    globals.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { configurable: true, value });
  };
  const channels: MessageChannel[] = [];
  // 保留真实调度器；在 fixture 失败时也关闭 Node 中会阻止进程退出的 ports。
  replace("MessageChannel", class extends MessageChannel {
    constructor() {
      super();
      channels.push(this);
    }
  });
  let hooks: ReturnType<typeof registerHooks> | undefined;
  try {
    const f = await createFixture(replace, (registered) => { hooks = registered; }, missingWrites);
    await run(f);
  } finally {
    for (const channel of channels) {
      channel.port1.close();
      channel.port2.close();
    }
    hooks?.deregister();
    for (const [key, original] of globals) {
      if (original === undefined) Reflect.deleteProperty(globalThis, key);
      else Object.defineProperty(globalThis, key, original);
    }
  }
}

async function createFixture(
  replace: (key: PropertyKey, value: unknown) => void,
  register: (hooks: ReturnType<typeof registerHooks>) => void,
  missingWrites: boolean | "malformed" | "initial-archive",
) {
  const posted: Message[] = [];
  const globalLifecycle: Message[] = [];
  let initialized: () => void = () => { throw new Error("初始化信号未安装"); };
  const ready = new Promise<void>((resolve) => { initialized = resolve; });
  const calls: string[] = [];
  const timers = new Map<number, () => void>();
  let nextTimer = 0;
  let handle = 7;
  let failing: "create" | "snapshot" | "prepare" | "drop" | "catalog" | "recoverable" | "snapshot-and-drop" | "company-read" | "company-read-typed" | "shutdown" | null = null;
  let failingPost: string | null = null;
  let writeFailure: unknown;
  let listener: ((event: { data: Message }) => void) | undefined;
  const saved = missingWrites === "initial-archive" ? commandDayEndArchiveFixture() : currentSaveFixture();
  const layout = { baseAddr: 0, headerWords: HEADER_WORDS, slotStride: SLOT_STRIDE, joinStride: JOIN_STRIDE, capacity: 4, genBits: 29 };
  let sharedMemory!: WebAssembly.Memory;
  const bindings = {
    initSync: ({ memory }: { memory: WebAssembly.Memory }) => { sharedMemory = memory; return { memory, __wbindgen_thread_destroy() {} }; },
    facade_slot_init_for_setup: () => { const words = new Int32Array(sharedMemory.buffer); Atomics.store(words, CAPACITY, 4); Atomics.store(words, GEN_BITS, 29); Atomics.store(words, FREE_HEAD, FREE_HEAD_EMPTY | 0); },
    clear_prepared_restore: () => {},
    facade_slot_init_for_restore_json: (encoded: string) => {
      assert.deepEqual(JSON.parse(encoded), saved); calls.push("initial-topology");
      const words = new Int32Array(sharedMemory.buffer); Atomics.store(words, CAPACITY, 4); Atomics.store(words, GEN_BITS, 29); Atomics.store(words, FREE_HEAD, FREE_HEAD_EMPTY | 0);
    },
    facade_slot_layout: () => layout,
    ingress_token: (current: number) => current,
    create_session: () => { calls.push(`create:${handle}`); if (failing === "create") throw new Error("create failed"); return handle; },
    civil_date: (current: number) => current === 7 ? "2030-01-02" : "2031-02-03",
    snapshot: (current: number) => { calls.push(`snapshot:${current}`); if (failing === "snapshot" || failing === "snapshot-and-drop") throw new Error("snapshot failed"); if (failing === "recoverable") throw { code: "READ_RETRY", where: "test.read", message: "可恢复读取失败", recoverable: true }; return snapshot(0, 0); },
    stock_catalog_view: () => [{ id: 0, code: "000812" }],
    company_catalog_view: () => { if (failing === "catalog") throw { code: "CATALOG_BASELINE_FAILED", where: "web-wasm.company_catalog_view", message: "公开公司身份目录构造失败", context: { operation: "company_catalog_view", fault: { code: 3019, context: [7] } }, recoverable: false, recoveryActions: ["重新同步"] }; return [{ id: 0, code: "C-000812", name: "公司000812", listed_stock: 0 }]; },
    prepare_public_baseline: (current: number) => { calls.push(`prepare:${current}`); if (failing === "prepare") throw new Error("prepare failed"); },
    shutdown_session: (current: number) => {
      calls.push(`shutdown:${current}`);
      if (failing === "shutdown") throw { handle: current, result: { primary: { stage: "join", worker: "group-0", detail: "真实 primary fixture" }, secondary: [{ stage: "final", worker: undefined, detail: "真实 secondary fixture" }], completion: { owned_workers: 1, joined_workers: 1, worker_results: [{ worker: "group-0", result: { Err: "panic fixture" } }], group_finals: 0, desk_finals: 0 } } };
      return { handle: current, result: { runtime: undefined } };
    },
    drop_session: (current: number) => { calls.push(`drop:${current}`); if (failing === "drop" || failing === "snapshot-and-drop") throw new Error("drop failed"); },
    restore_json: (encoded: string) => { assert.deepEqual(JSON.parse(encoded), saved); calls.push(`restore:${handle}`); return handle; },
    restore_prepared: () => { calls.push(`restore:${handle}`); return handle; },
    host_capabilities: () => ({ npcDecisionDiagnostics: false, indicatorCapabilities: { intradayAverage: false, macd: false, priceKdj: false, candleKdj: false }, personalTradeHistory: false }),
    step: (current: number) => { calls.push(`step:${current}`); return tickBatch([frame(1, 0, [0])], snapshot(1, 1)); },
    tick: () => 1n,
    flush_runtime_sync: (current: number) => { calls.push(`flush:${current}`); return snapshot(0, 0); },
    save: (current: number) => { calls.push(`save:${current}`); return saved; },
    end_civil_day: () => civilUpdate(),
    company_read_query: (current: number, query: unknown) => {
      calls.push(`company-read:${current}:${JSON.stringify(query)}`);
      if (failing === "company-read") throw new Error("本人公司查询的实际拒绝原因");
      if (failing === "company-read-typed") throw { kind: "CorporateState", data: { reason: "实际公司拒绝原因" } };
      return { owner: "0", value: { kind: "OwnerRightsOfferings", data: [] } };
    },
    // 公司写返回值是 synthetic transport fixture，不证明真实 Rust 公司批准或 Correction 生命周期。
    subscribe_rights_offering: (current: number, eventId: string, shares: string) => {
      calls.push(`rights:${current}:${eventId}:${shares}`);
      if (writeFailure !== undefined) throw writeFailure;
      return { event_id: eventId, requested_shares: shares, submitted_on: "2030-01-02" };
    },
    propose_player_company_action: (current: number, proposal: unknown) => {
      calls.push(`proposal:${current}:${JSON.stringify(proposal)}`);
      if (writeFailure !== undefined) throw writeFailure;
      return new Map([["receipt", { account: "0", company: 0, kind: "CashDividend", event_id: "传输回执", approved_on: "2030-01-02" }]]);
    },
    enqueue_report_correction: (current: number, request: { operation_id: string }) => {
      calls.push(`correction-enqueue:${current}:${JSON.stringify(request)}`);
      if (writeFailure !== undefined) throw writeFailure;
      return { operation_id: request.operation_id };
    },
    cancel_report_correction: (current: number, operationId: string) => {
      calls.push(`correction-cancel:${current}:${operationId}`);
      if (writeFailure !== undefined) throw writeFailure;
      return { operation_id: operationId };
    },
  };
  replace(bindingsKey, bindings);
  replace("self", { crossOriginIsolated: true, postMessage: (message: Message) => { if (message.type === "engineLifecycle") globalLifecycle.push(message); }, addEventListener: (_type: string, callback: typeof listener) => { listener = callback; } });
  const emptyModule = [0,97,115,109,1,0,0,0];
  const exportNames = ["restore_json", "facade_slot_init_for_restore_json", "restore_prepared", "clear_prepared_restore"];
  const exportEntries = exportNames.flatMap((name, index) => [name.length, ...new TextEncoder().encode(name), 0, index]);
  const wasmBytes = Uint8Array.from(missingWrites === "initial-archive" ? [...emptyModule, 1,4,1,96,0,0, 3,5,4,0,0,0,0, 7,exportEntries.length+1,4,...exportEntries, 10,13,4,2,0,11,2,0,11,2,0,11,2,0,11] : emptyModule);
  const wasmHash = [...new Uint8Array(await crypto.subtle.digest("SHA-256", wasmBytes))].map((byte) => byte.toString(16).padStart(2, "0")).join("");
  replace("fetch", async () => ({ ok: true, arrayBuffer: async () => wasmBytes.buffer }));
  replace("setTimeout", (callback: () => void) => { const id = ++nextTimer; timers.set(id, callback); return id; });
  replace("clearTimeout", (id: number) => { timers.delete(id); });
  const id = ++fixtureId;
  register(registerHooks({
    resolve(specifier, context, nextResolve) {
      // fixture 自持 bindings；不依赖被 git 忽略的 WASM 构建产物。
      if (specifier === "../../wasm-pkg/web_wasm.js") {
        return { url: `virtual:wasm-pkg:owner-${id}`, shortCircuit: true };
      }
      if (specifier === "./engine-clock-env.ts") return { url: `virtual:owner-env:${id}`, shortCircuit: true };
      return nextResolve(specifier, context);
    },
    load(url, context, nextLoad) {
      if (url.startsWith("virtual:wasm-pkg:owner-")) {
        const exports = Object.keys(bindings)
          .map((key) => (missingWrites === true || missingWrites === "malformed") && ["propose_player_company_action", "enqueue_report_correction", "cancel_report_correction", "subscribe_rights_offering"].includes(key) ? `export const ${key} = ${missingWrites === "malformed" ? "17" : "undefined"};` : `export const ${key} = (...args) => bindings.${key}(...args);`).join("\n");
        return { shortCircuit: true, format: "module", source: `const bindings = globalThis[Symbol.for("stock-game-wasm-owner-tests")];\n${exports}` };
      }
      if (url.startsWith("virtual:owner-env:")) return { shortCircuit: true, format: "module", source: `import {createFacadeThreadBinding} from ${JSON.stringify(new URL("./facade-thread-binding.ts", import.meta.url).href)}; const binding=createFacadeThreadBinding({isWorker:()=>true}); export const {bindFacadeThreadEnvironment,installFacadeSlotLayout,getArenaView}=binding;` };
      const loaded = nextLoad(url, context);
      if (url.includes("/wasm-worker.ts?owner-test=")) {
        // 仅替换 Node 缺少的 Vite import.meta.env；协议 listener 与 owner 使用原源码。
        return { ...loaded, source: String(loaded.source).replaceAll("import.meta.env", '({ MODE: "e2e", DEV: false })') };
      }
      return loaded;
    },
  }));
  await import(`./wasm-worker.ts?owner-test=${id}`);
  assert.ok(listener);
  const control = new MessageChannel(); control.port1.addEventListener("message", (event) => { const value = event.data as Message; control.port1.postMessage(value.type === "facadeEngineMemory" ? { type: "facadeEngineMemoryRegistered", allocationId: 1 } : { type: "facadeEngineConfigured" }); }); control.port1.start();
  const business = new MessageChannel();
  const actualPost = business.port1.postMessage.bind(business.port1);
  business.port1.postMessage = (value: unknown) => {
    const message = value as Message;
    if (message.type === failingPost) throw new DOMException("发送失败", "DataCloneError");
    posted.push(message);
    if (message.type === "ready" || message.type === "failure") initialized();
    actualPost(value);
  };
  business.port1.start();
  const send = (message: Message) => { business.port1.dispatchEvent(new MessageEvent("message", { data: message })); };
  // 从 owner 端经真实 MessagePort 投递：覆盖浏览器队列与 structured clone，不手工 dispatch。
  const sendAsync = async (message: Message, settled: () => boolean) => {
    business.port2.postMessage(message);
    for (let count = 0; count < 100; count++) {
      if (settled()) return;
      await setImmediate();
    }
    throw new Error(`真实 MessagePort 未在期限内处理 ${message.type}`);
  };
  listener!({ data: { type: "bindEngineTransport", instanceId: 1, dataPort: business.port1, controlPort: control.port2 } });
  send({ type: "init", ...(missingWrites === "initial-archive" ? { initialArchive: saved, restoreCapability: { kind: "verified-package", wasmSha256: wasmHash, glueSha256: "b".repeat(64), sourceSha256: "c".repeat(64), buildManifestSha256: "d".repeat(64), exports: exportNames } } : { setup: {} }) });
  await ready;
  assert.deepEqual(posted, [{ type: "initializationProgress", phase: "engine" }, { type: "ready" }]);
  posted.length = 0;
  return { send, sendAsync, calls, posted: posted as Message[], globalLifecycle, timers, saved, handle: (next: number) => { handle = next; }, fail: (next: typeof failing) => { failing = next; }, failPost: (type: string | null) => { failingPost = type; }, failWrite: (error: unknown) => { writeFailure = error; } };
}

const correctionWriteRequest = { operation_id: "传输检查", company: 0, supersedes: 7, entries: [{ source: 900000001, date: "2030-01-02", kind: "CashRevenue", cash_flow: "Operating", lines: [{ account: "1002", side: "Debit", amount: "10.00" }, { account: "6001", side: "Credit", amount: "10.00" }] }], reason: "synthetic transport fixture" };
const companyWrites = [
  { type: "subscribeRightsOffering", payload: { event_id: "r1", requested_shares: "5" }, successType: "rightsSubscriptionAcknowledged", result: { event_id: "r1", requested_shares: "5", submitted_on: "2030-01-02" }, call: "rights:7:r1:5" },
  { type: "proposePlayerCompanyAction", payload: { proposal: { CashDividend: { company: 0, gross_per_share: "5" } } }, successType: "playerCompanyActionAcknowledged", result: { receipt: { account: "0", company: 0, kind: "CashDividend", event_id: "传输回执", approved_on: "2030-01-02" } }, call: 'proposal:7:{"CashDividend":{"company":0,"gross_per_share":"5"}}' },
  { type: "enqueueReportCorrection", payload: { request: correctionWriteRequest }, successType: "reportCorrectionEnqueueAcknowledged", result: { operation_id: "传输检查" }, call: `correction-enqueue:7:${JSON.stringify(correctionWriteRequest)}` },
  { type: "cancelReportCorrection", payload: { operation_id: "传输检查" }, successType: "reportCorrectionCancelAcknowledged", result: { operation_id: "传输检查" }, call: "correction-cancel:7:传输检查" },
] as const;

for (const row of companyWrites) {
  test(`真实 Worker listener 注册 ${row.type} 并调用对应 binding`, { timeout: 10000 }, async () => {
    await withWorker(async f => {
      f.send({ type: "create", seed: 1n });
      assert.equal(f.posted.some(message => message.type === "created" && message.generation === 1), true);
      f.posted.length = 0;
      f.send({ type: "civilDate", requestId: 50, generation: 1 });
      assert.deepEqual(f.posted, [{ type: "civilDate", requestId: 50, generation: 1, date: "2030-01-02" }], "先证明当前 handle/generation 的既有请求正控成功");
      f.posted.length = 0;
      f.send({ type: row.type, requestId: 51, generation: 1, ...row.payload });
      try {
        assert.deepEqual(f.posted, [{ type: row.successType, requestId: 51, generation: 1, result: row.result }], "合法三写必须到达实际 listener 的对应命令分支");
        assert.equal(f.calls.includes(row.call), true);
      } finally { f.send({ type: "drop" }); }
    });
  });
}

for (const row of companyWrites) {
  test(`Worker ${row.type} 调用前校验 generation、safe requestId 与 exact 顶层`, { timeout: 10000 }, async () => {
    await withWorker(async f => {
      f.send({ type: "create", seed: 1n });
      const before = f.calls.length;
      for (const requestId of [undefined, NaN, 0, -1, 1.5, Number.MAX_SAFE_INTEGER + 1]) {
        f.posted.length = 0;
        f.send({ type: row.type, requestId, generation: 1, ...row.payload });
        assert.match(String(f.posted[0].message), /requestId.*正安全整数/);
      }
      f.posted.length = 0;
      f.send({ type: row.type, generation: 0, ...row.payload });
      assert.match(String(f.posted[0].message), /过期会话/, "generation 门必须先于缺 requestId");
      for (const extra of ["account", "day", "epoch", "nonce", "proof", "unknown"]) {
        f.posted.length = 0;
        f.send({ type: row.type, requestId: 7, generation: 1, ...row.payload, [extra]: 1 });
        assert.match(String(f.posted[0].message), new RegExp(extra));
      }
      f.posted.length = 0;
      f.send({ type: row.type, requestId: 7, generation: 1 });
      assert.match(String(f.posted[0].message), /必填字段/);
      assert.equal(f.calls.length, before, "非法 envelope 不得触发任何 Rust binding");
      f.send({ type: "drop" });
    });
  });
  test(`Worker ${row.type} 显式拒绝缺失 binding`, { timeout: 10000 }, async () => {
    for (const missing of [true, "malformed"] as const) await withWorker(async f => {
      f.send({ type: "create", seed: 1n }); f.posted.length = 0;
      f.send({ type: row.type, requestId: 7, generation: 1, ...row.payload });
      assert.equal(f.posted[0].type, "operationError");
      assert.match(String(f.posted[0].message), /bindings.*不支持.*重建/);
      assert.equal(Object.hasOwn(f.posted[0], "failure"), false);
      f.send({ type: "drop" });
    }, missing);
  });
}

test("Worker 三写 typed 业务拒绝保关联与公开 failure，同会话继续可读", { timeout: 10000 }, async () => {
  await withWorker(async f => {
    f.send({ type: "create", seed: 1n });
    for (const row of companyWrites) for (const classification of ["InvalidInput", "BusinessCondition", "UnsupportedOperation", "SystemState"]) {
      const failure = row.type === "subscribeRightsOffering" ? { kind: "RightsSubscription", data: { account: "0", event_id: "r1", classification, reason: "具体业务拒绝，非 fatal" } } : row.type === "proposePlayerCompanyAction"
        ? { kind: "PlayerProposal", data: { account: "0", company: 0, kind: "CashDividend", day: "2030-01-02", classification, reason: "具体业务拒绝，非 fatal" } }
        : { kind: "ReportCorrection", data: { kind: "NotPending", data: { operation_id: "传输检查" } } };
      f.failWrite(failure); f.posted.length = 0;
      f.send({ type: row.type, requestId: 7, generation: 1, ...row.payload });
      assert.equal(f.posted[0].type, "operationError");
      assert.equal(f.posted[0].generation, 1); assert.equal(f.posted[0].requestId, 7);
      assert.deepEqual(f.posted[0].failure, failure);
      assert.match(String(f.posted[0].message), /具体业务拒绝|NotPending/);
      f.send({ type: "civilDate", requestId: 8, generation: 1 });
      assert.equal(f.posted.at(-1)?.type, "civilDate");
    }
    f.send({ type: "drop" });
  });
});

test("Worker recognized 畸形或错 family 为协议故障，真正 Step 仍沿既有 fatal 链", { timeout: 10000 }, async () => {
  for (const error of [
    { kind: "PlayerProposal", data: {} }, { kind: "ReportCorrection", data: { kind: "StaleEpoch" } },
    { code: "STEP_FATAL", message: "真实 Step 形状的故障", context: { tick: 7 } },
  ]) await withWorker(async f => {
    f.send({ type: "create", seed: 1n }); f.send({ type: "start", requestId: 1, generation: 1 });
    f.failWrite(error); f.posted.length = 0;
    f.send({ type: "proposePlayerCompanyAction", requestId: 7, generation: 1, proposal: { CashDividend: { company: 0, gross_per_share: "5" } } });
    assert.equal(f.posted[0].type, "failure"); assert.equal(f.timers.size, 0);
    if ("code" in error) {
      assert.equal(f.posted[0].code, error.code); assert.deepEqual(f.posted[0].context, error.context);
    } else {
      assert.equal(f.posted[0].code, "WASM_WORKER_PROTOCOL");
      const cause = f.posted[0].cause as { failure: unknown; decoderError: unknown };
      assert.deepEqual(cause.failure, error);
      assert.ok(cause.decoderError instanceof Error, "协议故障保真实 decoder Error");
      assert.match(String(f.posted[0].message), /协议/);
    }
    f.send({ type: "drop" });
  });
});

test("Worker 发送 typed operationError 失败同时报告公开 failure 和发送原因", { timeout: 10000 }, async () => {
  await withWorker(async f => {
    f.send({ type: "create", seed: 1n });
    const failure = { kind: "ReportCorrection", data: { kind: "StaleEpoch" } };
    f.failWrite(failure); f.failPost("operationError"); f.posted.length = 0;
    f.send({ type: "cancelReportCorrection", requestId: 7, generation: 1, operation_id: "传输检查" });
    assert.equal(f.posted[0].type, "failure");
    assert.match(String(f.posted[0].message), /发送 operationError 失败.*发送失败/);
    assert.deepEqual((f.posted[0].cause as { failure: unknown }).failure, failure);
    f.failPost(null); f.send({ type: "drop" });
  });
});

test("Worker 全部会话 caller 共用 owner，restore 完整回应先于 microtask restart", { timeout: 10000 }, async () => {
  await withWorker(async (f) => {
    f.send({ type: "create", seed: 1n });
    assert.equal(f.posted[0].type, "ingress");
    assert.equal(f.posted[0].token, 7);
    assert.equal(f.posted[0].generation, 1);
    assert.equal(f.posted[0].shared, true);
    assert.equal(Object.hasOwn(f.posted[0], "memory"), false, "ingress 复用 supervisor 已接收的唯一 substrate");
    assert.deepEqual(f.posted.slice(1), [
      { type: "created", generation: 1, capabilities: { npcDecisionDiagnostics: false, indicatorCapabilities: { intradayAverage: false, macd: false, priceKdj: false, candleKdj: false }, personalTradeHistory: false } },
      { ...catalogWorkerFixture(snapshot(0, 0)), type: "baseline", generation: 1, snapshot: snapshot(0, 0), civilDate: "2030-01-02" },
    ]);
    f.posted.length = 0;
    f.send({ type: "save", generation: 1, requestId: 1 });
    assert.deepEqual(f.posted, [{ type: "saved", requestId: 1, generation: 1, slot: f.saved }]);
    f.send({ type: "start", generation: 1, requestId: 20 });
    assert.deepEqual(f.posted.at(-1), { type: "started", generation: 1, requestId: 20 });
    assert.equal(f.timers.size, 1);
    f.posted.length = 0;
    f.handle(9);
    f.send({ type: "restore", generation: 1, requestId: 2, slot: f.saved });
    assert.equal(f.timers.size, 0, "restore 完成回应之前不能同步 restart");
    assert.equal(f.posted[0].type, "ingress");
    assert.equal((f.posted[0] as Message).token, 9);
    assert.equal(f.posted[0].generation, 2);
    assert.deepEqual(f.posted.slice(1), [
      { ...catalogWorkerFixture(snapshot(0, 0)), type: "restored", generation: 1, nextGeneration: 2, requestId: 2, snapshot: snapshot(0, 0), civilDate: "2031-02-03" },
    ]);
    await Promise.resolve();
    assert.equal(f.timers.size, 1);
    f.posted.length = 0;
    f.send({ type: "save", generation: 1, requestId: 3 });
    assert.deepEqual(f.posted, [{ type: "operationError", requestId: 3, generation: 1, message: "Worker 请求属于已过期会话" }]);
    f.send({ type: "stop", generation: 2, requestId: 21 });
    assert.deepEqual(f.posted.at(-1), { type: "stopped", generation: 2, requestId: 21 });
    f.send({ type: "stepOnce", generation: 2, requestId: 4 });
    assert.ok(f.calls.includes("step:9"));
    assert.deepEqual(f.posted.at(-1), { type: "stepped", generation: 2, requestId: 4, tick: 1 });
    f.posted.length = 0;
    f.send({ type: "setPausePreferences", generation: 2, requestId: 5, preferences: { pause_after_close: true, pause_before_open: false } });
    f.send({ type: "start", generation: 2, requestId: 22 });
    f.send({ type: "endCivilDay", generation: 2, requestId: 6 });
    assert.deepEqual(f.posted.map((message) => message.type), ["pausePreferencesSet", "started", "protocol", "barrierPaused", "civilDayEnded"]);
    assert.equal(f.timers.size, 0);
    f.send({ type: "start", generation: 2, requestId: 23 });
    f.send({ type: "drop" });
    assert.equal(f.timers.size, 0);
    assert.equal(f.calls.at(-1), "drop:9");
  });
});

test("Worker restore提交前snapshot失败恢复旧运行，提交后prepare失败fatal关闭新会话", { timeout: 10000 }, async () => {
  await withWorker(async f => {
    f.send({ type: "create", seed: 1n });
    f.send({ type: "start", generation: 1, requestId: 20 });
    f.handle(9); f.fail("snapshot"); f.posted.length = 0;
    f.send({ type: "restore", generation: 1, requestId: 10, slot: f.saved });
    assert.equal(f.timers.size, 0);
    assert.deepEqual(f.posted, [{ type: "operationError", requestId: 10, generation: 1, message: "snapshot failed" }]);
    await Promise.resolve();
    assert.equal(f.timers.size, 1);
    f.fail("prepare"); f.posted.length = 0;
    f.send({ type: "restore", generation: 1, requestId: 11, slot: f.saved });
    await Promise.resolve();
    assert.equal(f.timers.size, 0);
    assert.equal(f.posted[0].type, "failure");
    assert.equal(f.posted[0].generation, 2);
    assert.equal((f.posted[0] as Message).code, "WASM_RESTORE_COMMITTED_FAILED");
    assert.ok(f.calls.includes("drop:9"));
    f.fail(null); f.send({ type: "drop" });
  });
});

test("真实Worker restore已换handle后catalog故障保持3019，drain后不重启或发成功", { timeout: 10000 }, async () => {
  await withWorker(async f => {
    f.send({ type: "create", seed: 1n });
    f.send({ type: "start", generation: 1, requestId: 1 });
    f.handle(9);
    f.fail("catalog");
    f.posted.length = 0;
    f.send({ type: "restore", generation: 1, requestId: 2, slot: f.saved });
    await Promise.resolve();
    await Promise.resolve();
    assert.equal(f.timers.size, 0);
    assert.ok(f.calls.includes("drop:7"));
    assert.ok(f.calls.includes("drop:9"), "catalog故障必须清理新handle");
    assert.equal(f.posted.some(message => message.type === "restored" || message.type === "baseline"), false);
    const failure = f.posted.find(message => message.type === "failure");
    assert.ok(failure);
    assert.equal(failure.code, "CATALOG_BASELINE_FAILED");
    assert.deepEqual((failure.context as { fault: unknown }).fault, { code: 3019, context: [7] });
    assert.equal(failure.generation, 2);
    f.posted.length = 0;
    f.send({ type: "save", generation: 1, requestId: 3 });
    assert.equal(f.posted[0].type, "operationError");
    f.send({ type: "drop" });
  });
});

test("真实Worker restore提交前snapshot失败按旧handle/generation恢复运行", { timeout: 10000 }, async () => {
  await withWorker(async f => {
    f.send({ type: "create", seed: 1n });
    f.send({ type: "start", generation: 1, requestId: 1 });
    f.handle(9); f.fail("snapshot"); f.posted.length = 0;
    f.send({ type: "restore", generation: 1, requestId: 2, slot: f.saved });
    await Promise.resolve();
    assert.equal(f.timers.size, 1);
    f.fail(null); f.posted.length = 0;
    f.send({ type: "save", generation: 1, requestId: 3 });
    assert.equal(f.posted[0].type, "saved");
    f.send({ type: "drop" });
  });
});

test("真实Worker replace后旧drop失败属于已提交fatal，旧generation不能操作新handle", { timeout: 10000 }, async () => {
  await withWorker(async f => {
    f.send({ type: "create", seed: 1n });
    f.send({ type: "start", generation: 1, requestId: 1 });
    f.handle(9); f.fail("drop"); f.posted.length = 0;
    f.send({ type: "restore", generation: 1, requestId: 2, slot: f.saved });
    await Promise.resolve();
    assert.equal(f.timers.size, 0);
    assert.equal(f.posted[0].type, "failure");
    assert.equal(f.posted[0].generation, 2);
    f.fail(null); f.posted.length = 0;
    f.send({ type: "save", generation: 1, requestId: 3 });
    assert.equal(f.posted[0].type, "operationError");
    assert.equal(f.calls.includes("save:9"), false);
    f.send({ type: "drop" });
  });
});

test("真实Worker refresh目录fatal关闭当前handle以撤销独立intake能力", { timeout: 10000 }, async () => {
  await withWorker(async f => {
    f.send({ type: "create", seed: 1n });
    f.send({ type: "start", generation: 1, requestId: 1 });
    f.fail("catalog"); f.posted.length = 0;
    f.send({ type: "refreshBaseline", generation: 1, requestId: 2 });
    await Promise.resolve();
    assert.equal(f.timers.size, 0);
    assert.ok(f.calls.includes("drop:7"), "drop_session必须关闭真实SessionRegistry token，不能只停止主Worker loop");
    assert.equal(f.posted.some(message => message.type === "refreshed"), false);
    assert.equal(f.posted[0].type, "failure");
    f.fail(null); f.posted.length = 0;
    f.send({ type: "save", generation: 1, requestId: 3 });
    assert.equal(f.posted[0].type, "operationError");
    assert.equal(f.calls.includes("save:7"), false);
    f.send({ type: "drop" });
  });
});


for (const failureType of ["ingress", "restored"] as const) {
  test(`真实Worker已提交restore发送${failureType}失败fatal且不重启`, { timeout: 10000 }, async () => {
    await withWorker(async f => {
      f.send({ type: "create", seed: 1n });
      f.send({ type: "start", generation: 1, requestId: 1 });
      f.handle(9); f.failPost(failureType); f.posted.length = 0;
      f.send({ type: "restore", generation: 1, requestId: 2, slot: f.saved });
      await Promise.resolve(); await Promise.resolve();
      assert.equal(f.timers.size, 0);
      assert.ok(f.calls.includes("drop:9"));
      assert.equal(f.posted.some(message => message.type === "restored" || message.type === "baseline"), false);
      assert.equal(f.posted.at(-1)?.type, "failure");
      assert.equal(f.posted.at(-1)?.generation, 2);
      f.failPost(null); f.posted.length = 0;
      for (const generation of [1, 2]) f.send({ type: "save", generation, requestId: generation + 10 });
      assert.deepEqual(f.posted.map(message => message.type), ["operationError", "operationError"]);
      assert.equal(f.calls.includes("save:9"), false);
      f.send({ type: "drop" });
    });
  });
}

test("真实Worker暂停旧会话的precommit恢复失败保持暂停", { timeout: 10000 }, async () => {
  await withWorker(async f => {
    f.send({ type: "create", seed: 1n });
    f.handle(9); f.fail("snapshot"); f.posted.length = 0;
    f.send({ type: "restore", generation: 1, requestId: 2, slot: f.saved });
    await Promise.resolve(); await Promise.resolve();
    assert.equal(f.timers.size, 0);
    assert.equal(f.posted[0].type, "operationError");
    f.fail(null); f.posted.length = 0;
    f.send({ type: "save", generation: 1, requestId: 3 });
    assert.equal(f.posted[0].type, "saved");
    assert.ok(f.calls.includes("save:7"));
    f.send({ type: "drop" });
  });
});


test("非目录可恢复structured故障维持既有stop与显错，不销毁当前handle", { timeout: 10000 }, async () => {
  await withWorker(async f => {
    f.send({ type: "create", seed: 1n });
    f.fail("recoverable"); f.posted.length = 0;
    f.send({ type: "refreshBaseline", generation: 1, requestId: 1 });
    assert.equal(f.posted[0].type, "failure");
    assert.equal(f.calls.includes("drop:7"), false);
    f.fail(null); f.posted.length = 0;
    f.send({ type: "refreshBaseline", generation: 1, requestId: 2 });
    assert.equal(f.posted[0].type, "refreshed");
    f.send({ type: "drop" });
  });
});


test("真实Worker precommit snapshot与candidate cleanup双失败保旧身份并恢复运行且显两错", { timeout: 10000 }, async () => {
  await withWorker(async f => {
    f.send({ type: "create", seed: 1n });
    f.send({ type: "start", generation: 1, requestId: 1 });
    f.handle(9); f.fail("snapshot-and-drop"); f.posted.length = 0;
    f.send({ type: "restore", generation: 1, requestId: 2, slot: f.saved });
    await Promise.resolve(); await Promise.resolve();
    assert.equal(f.timers.size, 1);
    assert.equal(f.posted[0].type, "operationError");
    assert.match(String(f.posted[0].message), /snapshot failed/);
    assert.match(String(f.posted[0].message), /drop failed/);
    f.fail(null); f.posted.length = 0;
    f.send({ type: "save", generation: 1, requestId: 3 });
    assert.equal(f.posted[0].type, "saved");
    assert.ok(f.calls.includes("save:7"));
    f.send({ type: "drop" });
  });
});


test("Worker 公司查询 dispatch 使用当前唯一 handle，旧 generation 不读且域失败明确回传", { timeout: 10000 }, async () => {
  await withWorker(async f => {
    f.send({ type: "create", seed: 1n });
    f.posted.length = 0;
    const query = { kind: "OwnerRightsOfferings" };
    f.send({ type: "companyReadQuery", requestId: 21, generation: 1, query });
    assert.ok(f.calls.includes('company-read:7:{"kind":"OwnerRightsOfferings"}'));
    assert.deepEqual(f.posted.at(-1), { type: "companyReadQuery", requestId: 21, generation: 1, result: { owner: "0", value: { kind: "OwnerRightsOfferings", data: [] } } });
    const before = f.calls.filter(call => call.startsWith("company-read:")).length;
    f.send({ type: "companyReadQuery", requestId: 22, generation: 0, query });
    assert.equal(f.calls.filter(call => call.startsWith("company-read:")).length, before);
    assert.equal(f.posted.at(-1)?.type, "operationError");
    f.fail("company-read");
    f.send({ type: "companyReadQuery", requestId: 23, generation: 1, query });
    assert.match(String(f.posted.at(-1)?.message), /本人公司查询的实际拒绝原因/);
    f.fail("company-read-typed");
    f.send({ type: "companyReadQuery", requestId: 24, generation: 1, query });
    await setImmediate();
    assert.deepEqual(f.posted.at(-1), {
      type: "operationError", requestId: 24, generation: 1, message: "实际公司拒绝原因",
      failure: { kind: "CorporateState", data: { reason: "实际公司拒绝原因" } },
    }, "typed 公司拒绝须原样保留完整 failure 与请求身份");
  });
});

for (const outcome of ["success", "failure"] as const) {
  test(`生产 Worker shutdown ${outcome} 停 loop、保完整结果并撤销后续 WASM 调用`, { timeout: 10000 }, async () => {
    await withWorker(async f => {
      f.send({ type: "create", seed: 1n });
      f.send({ type: "start", requestId: 901, generation: 1 });
      if (outcome === "failure") f.fail("shutdown");
      f.posted.length = 0;
      f.send({ type: "shutdown", requestId: 902, generation: 1 });
      assert.equal(f.timers.size, 0);
      assert.equal(f.calls.filter((call) => call === "shutdown:7").length, 1);
      const reply = f.posted.find((message) => message.type === "shutdownCompleted");
      assert.ok(reply); assert.equal(reply.generation, 1); assert.equal(reply.requestId, 902);
      const report = reply.report as { kind: string; primary?: { detail: string }; secondary?: { detail: string }[]; completion?: { joinedWorkers: number } };
      assert.equal(report.kind, outcome);
      if (outcome === "failure") {
        assert.equal(report.primary?.detail, "真实 primary fixture");
        assert.equal(report.secondary?.[0].detail, "真实 secondary fixture");
        assert.equal(report.completion?.joinedWorkers, 1);
      }
      const before = [...f.calls];
      for (const type of ["start", "restore", "civilDate", "drop", "shutdown"]) f.send({ type, requestId: 903, generation: 1, slot: f.saved });
      assert.deepEqual(f.calls, before, "closing 后不得重入 Rust 或 drop 已消费 handle");
    });
  });
}

test("真实 MessagePort 异步重复 create 显式失败，不覆盖旧 Registry/World", { timeout: 10000 }, async () => {
  await withWorker(async f => {
    await f.sendAsync({ type: "create", seed: 1n }, () => f.posted.some((message) => message.type === "baseline"));
    assert.equal(f.calls.filter((call) => call.startsWith("create:")).length, 1);
    f.posted.length = 0;
    await f.sendAsync({ type: "create", seed: 2n }, () => f.posted.length > 0);
    assert.equal(f.posted.length, 1);
    assert.equal(f.posted[0].type, "failure");
    assert.match(String(f.posted[0].message), /只能初始化一次/);
    assert.equal(f.calls.filter((call) => call.startsWith("create:")).length, 1, "第二次 create 不得进入 FFI 覆盖旧 handle");
    assert.deepEqual(f.posted.filter((message) => ["ingress", "created", "baseline"].includes(message.type)), []);
    f.posted.length = 0;
    await f.sendAsync({ type: "civilDate", requestId: 1, generation: 1 }, () => f.posted.some((message) => message.type === "civilDate"));
    assert.equal(f.posted.at(-1)?.type, "civilDate", "旧会话在重复 create 被拒绝后仍保持有效");
    await f.sendAsync({ type: "drop" }, () => f.calls.includes("drop:7"));
    assert.deepEqual(f.calls.filter((call) => call.startsWith("drop:")), ["drop:7"]);
  });
});

test("真实 MessagePort 异步首次 create FFI 失败后禁止重试并显式传播原始错误", { timeout: 10000 }, async () => {
  await withWorker(async f => {
    f.fail("create");
    f.posted.length = 0;
    await f.sendAsync({ type: "create", seed: 1n }, () => f.posted.length > 0);
    assert.equal(f.posted.length, 1);
    assert.equal(f.posted[0].type, "failure");
    assert.match(String(f.posted[0].message), /create failed/);
    assert.equal(f.posted[0].generation, 0);
    f.fail(null);
    f.posted.length = 0;
    await f.sendAsync({ type: "create", seed: 1n }, () => f.posted.length > 0);
    assert.equal(f.posted.length, 1);
    assert.equal(f.posted[0].type, "failure");
    assert.match(String(f.posted[0].message), /只能初始化一次/);
    assert.equal(f.calls.filter((call) => call.startsWith("create:")).length, 1, "失败后的重试不得再次进入 FFI");
    assert.equal(f.posted.some((message) => ["ingress", "created", "baseline"].includes(message.type)), false);
    f.posted.length = 0;
    await f.sendAsync({ type: "start", requestId: 2, generation: 1 }, () => f.posted.length > 0);
    assert.equal(f.posted.at(-1)?.type, "operationError");
    assert.match(String(f.posted.at(-1)?.message), /已过期会话/);
  });
});

test("真实 MessagePort 异步 create 参数合同仅允许 type 与 seed，失败请求不消耗唯一创建机会", { timeout: 10000 }, async () => {
  await withWorker(async f => {
    for (const row of [
      { extra: { setup: {} }, response: "failure" },
      { extra: { initialArchive: { second: "ignored" } }, response: "failure" },
      { extra: { requestId: 1 }, response: "operationError" },
    ] as const) {
      f.posted.length = 0;
      await f.sendAsync({ type: "create", seed: 1n, ...row.extra }, () => f.posted.length > 0);
      assert.equal(f.posted[0].type, row.response);
      assert.match(String(f.posted[0].message), /不是允许字段/);
      assert.equal(f.calls.some((call) => call.startsWith("create:")), false, "参数校验失败不得进入首次 create FFI");
    }
    f.posted.length = 0;
    await f.sendAsync({ type: "create", seed: 1n }, () => f.posted.some((message) => message.type === "baseline"));
    assert.equal(f.posted.some((message) => message.type === "created" && message.generation === 1), true);
    assert.equal(f.calls.filter((call) => call.startsWith("create:")).length, 1);
    await f.sendAsync({ type: "drop" }, () => f.calls.includes("drop:7"));
  });
});

test("真实 MessagePort 异步 create 的 seed 参数错误不消耗唯一创建机会", { timeout: 10000 }, async () => {
  await withWorker(async f => {
    for (const message of [{ type: "create" }, { type: "create", seed: 1 }] as const) {
      f.posted.length = 0;
      await f.sendAsync(message, () => f.posted.length > 0);
      assert.equal(f.posted[0].type, "failure");
      assert.match(String(f.posted[0].message), /seed|必填/);
      assert.equal(f.calls.some((call) => call.startsWith("create:")), false);
    }
    f.posted.length = 0;
    await f.sendAsync({ type: "create", seed: 1n }, () => f.posted.some((message) => message.type === "baseline"));
    assert.equal(f.calls.filter((call) => call.startsWith("create:")).length, 1);
    await f.sendAsync({ type: "drop" }, () => f.calls.includes("drop:7"));
  });
});

test("生产 Worker 初始完整 X 先完成同源 topology，再一次 restoreInitial 而非空 create", { timeout: 10000 }, async () => {
  await withWorker(async f => {
    assert.equal(f.calls.filter((call) => call === "initial-topology").length, 1);
    f.send({ type: "create", seed: 42n });
    assert.equal(f.calls.some((call) => call.startsWith("create:")), false);
    assert.equal(f.posted.length, 1);
    assert.equal(f.posted[0].type, "failure");
    assert.match(String(f.posted[0].message), /只能 restoreInitial/);
    f.posted.length = 0;
    f.send({ type: "restoreInitial" });
    assert.equal(f.calls.filter((call) => call === "restore:7").length, 1);
    assert.equal(f.posted.some((message) => message.type === "created" && message.generation === 1), true);
    assert.equal(f.posted.some((message) => message.type === "baseline" && message.generation === 1), true);
    f.send({ type: "restoreInitial" });
    assert.equal(f.calls.filter((call) => call === "restore:7").length, 1);
    f.send({ type: "shutdown", requestId: 911, generation: 1 });
  }, "initial-archive");
});


import { parseWorkerFailure } from "./worker-host.ts";

for (const variant of ["string", "unknown", "error", "getter"] as const) {
  test(`生产 Worker shutdown 非 envelope ${variant} 经真实 reporter 保留 FFI 主错与 decoder 次错`, { timeout: 10000 }, async () => {
    await withWorker(async f => {
      f.send({ type: "create", seed: 1n }); f.posted.length = 0;
      const original = variant === "string" ? "shutdown report serialize failed: SENTINEL" : variant === "unknown" ? { foreign: "FFI_SENTINEL" } : variant === "error" ? Object.assign(new Error("FFI_ERROR_SENTINEL"), { code: "FFI_SERIALIZE", context: { stage: "FFI_CONTEXT_SENTINEL" } }) : Object.defineProperty(Object.defineProperty({ result: {} }, "detail", { value: "FFI_GETTER_SENTINEL" }), "handle", { enumerable: true, get() { throw new Error("DECODER_GETTER_SENTINEL"); } });
      const bindings = (globalThis as unknown as Record<symbol, { shutdown_session: (current: number) => unknown }>)[bindingsKey];
      bindings.shutdown_session = current => { f.calls.push(`shutdown:${current}`); throw original; };
      f.send({ type: "shutdown", requestId: 31, generation: 1 });
      assert.equal(f.posted.length, 1);
      // 真实 reporter 已显式提取纯壳字段；此处再经过浏览器消息边界同等的克隆与主侧 parser。
      const failure = parseWorkerFailure(structuredClone(f.posted[0]));
      assert.equal(f.posted[0].type, "failure");
      assert.equal(failure.code, "WASM_SHUTDOWN_REPORT_INVALID");
      assert.equal(failure.where, "wasm-session-slot.shutdown");
      const cause = failure.cause as { ffiError: unknown; decoderError: { message: string } };
      if (variant === "string" || variant === "unknown") assert.deepEqual(cause.ffiError, original);
      if (variant === "error") {
        assert.equal((cause.ffiError as Record<string, unknown>).message, "FFI_ERROR_SENTINEL");
        assert.equal((cause.ffiError as Record<string, unknown>).code, "FFI_SERIALIZE");
        assert.deepEqual((cause.ffiError as Record<string, unknown>).context, { stage: "FFI_CONTEXT_SENTINEL" });
      }
      if (variant === "getter") {
        assert.equal((cause.ffiError as Record<string, unknown>).detail, "FFI_GETTER_SENTINEL");
        assert.equal(cause.decoderError.message, "DECODER_GETTER_SENTINEL");
      }
      assert.ok(cause.decoderError.message.length > 0);
      f.send({ type: "drop" });
      assert.deepEqual(f.calls.filter(call => call.startsWith("shutdown:") || call.startsWith("drop:")), ["shutdown:7"]);
    });
  });
}

 test("Worker 显式刷新 runtime 并回传权威基线，拒绝过期 generation", { timeout: 10000 }, async () => {
  await withWorker(async f => {
    f.send({ type: "create", seed: 1n });
    f.posted.length = 0;
    const preparations = f.calls.filter(call => call === "prepare:7").length;
    f.send({ type: "flushRuntimeSync", requestId: 7, generation: 1 });
    assert.equal(f.calls.filter(call => call === "prepare:7").length, preparations, "刷新不能清除尚未消费的日终 save candidate");
    assert.equal(f.calls.includes("flush:7"), true);
    assert.equal(f.posted[0].type, "runtimeSyncFlushed");
    assert.deepEqual(f.posted[0].snapshot, snapshot(0, 0));
    f.posted.length = 0;
    f.send({ type: "flushRuntimeSync", requestId: 8, generation: 0 });
    assert.equal(f.calls.filter(call => call === "flush:7").length, 1);
    assert.notEqual(f.posted[0].type, "runtimeSyncFlushed");
    f.send({ type: "drop" });
  });
});

for (const field of ["setup", "initialArchive", "seed"] as const) {
  test(`生产 Worker restoreInitial 夹带 ${field} 失败并关闭该实例，不进入恢复 FFI`, { timeout: 10000 }, async () => {
    await withWorker(async f => {
      assert.deepEqual(f.calls, ["initial-topology"], "先完成唯一初始档的 topology 预检");
      const before = [...f.calls];
      const payload = field === "setup" ? {} : field === "initialArchive" ? f.saved : 42n;
      await f.sendAsync({ type: "restoreInitial", [field]: payload }, () => f.posted.length > 0);
      assert.equal(f.posted.length, 1);
      assert.equal(f.posted[0].type, "failure");
      assert.equal(f.posted[0].generation, 0);
      assert.match(String(f.posted[0].message), /禁止第二份 payload/);
      assert.deepEqual(f.calls, before, "非法 payload 不得创建或消费 Rust 会话");
      assert.equal(f.timers.size, 0);
      assert.equal(f.posted.some(message => ["ingress", "created", "baseline"].includes(message.type)), false);
      for (const message of [{ type: "restoreInitial" }, { type: "create", seed: 42n }, { type: "save", requestId: 1, generation: 0 }]) {
        f.posted.length = 0;
        await f.sendAsync(message, () => f.posted.length > 0);
        assert.equal(f.posted[0].type, message.type === "save" ? "operationError" : "failure");
        assert.match(String(f.posted[0].message), /正在关闭/);
        assert.deepEqual(f.calls, before, "关闭后的同实例不得重入 FFI");
        assert.equal(f.posted.some(reply => ["ingress", "created", "baseline"].includes(reply.type)), false);
      }
    }, "initial-archive");
  });
}

test("生产 Worker 默认新局拒绝 restoreInitial，不隐式建立或恢复会话", { timeout: 10000 }, async () => {
  await withWorker(async f => {
    const before = [...f.calls];
    await f.sendAsync({ type: "restoreInitial" }, () => f.posted.length > 0);
    assert.equal(f.posted.length, 1);
    assert.equal(f.posted[0].type, "failure");
    assert.equal(f.posted[0].generation, 0);
    assert.match(String(f.posted[0].message), /需要唯一完整初始档/);
    assert.deepEqual(f.calls, before);
    assert.equal(f.timers.size, 0);
    assert.equal(f.posted.some(message => ["ingress", "created", "baseline"].includes(message.type)), false);
  });
});
