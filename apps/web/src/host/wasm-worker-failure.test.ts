import assert from "node:assert/strict";
import test, { after } from "node:test";

const posted: unknown[] = [];
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
  for (const channel of channels) {
    channel.port1.close();
    channel.port2.close();
  }
});
Object.defineProperty(globalThis, "self", {
  configurable: true,
  value: {
    postMessage(message: unknown) {
      posted.push(message);
    },
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
    if (originalSelf === undefined) delete (globalThis as { self?: unknown }).self;
    else Object.defineProperty(globalThis, "self", originalSelf);
  }
})();

test("WASM Worker enables controlled stepping only for an explicit E2E build mode", () => {
  assert.equal(isE2EStepMode("e2e"), true);
  assert.equal(isE2EStepMode("production"), false);
  assert.equal(isE2EStepMode(undefined), false);
  assert.equal(isE2EStepMode({ MODE: "e2e" }), false);
});

test("Worker rejects stale save generation before looking up the current WASM candidate", () => {
  posted.length = 0;
  messageListener!({ data: { type: "save", generation: 1, requestId: 99, candidate: { seq: 42, settledDate: "2030-01-05" } } } as MessageEvent);
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

    assert.throws(() => postFailure("wasm-worker.init", error), reason);
    assert.deepEqual(posted, []);
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
    assert.throws(() => postFailure("wasm-worker.init", thrown), (actual) => actual === accessFailure);
    assert.deepEqual(posted, []);
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
    message: "读取结构化错误失败：getter exploded",
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
  assert.match(String(message), /^读取结构化错误失败：/);
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
  messageListener!({ data: request } as unknown as MessageEvent<Record<string, unknown> & { type: string }>);

  assert.deepEqual(posted, [{
    type: "failure",
    generation: 0,
    code: "STEP_FATAL",
    where: "wasm-worker.speedMetrics",
    message: "internal step failure: expected receipt hash differs",
  }]);
});
