import assert from "node:assert/strict";
import test from "node:test";
import { MessageChannel, receiveMessageOnPort, type MessagePort } from "node:worker_threads";
import { parseHostFailure } from "./protocol-failure.ts";
import { classifyWasmFailure, describeWasmFailure } from "./wasm-failure.ts";

const posted: unknown[] = [];
let failurePort: MessagePort | null = null;
let firstPostError: Error | null = null;
let messageListener: ((event: MessageEvent<Record<string, unknown> & { type: string }>) => void) | null = null;
const originalSelf = Object.getOwnPropertyDescriptor(globalThis, "self");
Object.defineProperty(globalThis, "self", {
  configurable: true,
  value: {
    postMessage(message: unknown) {
      if (firstPostError !== null) {
        const error = firstPostError;
        firstPostError = null;
        throw error;
      }
      if (failurePort !== null) failurePort.postMessage(message);
      else posted.push(message);
    },
    addEventListener(_type: "message", listener: typeof messageListener) {
      messageListener = listener;
    },
  },
});
const { isE2EStepMode, postFailure } = await import("./wasm-worker.ts");
if (originalSelf === undefined) delete (globalThis as { self?: unknown }).self;
else Object.defineProperty(globalThis, "self", originalSelf);

function receiveFailure(error: unknown): Record<string, unknown> {
  const { port1, port2 } = new MessageChannel();
  failurePort = port1;
  try {
    assert.doesNotThrow(() => postFailure("wasm-worker.step", error));
    const received = receiveMessageOnPort(port2);
    assert.notEqual(received, undefined, "真实 MessagePort 必须收到 failure，不能只验证数组 push");
    assert.equal(receiveMessageOnPort(port2), undefined, "每次错误只交付一条 failure");
    const { type, generation, ...details } = received!.message as Record<string, unknown>;
    assert.equal(type, "failure");
    assert.equal(generation, 0);
    return parseHostFailure(details) as unknown as Record<string, unknown>;
  } finally {
    failurePort = null;
    port1.close();
    port2.close();
  }
}

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

test("WASM Worker 将原生 Error 的可枚举 code、不可枚举 message 与真实 where 交付给严格 HostFailure parser", () => {
  posted.length = 0;
  const failure = {
    code: "ERR_MODULE_NOT_FOUND",
    where: "node:internal/modules/esm/resolve",
    message: "Cannot find module 'web_wasm.js' imported from wasm-worker.ts",
  };
  const error = Object.assign(new Error(failure.message), { code: failure.code, where: failure.where });
  assert.equal(Object.getOwnPropertyDescriptor(error, "code")?.enumerable, true);
  assert.equal(Object.getOwnPropertyDescriptor(error, "message")?.enumerable, false);

  assert.doesNotThrow(() => postFailure("wasm-worker.init", error));

  assert.deepEqual(posted, [{ type: "failure", generation: 0, ...failure }]);
  const { type, generation, ...details } = posted[0] as Record<string, unknown>;
  assert.equal(type, "failure");
  assert.equal(generation, 0);
  assert.deepEqual(parseHostFailure(details), failure);
});

test("WASM Worker 使用已分类核心字段，保留允许的不可枚举详情且不读取未知 getter", () => {
  posted.length = 0;
  const reads = { code: 0, message: 0 };
  const failure = {
    code: "STEP_FATAL", message: "receipt hash mismatch", where: "engine::commit_tick",
    cause: { receipt: "bad" }, context: { tick: 7 }, recoverable: false, recoveryActions: ["反馈错误详情"],
  };
  const error = Object.defineProperties({}, {
    code: { get() { assert.equal(++reads.code, 1); return failure.code; } },
    message: { get() { assert.equal(++reads.message, 1); return failure.message; } },
    ...Object.fromEntries(Object.entries(failure).filter(([key]) => key !== "code" && key !== "message")
      .map(([key, value]) => [key, { value }])),
    secret: { enumerable: true, get() { throw new Error("未知字段不得读取"); } },
  });

  assert.doesNotThrow(() => postFailure("wasm-worker.step", error));

  assert.deepEqual(posted, [{ type: "failure", generation: 0, ...failure }]);
  assert.deepEqual(reads, { code: 1, message: 1 });
});

test("WASM Worker 明确报告允许字段 getter 的来源并保留原始 code/message，不在错误投影时再次抛错", () => {
  for (const key of ["where", "cause", "context", "recoverable", "recoveryActions"]) {
    posted.length = 0;
    const failure = { code: "STEP_FATAL", message: "receipt hash mismatch" };
    const error = Object.defineProperty({ ...failure }, key, {
      enumerable: true,
      get() { throw new Error(`${key} getter exploded`); },
    });

    assert.doesNotThrow(() => postFailure("wasm-worker.step", error), key);

    assert.deepEqual(posted, [{
      type: "failure", generation: 0, code: "WASM_WORKER_PROTOCOL", where: "wasm-worker.step",
      message: `读取结构化错误字段 ${key} 失败：${key} getter exploded`, cause: failure,
    }]);
  }
});

test("WASM Worker 不把畸形 structured 字段吞掉，也不向严格 parser 回填空 message", () => {
  const failure = { code: "STEP_FATAL", where: "engine.step", message: "receipt hash mismatch" };
  for (const [key, value] of [
    ["code", " "], ["where", ""], ["message", " "], ["recoverable", "yes"], ["recoveryActions", [17]],
  ] as const) {
    const malformed = { ...failure, [key]: value };
    assert.throws(() => parseHostFailure(malformed), {
      code: "PROTOCOL_MALFORMED", where: `HostFailure.${key}`,
    });
    const reported = receiveFailure(malformed);
    assert.equal(reported.code, "WASM_WORKER_PROTOCOL");
    assert.equal(reported.where, "wasm-worker.step");
    assert.match(reported.message as string, new RegExp(`校验结构化错误详情失败.*HostFailure\\.${key}`));
    assert.deepEqual(reported.cause, { code: malformed.code, message: malformed.message, where: malformed.where });
  }
});

for (const key of ["0", "map"] as const) {
  test(`WASM Worker 保护不可枚举 recoveryActions 的 ${key} getter 并通过真实 MessagePort 报告原错误`, () => {
    const original = { code: "STEP_FATAL", message: "receipt hash mismatch", where: "engine::commit_tick" };
    const actions = Object.defineProperty(["反馈错误详情"], key, {
      get() { throw new Error(`recoveryActions ${key} getter exploded`); },
    });
    const error = Object.defineProperty({ ...original }, "recoveryActions", { value: actions });
    assert.equal(Object.getOwnPropertyDescriptor(error, "message")?.enumerable, true);
    assert.equal(Object.getOwnPropertyDescriptor(error, "recoveryActions")?.enumerable, false);
    const reported = receiveFailure(error);
    assert.equal(reported.code, "WASM_WORKER_PROTOCOL");
    assert.equal(reported.where, "wasm-worker.step");
    assert.match(reported.message as string, new RegExp(`读取结构化错误详情失败.*recoveryActions ${key} getter exploded`));
    assert.deepEqual(reported.cause, original);
  });
}

test("WASM Worker 将 ErrorOptions 的不可枚举函数 cause 发送失败显式交付给真实 MessagePort", () => {
  const original = { code: "STEP_FATAL", message: "receipt hash mismatch", where: "engine::commit_tick" };
  const error = Object.assign(new Error(original.message, { cause: function uncloneableCause() {} }), {
    code: original.code, where: original.where,
  });
  Object.defineProperty(error, "message", { enumerable: true });
  assert.equal(Object.getOwnPropertyDescriptor(error, "message")?.enumerable, true);
  assert.equal(Object.getOwnPropertyDescriptor(error, "cause")?.enumerable, false);
  const reported = receiveFailure(error);
  assert.equal(reported.code, "WASM_WORKER_PROTOCOL");
  assert.equal(reported.where, "wasm-worker.step");
  assert.match(reported.message as string, /发送结构化错误详情失败.*could not be cloned/);
  assert.deepEqual(reported.cause, original);
});

for (const key of ["cause", "context"] as const) {
  test(`WASM Worker 保护不可枚举 ${key} 的嵌套 getter 并通过真实 MessagePort 报告原错误`, () => {
    const original = { code: "STEP_FATAL", message: "receipt hash mismatch", where: "engine::commit_tick" };
    const details = { get nested() { throw new Error(`${key} nested getter exploded`); } };
    const error = Object.defineProperty({ ...original }, key, { value: details });
    assert.equal(Object.getOwnPropertyDescriptor(error, "message")?.enumerable, true);
    assert.equal(Object.getOwnPropertyDescriptor(error, key)?.enumerable, false);
    const reported = receiveFailure(error);
    assert.equal(reported.code, "WASM_WORKER_PROTOCOL");
    assert.equal(reported.where, "wasm-worker.step");
    assert.match(reported.message as string, new RegExp(`发送结构化错误详情失败.*${key} nested getter exploded`));
    assert.deepEqual(reported.cause, original);
  });
}

test("WASM Worker 发送 clone-safe 详情快照，不在 postMessage 时再次访问嵌套 getter", () => {
  let reads = 0;
  const original = { code: "STEP_FATAL", message: "receipt hash mismatch", where: "engine::commit_tick" };
  const context = {
    get tick() {
      assert.equal(++reads, 1, "快照生成后不得重复访问原始详情");
      return 7;
    },
  };
  const error = Object.defineProperty({ ...original }, "context", { value: context });
  assert.deepEqual(receiveFailure(error), { ...original, context: { tick: 7 } });
  assert.equal(reads, 1);
});

test("WASM Worker 将首次 postMessage 异常明确报告，并通过真实 MessagePort 保存原始核心详情", () => {
  const original = { code: "STEP_FATAL", message: "receipt hash mismatch", where: "engine::commit_tick" };
  firstPostError = new Error("port send failed");
  const reported = receiveFailure(original);
  assert.equal(reported.code, "WASM_WORKER_PROTOCOL");
  assert.equal(reported.where, "wasm-worker.step");
  assert.match(reported.message as string, /发送结构化错误详情失败.*port send failed/);
  assert.deepEqual(reported.cause, original);
});

for (const key of ["recoverable", "recoveryActions", "cause", "context"] as const) {
  test(`WASM Worker 保护不可枚举 ${key} 中的恶意 Proxy，不让错误报告器再次抛错`, () => {
    const original = { code: "STEP_FATAL", message: "receipt hash mismatch", where: "engine::commit_tick" };
    const { proxy, revoke } = Proxy.revocable([], {});
    revoke();
    const error = Object.defineProperty({ ...original }, key, { value: proxy });
    const reported = receiveFailure(error);
    assert.equal(reported.code, "WASM_WORKER_PROTOCOL");
    assert.equal(reported.where, "wasm-worker.step");
    assert.match(reported.message as string, /结构化错误详情失败/);
    assert.deepEqual(reported.cause, original);
  });
}

test("WASM Worker 详情字段的 Proxy 描述符失败仍保存已安全读取的原始 where", () => {
  const original = { code: "STEP_FATAL", message: "receipt hash mismatch", where: "engine::commit_tick" };
  const error = new Proxy({ ...original }, {
    getOwnPropertyDescriptor(target, key) {
      if (key === "cause") throw new Error("cause descriptor exploded");
      return Reflect.getOwnPropertyDescriptor(target, key);
    },
  });
  const reported = receiveFailure(error);
  assert.equal(reported.code, "WASM_WORKER_PROTOCOL");
  assert.equal(reported.where, "wasm-worker.step");
  assert.match(reported.message as string, /读取结构化错误字段 cause 失败.*cause descriptor exploded/);
  assert.deepEqual(reported.cause, original);
});

test("WASM Worker 不把无 code 原生 Error 的空 message 当作可用错误交付", () => {
  const reported = receiveFailure(new Error(""));
  assert.equal(reported.code, "WASM_WORKER_PROTOCOL");
  assert.equal(reported.where, "wasm-worker.step");
  assert.match(reported.message as string, /非结构化错误.*非空.*message/);
  assert.deepEqual(reported.cause, { message: "" });
});

test("WASM Worker 对没有 code 的原生 Error 和字符串保持显式 unstructured 错误路径", () => {
  for (const error of [new Error("native failure without code"), "plain thrown failure"]) {
    posted.length = 0;
    postFailure("wasm-worker.step", error);
    assert.deepEqual(posted, [{
      type: "failure", generation: 0, code: "WASM_WORKER_PROTOCOL", where: "wasm-worker.step",
      message: error instanceof Error ? error.message : error,
    }]);
  }
});

test("wasm-failure 分类直接读取原生 Error 核心字段，未知对象与恶意访问仍明确区分", () => {
  const error = Object.assign(new Error("module missing"), { code: "ERR_MODULE_NOT_FOUND" });
  assert.deepEqual(classifyWasmFailure(error), { kind: "structured", code: error.code, message: error.message });
  assert.deepEqual(classifyWasmFailure(new Error("no code")), { kind: "unstructured" });
  assert.deepEqual(classifyWasmFailure({ reason: "bad boundary" }), { kind: "unstructured" });
  assert.deepEqual(classifyWasmFailure([]), { kind: "unstructured" });
  assert.deepEqual(classifyWasmFailure(Object.defineProperty({}, "message", {
    get() { throw new Error("message getter exploded"); },
  })), { kind: "access-error", reason: "message getter exploded" });
  assert.equal(describeWasmFailure(error), error.message);
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

test("Worker 消息 listener 将原生带 code 错误的原 message 与请求来源完整发送，而不是二次 ProtocolError", () => {
  posted.length = 0;
  const error = Object.assign(new Error("Cannot find module 'web_wasm.js'"), { code: "ERR_MODULE_NOT_FOUND" });
  const request = Object.defineProperty({ type: "speedMetrics", requestId: 20 }, "generation", {
    get() { throw error; },
  });

  assert.notEqual(messageListener, null);
  messageListener!({ data: request } as unknown as MessageEvent<Record<string, unknown> & { type: string }>);

  assert.deepEqual(posted, [{
    type: "failure", generation: 0, code: error.code, message: error.message, where: "wasm-worker.speedMetrics",
  }]);
});

test("S2 查询消息先做请求字段校验：无效 requestId 与公司身份走 operationError 显式拒绝", () => {
  posted.length = 0;
  messageListener!({ data: { type: "rightsRejections", generation: 0, requestId: -1 } } as MessageEvent);
  assert.deepEqual(posted, [{ type: "operationError", requestId: -1, generation: 0, message: "配股认购拒绝回执请求 ID 无效" }]);
  posted.length = 0;
  messageListener!({ data: { type: "preferenceRejections", generation: 0, requestId: 2, company: " " } } as MessageEvent);
  assert.deepEqual(posted, [{ type: "operationError", requestId: 2, generation: 0, message: "偏好台账公司身份必须是非空且不超过 64 字符的字符串" }]);
  posted.length = 0;
  messageListener!({ data: { type: "preferenceRejections", generation: 0, requestId: 3, company: 600101 } } as MessageEvent);
  assert.deepEqual(posted, [{ type: "operationError", requestId: 3, generation: 0, message: "偏好台账公司身份必须是非空且不超过 64 字符的字符串" }]);
});

test("G 批认购提交消息先做请求字段校验：无效事件身份与股数走 operationError 显式拒绝", () => {
  posted.length = 0;
  messageListener!({ data: { type: "subscribeRightsOffering", generation: 0, requestId: -1, eventId: "rights-event", shares: "41" } } as MessageEvent);
  assert.deepEqual(posted, [{ type: "operationError", requestId: -1, generation: 0, message: "配股认购提交请求 ID 无效" }]);
  posted.length = 0;
  messageListener!({ data: { type: "subscribeRightsOffering", generation: 0, requestId: 5, eventId: " ", shares: "41" } } as MessageEvent);
  assert.deepEqual(posted, [{ type: "operationError", requestId: 5, generation: 0, message: "配股认购事件身份必须是非空且不超过 128 字符的字符串" }]);
  posted.length = 0;
  messageListener!({ data: { type: "subscribeRightsOffering", generation: 0, requestId: 6, eventId: "rights-event", shares: "0" } } as MessageEvent);
  assert.deepEqual(posted, [{ type: "operationError", requestId: 6, generation: 0, message: "配股认购股数必须是正的规范 u64 十进制字符串" }]);
  posted.length = 0;
  messageListener!({ data: { type: "subscribeRightsOffering", generation: 0, requestId: 7, eventId: "rights-event", shares: 41 } } as MessageEvent);
  assert.deepEqual(posted, [{ type: "operationError", requestId: 7, generation: 0, message: "配股认购股数必须是正的规范 u64 十进制字符串" }]);
});
