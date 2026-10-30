import assert from "node:assert/strict";
import test, { after } from "node:test";
import { setImmediate } from "node:timers/promises";
import { parseWorkerFailure } from "./worker-host.ts";
import { classifyWasmFailure, describeWasmFailure } from "./wasm-failure.ts";
import { WorkerTransportFixture } from "./worker-transport.test-support.ts";

const posted: unknown[] = [];
let remainingPostFailures = 0;
const reportedErrors: unknown[] = [];
let reportErrorEnabled = true;
let messageListener: ((event: MessageEvent<Record<string, unknown> & { type: string }>) => void) | null = null;
const originalSelf = Object.getOwnPropertyDescriptor(globalThis, "self");
const originalMessageChannel = globalThis.MessageChannel;
const channels: MessageChannel[] = [];
globalThis.MessageChannel = class extends originalMessageChannel {
  constructor() {
    super();
    channels.push(this);
  }
};
after(() => {
  transport.close();
  for (const channel of channels) {
    channel.port1.close();
    channel.port2.close();
  }
  if (originalSelf === undefined) Reflect.deleteProperty(globalThis, "self");
  else Object.defineProperty(globalThis, "self", originalSelf);
});
Object.defineProperty(globalThis, "self", {
  configurable: true,
  value: {
    postMessage(message: unknown) {
      if (remainingPostFailures > 0) {
        remainingPostFailures -= 1;
        throw new Error(`port failure ${remainingPostFailures}`);
      }
      posted.push(structuredClone(message));
    },
    get reportError() { return reportErrorEnabled ? (error: unknown) => { reportedErrors.push(error); } : undefined; },
    addEventListener(_type: "message", listener: typeof messageListener) {
      messageListener = listener;
    },
  },
});
const { isE2EStepMode, postFailure } = await (async () => {
  try {
    return await import("./wasm-worker.ts");
  } finally {
    globalThis.MessageChannel = originalMessageChannel;
    if (originalSelf === undefined) Reflect.deleteProperty(globalThis, "self");
    else Object.defineProperty(globalThis, "self", originalSelf);
  }
})();

const transport = new WorkerTransportFixture();
if (messageListener === null) throw new Error("WASM Worker bootstrap listener 未安装");
transport.bindEngineTransport(messageListener as (event: MessageEvent<unknown>) => void, 1);
const workerBusinessPort = transport.channels[0]!.port2 as unknown as MessagePort;
const originalBusinessPost = workerBusinessPort.postMessage.bind(workerBusinessPort);
workerBusinessPort.postMessage = (message: unknown) => {
  if (remainingPostFailures > 0) {
    remainingPostFailures -= 1;
    throw new Error(`port failure ${remainingPostFailures}`);
  }
  posted.push(structuredClone(message));
  originalBusinessPost(message);
};
const ownerBusinessPort = transport.channels[0]!.port1 as unknown as MessagePort;
async function sendWorkerRequest(message: unknown) {
  transport.dispatch(ownerBusinessPort, message);
  for (let count = 0; count < 8; count++) await setImmediate();
}
const sendRawWorkerRequest = (event: { data: unknown }) => transport.dispatchRaw(workerBusinessPort, event.data);
Object.defineProperty(globalThis, "self", {
  configurable: true,
  value: {
    postMessage() { throw new Error("WASM Worker business output bypassed dataPort"); },
    get reportError() { return reportErrorEnabled ? (error: unknown) => { reportedErrors.push(error); } : undefined; },
    addEventListener(_type: string, listener: typeof messageListener) { messageListener = listener; },
  },
});

test("WASM Worker enables controlled stepping only for an explicit E2E build mode", () => {
  assert.equal(isE2EStepMode("e2e"), true);
  assert.equal(isE2EStepMode("production"), false);
  assert.equal(isE2EStepMode(undefined), false);
  assert.equal(isE2EStepMode({ MODE: "e2e" }), false);
});

test("Worker rejects stale save generation before looking up the current WASM candidate", async () => {
  posted.length = 0;
  await sendWorkerRequest({ type: "save", generation: 1, requestId: 99, candidate: { seq: 42, settledDate: "2030-01-05" } });
  assert.deepEqual(posted, [{ type: "operationError", requestId: 99, generation: 1, message: "Worker 请求属于已过期会话" }]);
});

test("WASM Worker preserves a structured HostFailure code and message", () => {
  posted.length = 0;

  postFailure("wasm-worker.step", {
    code: "STEP_FATAL",
    message: "invariant violation at web-wasm.step: receipt chain broke",
  });

  assert.deepEqual(posted, [{
    type: "failure",
    generation: 0,
    code: "STEP_FATAL",
    where: "wasm-worker.step",
    message: "invariant violation at web-wasm.step: receipt chain broke",
  }]);
});

test("WASM Worker 保留标准 Error 的 code 和非枚举 message", () => {
  posted.length = 0;
  const error = Object.assign(new Error("模块加载失败，原始原因必须可见"), { code: "MODULE_LOAD_FAILED" });

  postFailure("wasm-worker.init", error);

  assert.deepEqual(posted, [{
    type: "failure", generation: 0, code: "MODULE_LOAD_FAILED",
    where: "wasm-worker.init", message: "模块加载失败，原始原因必须可见",
  }]);
});

test("WASM Worker 保留非枚举 code 和有效的 HostFailure 可选字段", () => {
  posted.length = 0;
  const error = Object.assign(new Error("模块初始化失败", {
    cause: { code: "BINDINGS_INVALID", message: "导出不完整" },
  }), {
    where: "bindings.initialize",
    context: { module: "bindings" },
    recoverable: false,
    recoveryActions: ["反馈错误详情"],
  });
  Object.defineProperty(error, "code", { value: "MODULE_INIT_FAILED" });

  postFailure("wasm-worker.init", error);

  assert.deepEqual(posted, [{
    type: "failure", generation: 0, code: "MODULE_INIT_FAILED",
    where: "bindings.initialize", message: "模块初始化失败",
    cause: { code: "BINDINGS_INVALID", message: "导出不完整" },
    context: { module: "bindings" },
    recoverable: false, recoveryActions: ["反馈错误详情"],
  }]);
});

test("WASM Worker 仍显式拒绝非法的 HostFailure 可选字段", () => {
  for (const [details, reason] of [
    [{ recoverable: "yes" }, /HostFailure\.recoverable 必须是布尔值或 null/],
    [{ recoveryActions: [""] }, /HostFailure\.recoveryActions 必须是非空字符串/],
  ] as const) {
    posted.length = 0;
    const error = Object.assign(new Error("原始错误"), { code: "INIT_FAILED", ...details });

    assert.doesNotThrow(() => postFailure("wasm-worker.init", error));
    const failure = posted[0] as Record<string, unknown>;
    assert.equal(failure.code, "WASM_WORKER_PROTOCOL");
    assert.equal(failure.where, "wasm-worker.init");
    assert.match(String(failure.message), reason);
    assert.deepEqual(failure.cause, { code: "INIT_FAILED", message: "原始错误", ...details });
  }
});

test("WASM Worker 仍显式报告可选字段的 getter 和 Proxy 访问错误", () => {
  const accessFailure = new Error("读取 context 失败");
  const error = Object.assign(new Error("原始错误"), { code: "INIT_FAILED" });
  Object.defineProperty(error, "context", { enumerable: true, get() { throw accessFailure; } });
  const proxy = new Proxy({ code: "INIT_FAILED", message: "原始错误" }, {
    getOwnPropertyDescriptor() { throw accessFailure; },
  });
  for (const thrown of [error, proxy]) {
    posted.length = 0;
    assert.doesNotThrow(() => postFailure("wasm-worker.init", thrown));
    const failure = posted[0] as Record<string, unknown>;
    assert.equal(failure.code, "WASM_WORKER_PROTOCOL");
    assert.equal(failure.where, "wasm-worker.init");
    assert.match(String(failure.message), /读取结构化错误字段.*读取 context 失败/);
    assert.deepEqual(failure.cause, { code: "INIT_FAILED", message: "原始错误" });
  }
});

test("WASM Worker preserves an explicit engine error origin", () => {
  posted.length = 0;
  postFailure("wasm-worker.step", {
    code: "STEP_FATAL",
    where: "engine::commit_tick",
    message: "receipt hash mismatch",
  });
  assert.deepEqual(posted, [{
    type: "failure", generation: 0, code: "STEP_FATAL",
    where: "engine::commit_tick", message: "receipt hash mismatch",
  }]);
});

test("WASM Worker describes malformed object errors without Object string coercion", () => {
  posted.length = 0;

  postFailure("wasm-worker.step", { reason: "bad wasm boundary" });

  assert.deepEqual(posted, [{
    type: "failure",
    generation: 0,
    code: "WASM_WORKER_PROTOCOL",
    where: "wasm-worker.step",
    message: '非标准错误对象：{"reason":"bad wasm boundary"}',
  }]);
});

test("WASM Worker reports a structured-error accessor failure explicitly", () => {
  posted.length = 0;
  const malformed = Object.defineProperty({}, "code", {
    get() {
      throw new Error("getter exploded");
    },
  });

  postFailure("wasm-worker.step", malformed);

  assert.deepEqual(posted, [{
    type: "failure",
    generation: 0,
    code: "WASM_WORKER_PROTOCOL",
    where: "wasm-worker.step",
    message: "读取结构化错误字段 code 失败：getter exploded",
  }]);
});

test("WASM Worker can report an unprintable thrown object without failing again", () => {
  posted.length = 0;
  const unprintable: Record<PropertyKey, unknown> = {
    toJSON() {
      throw unprintable;
    },
    [Symbol.toPrimitive]() {
      throw unprintable;
    },
  };

  postFailure("wasm-worker.step", unprintable);

  assert.deepEqual(posted, [{
    type: "failure",
    generation: 0,
    code: "WASM_WORKER_PROTOCOL",
    where: "wasm-worker.step",
    message: "非标准错误对象无法序列化：<无法读取错误详情>",
  }]);
});

test("WASM Worker can report a revoked Proxy without failing again", () => {
  posted.length = 0;
  const { proxy, revoke } = Proxy.revocable({}, {});
  revoke();

  assert.doesNotThrow(() => postFailure("wasm-worker.step", proxy));
  assert.equal(posted.length, 1);
  const { message, ...failure } = posted[0] as Record<string, unknown>;
  assert.deepEqual(failure, {
    type: "failure",
    generation: 0,
    code: "WASM_WORKER_PROTOCOL",
    where: "wasm-worker.step",
  });
  assert.match(String(message), /^读取结构化错误字段 object 失败：/);
});

test("WASM Worker safely stringifies a non-string Error message", () => {
  posted.length = 0;
  const malformedError = Object.defineProperty(Object.create(Error.prototype) as Error, "message", {
    get() {
      return Symbol("bad");
    },
  });

  postFailure("wasm-worker.step", malformedError);

  assert.deepEqual(posted, [{
    type: "failure",
    generation: 0,
    code: "WASM_WORKER_PROTOCOL",
    where: "wasm-worker.step",
    message: "Symbol(bad)",
  }]);
});

test("WASM Worker safely stringifies a non-string serialization-error message", () => {
  posted.length = 0;
  const malformedError = Object.defineProperty(Object.create(Error.prototype) as Error, "message", {
    get() {
      return Symbol("bad");
    },
  });

  postFailure("wasm-worker.step", {
    toJSON() {
      throw malformedError;
    },
  });

  assert.deepEqual(posted, [{
    type: "failure",
    generation: 0,
    code: "WASM_WORKER_PROTOCOL",
    where: "wasm-worker.step",
    message: "非标准错误对象无法序列化：Symbol(bad)",
  }]);
});

test("WASM Worker routes a structured request failure through HostFailure", () => {
  posted.length = 0;
  const request = Object.defineProperty({
    type: "speedMetrics",
    requestId: 19,
  }, "generation", {
    get() {
      throw {
        code: "STEP_FATAL",
        message: "internal step failure: expected receipt hash differs",
      };
    },
  });

  assert.notEqual(messageListener, null);
  sendRawWorkerRequest({ data: request } as unknown as MessageEvent<Record<string, unknown> & { type: string }>);

  assert.deepEqual(posted, [{
    type: "failure",
    generation: 0,
    code: "STEP_FATAL",
    where: "wasm-worker.speedMetrics",
    message: "internal step failure: expected receipt hash differs",
  }]);
});


test("错误报告只读一次核心与详情 getter，并交付严格校验后的 clone 快照", () => {
  posted.length = 0;
  const reads: Record<string, number> = {};
  const values = { code: "STEP_FATAL", message: "原始故障", where: "engine.step", context: { tick: 7 }, recoveryActions: ["反馈详情"] };
  const error = Object.fromEntries(Object.keys(values).map((key) => [key, undefined]));
  for (const [key, value] of Object.entries(values)) {
    Object.defineProperty(error, key, { enumerable: true, get() {
      reads[key] = (reads[key] ?? 0) + 1;
      if (reads[key] > 1) throw new Error(`${key} 被重复读取`);
      return value;
    } });
  }
  postFailure("wasm-worker.step", error);
  assert.deepEqual(reads, { code: 1, message: 1, where: 1, context: 1, recoveryActions: 1 });
  assert.deepEqual(posted, [{ type: "failure", generation: 0, ...values }]);
  assert.notEqual((posted[0] as typeof values).context, values.context);
});

test("where getter 抛错和不可克隆详情均显式保留原故障及失败字段", () => {
  for (const [key, descriptor, reason] of [
    ["where", { get() { throw new Error("where getter exploded"); } }, /读取结构化错误字段 where 失败.*where getter exploded/],
    ["context", { value: { callback() {} } }, /克隆结构化错误字段 context 失败/],
  ] as const) {
    posted.length = 0;
    const error = Object.defineProperty({ code: "STEP_FATAL", message: "原始故障" }, key, descriptor);
    assert.doesNotThrow(() => postFailure("wasm-worker.step", error));
    const failure = posted[0] as Record<string, unknown>;
    assert.equal(failure.code, "WASM_WORKER_PROTOCOL");
    assert.match(String(failure.message), reason);
    assert.deepEqual(failure.cause, { code: "STEP_FATAL", message: "原始故障" });
  }
});

test("嵌套 recoveryActions getter 只克隆一次，非法快照仍由严格 parser 拒绝", () => {
  posted.length = 0;
  let reads = 0;
  const actions = Object.defineProperty([""], "0", { enumerable: true, get() { reads += 1; return 17; } });
  postFailure("wasm-worker.step", { code: "STEP_FATAL", message: "原始故障", recoveryActions: actions });
  assert.equal(reads, 1);
  const failure = posted[0] as Record<string, unknown>;
  assert.equal(failure.code, "WASM_WORKER_PROTOCOL");
  assert.match(String(failure.message), /HostFailure.recoveryActions 必须是非空字符串/);
  assert.deepEqual(failure.cause, { code: "STEP_FATAL", message: "原始故障", recoveryActions: [17] });
});

test("发送错误详情失败时第二条消息保留原始故障和发送原因", () => {
  posted.length = 0;
  remainingPostFailures = 1;
  postFailure("wasm-worker.step", { code: "STEP_FATAL", message: "原始故障" });
  assert.deepEqual(posted, [{
    type: "failure", generation: 0, code: "WASM_WORKER_PROTOCOL", where: "wasm-worker.step",
    message: "发送结构化错误详情失败：port failure 0",
    cause: { code: "STEP_FATAL", message: "原始故障", where: "wasm-worker.step" },
  }]);
});

test("错误消息双发送失败通过 reportError 暴露原故障及两次发送原因", () => {
  posted.length = 0;
  reportedErrors.length = 0;
  remainingPostFailures = 2;
  assert.doesNotThrow(() => postFailure("wasm-worker.step", { code: "STEP_FATAL", message: "原始故障" }));
  assert.deepEqual(posted, []);
  assert.equal(reportedErrors.length, 1);
  const error = reportedErrors[0] as Error;
  assert.match(error.message, /STEP_FATAL.*原始故障.*port failure 1.*port failure 0/);
  assert.deepEqual(error.cause, {
    failure: { code: "STEP_FATAL", message: "原始故障", where: "wasm-worker.step" },
    sendError: "port failure 1", lastResortError: "port failure 0",
  });
});

test("listener 的 getter 故障仍送出 failure 而不产生二次报告异常", () => {
  posted.length = 0;
  const error = Object.defineProperty({ code: "STEP_FATAL", message: "原始故障" }, "context", {
    get() { throw new Error("context getter exploded"); },
  });
  const request = Object.defineProperty({ type: "speedMetrics", requestId: 29 }, "generation", { get() { throw error; } });
  sendRawWorkerRequest({ data: request } as unknown as MessageEvent<Record<string, unknown> & { type: string }>);
  const failure = posted[0] as Record<string, unknown>;
  assert.equal(failure.code, "WASM_WORKER_PROTOCOL");
  assert.match(String(failure.message), /context getter exploded/);
  assert.deepEqual(failure.cause, { code: "STEP_FATAL", message: "原始故障" });
});


test("缺少 reportError 的旧环境通过 microtask 显式抛出完整故障详情", () => {
  posted.length = 0;
  remainingPostFailures = 2;
  reportErrorEnabled = false;
  const originalQueueMicrotask = globalThis.queueMicrotask;
  let scheduled: (() => void) | undefined;
  globalThis.queueMicrotask = (callback) => { scheduled = callback; };
  try {
    assert.doesNotThrow(() => postFailure("wasm-worker.step", { code: "STEP_FATAL", message: "原始故障" }));
    assert.equal(typeof scheduled, "function");
    assert.throws(() => scheduled!(), /STEP_FATAL.*原始故障.*port failure 1.*port failure 0/);
  } finally {
    reportErrorEnabled = true;
    globalThis.queueMicrotask = originalQueueMicrotask;
  }
  assert.deepEqual(posted, []);
});

test("operationError 发送失败改送 failure，保留请求原错误与传输原因", async () => {
  posted.length = 0;
  remainingPostFailures = 1;
  await sendWorkerRequest({ type: "save", generation: 1, requestId: 42 });
  assert.deepEqual(posted, [{
    type: "failure", generation: 0, code: "WASM_WORKER_PROTOCOL", where: "wasm-worker.save",
    message: "发送 operationError 失败：port failure 0；原始错误：Worker 请求属于已过期会话",
    cause: { message: "Worker 请求属于已过期会话" },
  }]);
});

test("空白原生 Error message 明确报协议错误，交付非空说明及原 message", () => {
  posted.length = 0;
  postFailure("wasm-worker.init", new Error(""));
  assert.deepEqual(posted, [{
    type: "failure", generation: 0, code: "WASM_WORKER_PROTOCOL", where: "wasm-worker.init",
    message: "非结构化错误没有可读的非空 message", cause: { message: "" },
  }]);
});


test("message getter 失败仍保留已读 code 并标明失败字段", () => {
  posted.length = 0;
  let reads = 0;
  const error = Object.defineProperty({ code: "STEP_FATAL" }, "message", { get() {
    reads += 1;
    throw new Error("message getter exploded");
  } });
  postFailure("wasm-worker.step", error);
  assert.equal(reads, 1);
  assert.deepEqual(posted, [{
    type: "failure", generation: 0, code: "WASM_WORKER_PROTOCOL", where: "wasm-worker.step",
    message: "读取结构化错误字段 message 失败：message getter exploded；已读原始错误：code=STEP_FATAL",
    cause: { code: "STEP_FATAL" },
  }]);
});

test("非结构化 Error 与 object 的核心 getter 只读一次，listener 复用分类说明", () => {
  for (const listener of [false, true]) {
    for (const nativeError of [false, true]) {
      posted.length = 0;
      const reads = { code: 0, message: 0 };
      const error = Object.defineProperties(nativeError ? new Error() : {}, {
        code: { enumerable: true, get() { reads.code += 1; if (reads.code > 1) throw new Error("code read twice"); return undefined; } },
        message: { enumerable: true, get() { reads.message += 1; if (reads.message > 1) throw new Error("message read twice"); return "原始故障"; } },
      });
      if (listener) {
        let generationReads = 0;
        const request = Object.defineProperty({ type: "speedMetrics", requestId: 49 }, "generation", { get() {
          generationReads += 1;
          if (generationReads === 1) throw error;
          return 0;
        } });
        sendRawWorkerRequest({ data: request } as unknown as MessageEvent<Record<string, unknown> & { type: string }>);
      } else postFailure("wasm-worker.step", error);
      assert.deepEqual(reads, { code: 1, message: 1 }, `listener=${listener} nativeError=${nativeError}`);
      assert.match(String((posted[0] as Record<string, unknown>).message), /原始故障/);
    }
  }
});


test("clone-safe 循环、Map、Set、BigInt 与原生 Error 详情仍通过主线程严格 parser", () => {
  posted.length = 0;
  const cycle: { tick: bigint; self?: unknown } = { tick: 7n };
  cycle.self = cycle;
  const cause = new Error("原始 cause", { cause: new Error("内层 cause") });
  const context = { cycle, map: new Map([["tick", 7n]]), set: new Set([7n]), exception: new DOMException("操作失败", "InvalidStateError") };
  postFailure("wasm-worker.step", { code: "STEP_FATAL", message: "原始故障", cause, context });
  const failure = parseWorkerFailure(posted[0]);
  assert.equal(failure.code, "STEP_FATAL");
  assert.ok(failure.cause instanceof Error);
  assert.equal(failure.cause.message, "原始 cause");
  const received = failure.context as typeof context;
  assert.equal(received.cycle.self, received.cycle);
  assert.equal(received.cycle.tick, 7n);
  assert.deepEqual(received.map, context.map);
  assert.deepEqual(received.set, context.set);
  assert.notEqual(received, context);
});

test("非结构化错误保留 prototype.toJSON、Date、URL 的原有诊断", () => {
  class ExternalFault { toJSON() { return { reason: "原始外部故障详情" }; } }
  for (const error of [new ExternalFault(), new Date("2030-01-02T00:00:00Z"), new URL("https://example.com/failure")]) {
    const classified = classifyWasmFailure(error);
    assert.equal(classified.kind, "unstructured");
    if (classified.kind === "unstructured") assert.equal(classified.description, describeWasmFailure(error));
  }
});

test("空 message 保留首读 code，message getter 抛错保留已读数值 code", () => {
  posted.length = 0;
  postFailure("wasm-worker.init", Object.assign(new Error(""), { code: "MODULE_INIT_FAILED" }));
  assert.deepEqual((posted[0] as Record<string, unknown>).cause, { code: "MODULE_INIT_FAILED", message: "" });
  posted.length = 0;
  postFailure("wasm-worker.init", Object.defineProperty({ code: 17 }, "message", { get() { throw new Error("message unreadable"); } }));
  assert.deepEqual((posted[0] as Record<string, unknown>).cause, { code: 17 });
  assert.match(String((posted[0] as Record<string, unknown>).message), /字段 message 失败.*message unreadable/);
});


test("context 回指原错误时 clone 再遍历 getter 的失败明确标注阶段并保留原故障", () => {
  posted.length = 0;
  let codeReads = 0;
  const error = Object.defineProperty({ message: "原始故障", context: undefined as unknown }, "code", {
    enumerable: true, get() {
      codeReads += 1;
      if (codeReads > 1) throw new Error("循环图回访 code getter");
      return "STEP_FATAL";
    },
  });
  error.context = error;
  assert.doesNotThrow(() => postFailure("wasm-worker.step", error));
  assert.equal(codeReads, 2);
  const failure = parseWorkerFailure(posted[0]);
  assert.equal(failure.code, "WASM_WORKER_PROTOCOL");
  assert.match(failure.message, /克隆结构化错误字段 context 失败.*循环图回访 code getter.*STEP_FATAL.*原始故障/);
  assert.deepEqual(failure.cause, { code: "STEP_FATAL", message: "原始故障" });
});

test("非结构化 Proxy 的核心 get trap 仅首读一次并复用原故障说明", () => {
  posted.length = 0;
  const reads = { code: 0, message: 0 };
  const error = new Proxy({ code: undefined, message: "original proxy failure" }, {
    get(target, key, receiver) {
      if (key === "code" || key === "message") {
        reads[key] += 1;
        if (reads[key] > 1) throw new Error(`${key} read twice`);
      }
      return Reflect.get(target, key, receiver);
    },
  });
  postFailure("wasm-worker.step", error);
  assert.deepEqual(reads, { code: 1, message: 1 });
  assert.match(String((posted[0] as Record<string, unknown>).message), /original proxy failure/);
});

test("原生 Date 的 custom toJSON 也复用首读 message 而不重复访问核心 getter", () => {
  let reads = 0;
  const error = Object.defineProperties(new Date("2030-01-02T00:00:00Z"), {
    message: { get() { reads += 1; if (reads > 1) throw new Error("message read twice"); return "original Date failure"; } },
    toJSON: { value(this: { message: string }) { return { message: this.message }; } },
  });
  const classified = classifyWasmFailure(error);
  assert.equal(reads, 1);
  assert.equal(classified.kind, "unstructured");
  if (classified.kind === "unstructured") assert.match(classified.description, /original Date failure/);
});

test("双发送失败的用户文本保留 engine 原始 where，详情先降级时也保留", () => {
  for (const badContext of [false, true]) {
    posted.length = 0;
    reportedErrors.length = 0;
    remainingPostFailures = 2;
    const error = { code: "STEP_FATAL", message: "原始故障", where: "engine::commit_tick" };
    if (badContext) Object.defineProperty(error, "context", { get() { throw new Error("context unreadable"); } });
    postFailure("wasm-worker.step", error);
    assert.equal(reportedErrors.length, 1);
    assert.match((reportedErrors[0] as Error).message, /engine::commit_tick.*port failure 1.*port failure 0/);
  }
});

test("原型检查失败的非结构化 Proxy 仍显示首读 message", () => {
  posted.length = 0;
  const error = new Proxy({ message: "original proxy failure" }, {
    getPrototypeOf() { throw new Error("prototype unreadable"); },
  });
  postFailure("wasm-worker.step", error);
  const message = String((posted[0] as Record<string, unknown>).message);
  assert.match(message, /检查错误对象失败.*prototype unreadable/);
  assert.match(message, /original proxy failure/);
});

test("畸形复杂 message 仍保留已读 primitive code/where，operationError 文本也可见", () => {
  for (const listener of [false, true]) {
    posted.length = 0;
    const error = Object.assign(new Error(), { code: "KNOWN", where: "bindings.initialize", message: { reason: "bad" } });
    if (listener) {
      let generationReads = 0;
      const request = Object.defineProperty({ type: "speedMetrics", requestId: 59 }, "generation", { get() {
        if (++generationReads === 1) throw error;
        return 0;
      } });
      sendRawWorkerRequest({ data: request } as unknown as MessageEvent<Record<string, unknown> & { type: string }>);
    } else postFailure("wasm-worker.step", error);
    const failure = posted[0] as Record<string, unknown>;
    assert.match(String(failure.message), /KNOWN.*bindings.initialize/);
    if (!listener) assert.deepEqual(failure.cause, { code: "KNOWN", where: "bindings.initialize" });
  }
});

test("非结构化原生标量说明保留已读 primitive message", () => {
  posted.length = 0;
  postFailure("wasm-worker.step", Object.assign(new Date("2030-01-02T00:00:00Z"), { message: "原故障必须可见" }));
  assert.match(String((posted[0] as Record<string, unknown>).message), /原故障必须可见/);
});
