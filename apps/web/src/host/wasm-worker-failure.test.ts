import assert from "node:assert/strict";
import test from "node:test";
import { MessageChannel, receiveMessageOnPort, type MessagePort } from "node:worker_threads";
import { parseHostFailure } from "./protocol-failure.ts";
import { classifyWasmFailure, describeWasmFailure } from "./wasm-failure.ts";
import { WasmTickLoop } from "./wasm-tick-loop.ts";
import { frame, snapshot, tickBatch } from "./protocol-test-fixtures.ts";

const posted: unknown[] = [];
let failurePort: MessagePort | null = null;
let firstPostError: Error | null = null;
// 模拟死端口，所有发送都失败。
let persistentPostError: Error | null = null;
let messageListener: ((event: MessageEvent<Record<string, unknown> & { type: string }>) => void) | null = null;
const originalSelf = Object.getOwnPropertyDescriptor(globalThis, "self");
Object.defineProperty(globalThis, "self", {
  configurable: true,
  value: {
    postMessage(message: unknown) {
      if (persistentPostError !== null) throw persistentPostError;
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
    let reads = 0;
    const actions = Object.defineProperty(["反馈错误详情"], key, {
      get() { reads++; throw new Error(`recoveryActions ${key} getter exploded`); },
    });
    const error = Object.defineProperty({ ...original }, "recoveryActions", { value: actions });
    assert.equal(Object.getOwnPropertyDescriptor(error, "message")?.enumerable, true);
    assert.equal(Object.getOwnPropertyDescriptor(error, "recoveryActions")?.enumerable, false);
    const reported = receiveFailure(error);
    if (key === "map") {
      assert.equal(reads, 0, "原数组自有方法不是诊断元素，不能读取");
      assert.deepEqual(reported, { ...original, recoveryActions: ["反馈错误详情"] });
    } else {
      assert.equal(reads, 1);
      assert.equal(reported.code, "WASM_WORKER_PROTOCOL");
      assert.equal(reported.where, "wasm-worker.step");
      assert.match(reported.message as string, new RegExp(`读取结构化错误详情失败.*recoveryActions ${key} getter exploded`));
      assert.deepEqual(reported.cause, original);
    }
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
  assert.deepEqual(classifyWasmFailure(new Error("no code")), { kind: "unstructured", description: "no code" });
  assert.deepEqual(classifyWasmFailure({ reason: "bad boundary" }), { kind: "unstructured", description: '非标准错误对象：{"reason":"bad boundary"}' });
  assert.deepEqual(classifyWasmFailure([]), { kind: "unstructured", description: "非标准错误对象：[]" });
  assert.deepEqual(classifyWasmFailure(Object.assign(new Error(""), { code: "ERR_KEEP_FIRST_READ" })), { kind: "unstructured", description: "", code: "ERR_KEEP_FIRST_READ" });
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

test("F1 recoverable 拒绝走 operationError 通道时传递已分类快照 message，不二次访问 raw getter", () => {
  posted.length = 0;
  const reads = { message: 0 };
  const error = Object.defineProperties({}, {
    code: { value: "REPORT_CORRECTION_REJECTED", enumerable: true },
    recoverable: { value: true, enumerable: true },
    message: {
      enumerable: true,
      get() {
        reads.message += 1;
        if (reads.message > 1) throw new Error("message getter 只允许读取一次");
        return "现金不足，无法受理财报更正";
      },
    },
  });
  const request = Object.defineProperty(
    { type: "subscribeRightsOffering", requestId: 21, generation: 0, eventId: "rights-event" },
    "shares",
    { get() { throw error; } },
  );

  messageListener!({ data: request } as unknown as MessageEvent<Record<string, unknown> & { type: string }>);

  assert.equal(reads.message, 1, "raw message getter 只能读取一次");
  assert.deepEqual(posted[0], { type: "operationError", requestId: 21, generation: 0, message: "现金不足，无法受理财报更正" });
  assert.equal((posted[1] as Record<string, unknown>).type, "failure");
});

test("F2 可恢复拒绝的不可克隆 cause 不销毁会话：顶层保留 code+recoverable+recoveryActions，详情降级入 cause（clone 臂）", () => {
  const original = { code: "REPORT_CORRECTION_REJECTED", message: "现金不足，无法受理财报更正", where: "engine::report_corrections" };
  const error = Object.assign({}, original, {
    cause: function uncloneableCause() {},
    recoverable: true,
    recoveryActions: ["核对现金余额后重新提交"],
  });

  const reported = receiveFailure(error);

  // worker-host notifyFailure 按 code===REPORT_CORRECTION_REJECTED && recoverable===true 分流保会话。
  assert.equal(reported.code, "REPORT_CORRECTION_REJECTED");
  assert.equal(reported.recoverable, true);
  assert.equal(reported.message, original.message);
  assert.equal(reported.where, original.where);
  assert.deepEqual(reported.recoveryActions, ["核对现金余额后重新提交"]);
  const cause = reported.cause as Record<string, unknown>;
  assert.equal(cause.code, "WASM_WORKER_PROTOCOL");
  assert.match(String(cause.message), /发送结构化错误详情失败/);
  assert.deepEqual(cause.cause, original);
});

test("F2 可恢复+不可克隆详情经消息 listener 全链路：operationError 与 failure 通道都保留真实拒绝与会话（listener 臂）", () => {
  posted.length = 0;
  const original = { code: "REPORT_CORRECTION_REJECTED", message: "现金不足，无法受理财报更正", where: "engine::report_corrections" };
  const error = Object.assign({}, original, {
    cause: function uncloneableCause() {},
    recoverable: true,
    recoveryActions: ["核对现金余额后重新提交"],
  });
  const request = Object.defineProperty(
    { type: "subscribeRightsOffering", requestId: 22, generation: 0, eventId: "rights-event" },
    "shares",
    { get() { throw error; } },
  );

  messageListener!({ data: request } as unknown as MessageEvent<Record<string, unknown> & { type: string }>);

  assert.deepEqual(posted[0], { type: "operationError", requestId: 22, generation: 0, message: original.message });
  const failure = posted[1] as Record<string, unknown>;
  assert.equal(failure.type, "failure");
  assert.equal(failure.code, "REPORT_CORRECTION_REJECTED");
  assert.equal(failure.recoverable, true);
  assert.equal(failure.message, original.message);
  assert.equal(failure.where, original.where);
  assert.deepEqual(failure.recoveryActions, ["核对现金余额后重新提交"]);
  const cause = failure.cause as Record<string, unknown>;
  assert.equal(cause.code, "WASM_WORKER_PROTOCOL");
  assert.match(String(cause.message), /发送结构化错误详情失败/);
  assert.deepEqual(cause.cause, original);
});

test("F3 守卫分支 cause 回填已安全读取的 code 与 where，不只保留空 message", () => {
  posted.length = 0;
  const error = Object.assign(new Error(""), { code: "ERR_MODULE_NOT_FOUND", where: "node:internal/modules/esm/resolve" });

  postFailure("wasm-worker.init", error);

  assert.deepEqual(posted, [{
    type: "failure",
    generation: 0,
    code: "WASM_WORKER_PROTOCOL",
    where: "wasm-worker.init",
    message: "非结构化错误没有可读的非空 message",
    cause: { code: "ERR_MODULE_NOT_FOUND", where: "node:internal/modules/esm/resolve", message: "" },
  }]);
});

test("F4 where 为可克隆非 string（42/null）时 fallback cause 保留原值", () => {
  for (const whereValue of [42, null] as const) {
    const reported = receiveFailure({ code: "STEP_FATAL", message: "receipt hash mismatch", where: whereValue });
    assert.equal(reported.code, "WASM_WORKER_PROTOCOL");
    assert.equal(reported.where, "wasm-worker.step");
    assert.match(String(reported.message), /校验结构化错误详情失败.*HostFailure\.where/);
    assert.deepEqual(reported.cause, { code: "STEP_FATAL", message: "receipt hash mismatch", where: whereValue });
  }
});

test("F5 端口级双失败时兜底发送不再向上抛 unhandled rejection，Worker 内显式登记", () => {
  posted.length = 0;
  persistentPostError = new Error("port dead");
  const logged: unknown[][] = [];
  const originalConsoleError = console.error;
  console.error = (...args: unknown[]) => { logged.push(args); };
  try {
    assert.doesNotThrow(() => postFailure("wasm-worker.step", { code: "STEP_FATAL", message: "receipt hash mismatch" }));
  } finally {
    console.error = originalConsoleError;
    persistentPostError = null;
  }
  assert.deepEqual(posted, [], "双失败时没有任何消息离开 Worker");
  assert.equal(logged.length, 1, "最后手段 console.error 恰好登记一次");
  assert.match(String(logged[0][0]), /WASM Worker 错误报告通道双失败/);
  assert.equal((logged[0][1] as Record<string, unknown>).code, "STEP_FATAL");
});

test("F2 边界：recoverable 尚未安全读到（前置详情 getter 抛出）时不臆断保会话，显式降级并保留已知快照", () => {
  const original = { code: "REPORT_CORRECTION_REJECTED", message: "现金不足，无法受理财报更正", where: "engine::report_corrections" };
  const error = Object.defineProperty(Object.assign({}, original, {
    recoverable: true,
    recoveryActions: ["核对现金余额后重新提交"],
  }), "context", {
    enumerable: true,
    get() { throw new Error("context getter exploded"); },
  });

  const reported = receiveFailure(error);

  // context 先抛错时 recoverability 尚未知，不得臆断保会话。
  assert.equal(reported.code, "WASM_WORKER_PROTOCOL");
  assert.equal(reported.recoverable, undefined);
  assert.match(String(reported.message), /读取结构化错误字段 context 失败/);
  assert.deepEqual(reported.cause, original);
});

test("F2 边界：真实 where 为空串导致校验失败时，保会话顶层回退到 Worker where 而非交付非法字段", () => {
  const error = {
    code: "REPORT_CORRECTION_REJECTED",
    message: "现金不足，无法受理财报更正",
    where: "",
    recoverable: true,
    recoveryActions: ["核对现金余额后重新提交"],
  };

  const reported = receiveFailure(error);

  assert.equal(reported.code, "REPORT_CORRECTION_REJECTED");
  assert.equal(reported.recoverable, true);
  assert.equal(reported.where, "wasm-worker.step", "空串 where 不可交付，回退 Worker where");
  const cause = reported.cause as Record<string, unknown>;
  assert.match(String(cause.message), /校验结构化错误详情失败.*HostFailure\.where/);
});

test("F2 边界：recoveryActions 含非字符串时不进入顶层，但 recoverable 保会话分流不丢", () => {
  const error = {
    code: "REPORT_CORRECTION_REJECTED",
    message: "现金不足，无法受理财报更正",
    where: "engine::report_corrections",
    recoverable: true,
    recoveryActions: [17],
  };

  const reported = receiveFailure(error);

  assert.equal(reported.code, "REPORT_CORRECTION_REJECTED");
  assert.equal(reported.recoverable, true);
  assert.equal(reported.recoveryActions, undefined, "畸形建议列表不进入顶层，避免主线程 parser 拒绝");
  const cause = reported.cause as Record<string, unknown>;
  assert.match(String(cause.message), /校验结构化错误详情失败.*HostFailure\.recoveryActions/);
});

test("F2 边界：code 为纯空白时不可作为可交付顶层 code，不保会话而显式降级", () => {
  const error = {
    code: " ",
    message: "现金不足，无法受理财报更正",
    recoverable: true,
  };

  const reported = receiveFailure(error);

  assert.equal(reported.code, "WASM_WORKER_PROTOCOL");
  assert.equal(reported.recoverable, undefined);
  assert.deepEqual(reported.cause, { code: " ", message: "现金不足，无法受理财报更正" });
});

test("G1 对抗性 recoveryActions getter 校验读取与克隆读取返回不同值时，不得用校验结果为克隆值背书", () => {
  // getter 后续读取变值不能让未校验的建议毁掉可恢复会话。
  const original = { code: "REPORT_CORRECTION_REJECTED", message: "现金不足，无法受理财报更正", where: "engine::report_corrections" };
  let reads = 0;
  const actions = Object.defineProperty([""], "0", {
    enumerable: true,
    configurable: true,
    get() {
      reads += 1;
      return reads <= 2 ? "核对现金余额后重新提交" : 17;
    },
  });
  const error = Object.assign({}, original, {
    cause: function uncloneableCause() {},
    recoverable: true,
    recoveryActions: actions,
  });

  const reported = receiveFailure(error);

  assert.equal(reported.code, "REPORT_CORRECTION_REJECTED");
  assert.equal(reported.recoverable, true, "畸形建议列表不得销毁可恢复会话");
  assert.notDeepEqual(reported.recoveryActions, [17], "克隆快照中的畸形值不得逃逸到主线程");
  // 第二读生成的实际快照合法时才保留，不能拿此前读值背书。
  assert.deepEqual(reported.recoveryActions, ["核对现金余额后重新提交"]);
  const cause = reported.cause as Record<string, unknown>;
  assert.match(String(cause.message), /发送结构化错误详情失败/);
  assert.match(String(cause.message), /降级字段：cause/, "仅克隆失败的字段（cause 函数）降级并列明");
  assert.deepEqual(cause.cause, original);
});

test("G2 显式非法 where:undefined 与缺省 where 区分：克隆成功的 undefined 保留为 cause 的 own 属性", () => {
  const reported = receiveFailure({ code: "STEP_FATAL", message: "receipt hash mismatch", where: undefined });
  assert.equal(reported.code, "WASM_WORKER_PROTOCOL");
  assert.equal(reported.where, "wasm-worker.step");
  assert.match(String(reported.message), /校验结构化错误详情失败.*HostFailure\.where/);
  const cause = reported.cause as Record<string, unknown>;
  assert.ok(Object.hasOwn(cause, "where"), "克隆成功的 where:undefined 必须保留 own 属性，与缺省 where 区分");
  assert.deepEqual(cause, { code: "STEP_FATAL", message: "receipt hash mismatch", where: undefined });
});

test("G3 死端口下 operationError 发送受保护：不产生 unhandled rejection，console 显式登记且后续 failure 兜底仍执行", async () => {
  posted.length = 0;
  persistentPostError = new Error("port dead");
  const unhandled: unknown[] = [];
  const onUnhandled = (reason: unknown) => { unhandled.push(reason); };
  process.on("unhandledRejection", onUnhandled);
  const logged: unknown[][] = [];
  const originalConsoleError = console.error;
  console.error = (...args: unknown[]) => { logged.push(args); };
  try {
    // operationError 先发送，不能因死端口阻断后续 failure 诊断。
    const recoverable = Object.defineProperties({}, {
      code: { value: "REPORT_CORRECTION_REJECTED", enumerable: true },
      recoverable: { value: true, enumerable: true },
      message: { value: "现金不足，无法受理财报更正", enumerable: true },
    });
    const request1 = Object.defineProperty(
      { type: "subscribeRightsOffering", requestId: 41, generation: 0, eventId: "rights-event" },
      "shares",
      { get() { throw recoverable; } },
    );
    messageListener!({ data: request1 } as unknown as MessageEvent<Record<string, unknown> & { type: string }>);
    // 场景 2：unstructured + requestId 分支（只有 operationError 一条发送）。
    messageListener!({ data: { type: "save", generation: 1, requestId: 42, candidate: { seq: 42, settledDate: "2030-01-05" } } } as MessageEvent);
    await new Promise((resolve) => setImmediate(resolve));
    await new Promise((resolve) => setImmediate(resolve));
  } finally {
    console.error = originalConsoleError;
    persistentPostError = null;
    process.off("unhandledRejection", onUnhandled);
  }
  assert.deepEqual(unhandled, [], "operationError 发送失败不得变成 unhandled rejection");
  assert.deepEqual(posted, [], "死端口没有任何消息离开 Worker");
  assert.equal(logged.filter((args) => /operationError 通道发送失败/.test(String(args[0]))).length, 2, "两个场景的 operationError 发送失败都必须显式 console 登记");
  assert.equal(logged.filter((args) => /错误报告通道双失败/.test(String(args[0]))).length, 1, "场景 1 的 failure 通道 F5 兜底登记仍执行");
});

test("G4 F2 fallback 保留已验证可克隆诊断字段：context 与 recoveryActions:null 标记不丢，仅克隆失败字段降级并列明", () => {
  const original = { code: "REPORT_CORRECTION_REJECTED", message: "现金不足，无法受理财报更正", where: "engine::report_corrections" };
  const error = Object.assign({}, original, {
    cause: function uncloneableCause() {},
    context: { tick: 7 },
    recoverable: true,
    recoveryActions: null,
  });

  const reported = receiveFailure(error);

  assert.equal(reported.code, "REPORT_CORRECTION_REJECTED");
  assert.equal(reported.recoverable, true);
  assert.deepEqual(reported.context, { tick: 7 }, "可克隆 context 必须保留在顶层供 buildErrorFeedback 显示");
  assert.ok(Object.hasOwn(reported, "recoveryActions"), "recoveryActions:null「未知」标记必须保留，不得与缺省混淆");
  assert.equal(reported.recoveryActions, null);
  const cause = reported.cause as Record<string, unknown>;
  assert.match(String(cause.message), /发送结构化错误详情失败/);
  assert.match(String(cause.message), /降级字段：cause/, "仅克隆失败的字段降级并在降级说明中列明");
  assert.deepEqual(cause.cause, original);
});

test("G4 F2 fallback 原始 cause 可克隆时并入降级 cause 链：校验触发的 fallback 同样不丢已验证诊断", () => {
  const original = { code: "REPORT_CORRECTION_REJECTED", message: "现金不足，无法受理财报更正", where: "engine::report_corrections" };
  const error = Object.assign({}, original, {
    cause: { receipt: "bad" },
    context: { tick: 7 },
    recoverable: true,
    recoveryActions: [17],
  });

  const reported = receiveFailure(error);

  assert.equal(reported.code, "REPORT_CORRECTION_REJECTED");
  assert.equal(reported.recoverable, true);
  assert.deepEqual(reported.context, { tick: 7 }, "可克隆 context 在校验失败触发的 fallback 中同样保留");
  assert.equal(reported.recoveryActions, undefined, "畸形建议列表不进入顶层");
  const cause = reported.cause as Record<string, unknown>;
  assert.match(String(cause.message), /校验结构化错误详情失败.*HostFailure\.recoveryActions/);
  assert.match(String(cause.message), /降级字段：recoveryActions/, "仅克隆失败/未通过校验的字段降级并列明");
  assert.deepEqual(cause.cause, { ...original, cause: { receipt: "bad" } }, "可克隆的原始 cause 并入降级 cause 链，不丢");
});

test("G5 F3 回填复用 code 首读结果：对抗 accessor 首读合法、再读抛错时 cause 仍含 code", () => {
  posted.length = 0;
  let reads = 0;
  const error = Object.defineProperty(new Error(""), "code", {
    enumerable: true,
    get() {
      reads += 1;
      if (reads === 1) return "ERR_MODULE_NOT_FOUND";
      throw new Error("code getter 只允许读取一次");
    },
  });

  postFailure("wasm-worker.init", error);

  assert.equal(reads, 1, "code 核心字段只读一次");
  assert.deepEqual(posted, [{
    type: "failure",
    generation: 0,
    code: "WASM_WORKER_PROTOCOL",
    where: "wasm-worker.init",
    message: "非结构化错误没有可读的非空 message",
    cause: { code: "ERR_MODULE_NOT_FOUND", message: "" },
  }]);
});

for (const invalidElements of [false, true]) {
  test(`recoveryActions 成功路径不调用自有 map，非法原元素=${invalidElements} 不能被伪造为合法建议`, () => {
    let calls = 0;
    const actions = Object.assign([invalidElements ? 17 : "重试"], {
      map() { calls++; return Object.assign(["重试"], { map: 17 }); },
    });
    const reported = receiveFailure({
      code: "REPORT_CORRECTION_REJECTED", message: "更正被拒绝", recoverable: true,
      cause: { receipt: "可克隆详情" }, recoveryActions: actions,
    });
    assert.equal(calls, 0, "不能调用原数组方法来校验或投影建议");
    assert.equal(reported.code, "REPORT_CORRECTION_REJECTED");
    assert.equal(reported.recoverable, true);
    if (invalidElements) {
      assert.equal(reported.recoveryActions, undefined);
      assert.match(String((reported.cause as Record<string, unknown>).message), /HostFailure\.recoveryActions.*降级字段：recoveryActions/);
    } else {
      assert.deepEqual(reported.recoveryActions, ["重试"]);
      assert.deepEqual(reported.cause, { receipt: "可克隆详情" });
    }
  });
}

test("非结构化错误的私有字段 toJSON 和 enumerable getter 保留原 receiver 与核心首读", () => {
  class PrivateFailure {
    #reason = "原错";
    readonly reads = { code: 0, message: 0 };
    constructor() {
      Object.defineProperty(this, "detail", { enumerable: true, get() { return this.#reason; } });
    }
    get code() { assert.equal(++this.reads.code, 1); return "ERR_PRIVATE_FAILURE"; }
    get message() { assert.equal(++this.reads.message, 1); return ""; }
    toJSON() { return { reason: this.#reason, detail: (this as unknown as { detail: string }).detail }; }
  }
  const error = new PrivateFailure();
  const reported = receiveFailure(error);
  assert.equal(reported.message, '非标准错误对象：{"reason":"原错","detail":"原错"}');
  assert.deepEqual(error.reads, { code: 1, message: 1 });

  class PrivateGetterFailure {
    #reason = "原getter";
    readReason() { return this.#reason; }
    constructor() { Object.defineProperty(this, "detail", { enumerable: true, get: this.readReason }); }
  }
  assert.equal(receiveFailure(new PrivateGetterFailure()).message, '非标准错误对象：{"detail":"原getter"}');
});

for (const key of ["map", "every"] as const) {
  test(`recoveryActions 自有 ${key} 不随实际快照交付，主线程仍可解析普通数组`, () => {
    const actions = Object.assign(["核对现金余额后重新提交"], { [key]: 17, extra: "不得携带" });
    actions[Symbol.iterator] = () => { throw new Error("不得调用原数组 iterator"); };
    const reported = receiveFailure({
      code: "REPORT_CORRECTION_REJECTED", message: "更正被拒绝", recoverable: true,
      cause: function uncloneableCause() {}, recoveryActions: actions,
    });
    assert.deepEqual(reported.recoveryActions, ["核对现金余额后重新提交"]);
    assert.deepEqual(Reflect.ownKeys(reported.recoveryActions as object), ["0", "length"]);
    assert.equal(reported.code, "REPORT_CORRECTION_REJECTED");
    assert.equal(reported.recoverable, true);
  });
}

test("recoveryActions 稀疏快照拒绝空洞并显式登记降级，不把空洞送给主线程", () => {
  const actions = ["核对现金余额后重新提交"];
  actions.length = 2;
  const reported = receiveFailure({
    code: "REPORT_CORRECTION_REJECTED", message: "更正被拒绝", recoverable: true,
    cause: function uncloneableCause() {}, recoveryActions: actions,
  });
  assert.equal(reported.recoveryActions, undefined);
  assert.equal(reported.recoverable, true);
  assert.match(String((reported.cause as Record<string, unknown>).message), /降级字段：recoveryActions、cause/);
});

test("recoveryActions 索引 getter 第二读变为非法值时只按实际克隆快照降级", () => {
  let reads = 0;
  const actions = Object.defineProperty([""], "0", {
    enumerable: true,
    get() { return ++reads === 1 ? "核对现金余额后重新提交" : 17; },
  });
  const reported = receiveFailure({
    code: "REPORT_CORRECTION_REJECTED", message: "更正被拒绝", recoverable: true,
    cause: function uncloneableCause() {}, recoveryActions: actions,
  });
  assert.equal(reads, 2);
  assert.equal(reported.recoveryActions, undefined);
  assert.equal(reported.recoverable, true);
  assert.match(String((reported.cause as Record<string, unknown>).message), /降级字段：recoveryActions、cause/);
});

for (const requestId of [undefined, 93]) {
  test(`listener requestId=${requestId} 复用首读 code 和空 message，不重复访问核心 getter`, () => {
    posted.length = 0;
    const reads = { code: 0, message: 0 };
    const error = Object.defineProperties(new Error(""), {
      code: { get() { if (++reads.code === 1) return "ERR_KEEP_FIRST_CODE"; throw new Error("code second read"); } },
      message: { get() { if (++reads.message === 1) return ""; throw new Error("message second read"); } },
    });
    const request = Object.defineProperty({ type: "setSpeed", generation: 0, ...(requestId === undefined ? {} : { requestId }) }, "speed", {
      get() { throw error; },
    });
    messageListener!({ data: request } as MessageEvent);
    assert.deepEqual(reads, { code: 1, message: 1 });
    if (requestId === undefined) {
      const { type, generation, ...details } = structuredClone(posted[0]) as Record<string, unknown>;
      assert.equal(type, "failure");
      assert.equal(generation, 0);
      assert.deepEqual(parseHostFailure(details), {
        code: "WASM_WORKER_PROTOCOL", where: "wasm-worker.message",
        message: "非结构化错误没有可读的非空 message",
        cause: { code: "ERR_KEEP_FIRST_CODE", message: "" },
      });
    } else {
      assert.deepEqual(posted, [{ type: "operationError", requestId, generation: 0, message: "非结构化错误没有可读的非空 message" }]);
    }
  });
}

test("listener pending 帧遇到死端口仍停 timer，原始 failure 与停止二次错误都有诊断且无 unhandledRejection", { timeout: 10000 }, async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const now = performance.now() + 1;
  context.mock.method(performance, "now", () => now);
  const captured = { loop: undefined as WasmTickLoop<ReturnType<typeof setTimeout>> | undefined };
  const originalStart = WasmTickLoop.prototype.start;
  context.mock.method(WasmTickLoop.prototype, "start", function (this: WasmTickLoop<ReturnType<typeof setTimeout>>) {
    captured.loop = this;
    originalStart.call(this);
  });
  const originalClear = globalThis.clearTimeout;
  let cancellations = 0;
  context.mock.method(globalThis, "clearTimeout", (timer: ReturnType<typeof setTimeout>) => { cancellations++; originalClear(timer); });
  posted.length = 0;
  messageListener!({ data: { type: "start", generation: 0, requestId: 94 } } as MessageEvent);
  const activeLoop = captured.loop;
  assert.ok(activeLoop);
  activeLoop.setFrameRate(0.01);
  activeLoop.publish(tickBatch([frame(1, 0, ["600000"])], snapshot(1, 1)), false);
  assert.deepEqual(posted, [{ type: "started", generation: 0, requestId: 94 }]);
  const original = { code: "REPORT_CORRECTION_REJECTED", message: "原始更正拒绝", where: "engine::report_corrections", recoverable: true, cause: { receipt: "original" } };
  const logged: unknown[][] = [];
  context.mock.method(console, "error", (...args: unknown[]) => { logged.push(args); });
  const unhandled: unknown[] = [];
  const onUnhandled = (error: unknown) => { unhandled.push(error); };
  process.on("unhandledRejection", onUnhandled);
  persistentPostError = new Error("pending frame port dead");
  try {
    const request = Object.defineProperty({ type: "setSpeed", generation: 0, requestId: 95 }, "speed", { get() { throw original; } });
    messageListener!({ data: request } as MessageEvent);
    await new Promise((resolve) => setImmediate(resolve));
    assert.equal(activeLoop.isRunning(), false);
    assert.equal(activeLoop.readSpeedMetrics().running, false);
    assert.equal(cancellations, 1);
    context.mock.timers.tick(100);
    assert.deepEqual(unhandled, []);
    assert.equal(logged.filter((args) => /operationError 通道发送失败/.test(String(args[0]))).length, 1);
    const lastResort = logged.find((args) => /错误报告通道双失败/.test(String(args[0])));
    assert.ok(lastResort, "停止二次错误不得阻止原始 failure 的受保护报告");
    const details = lastResort[1] as Record<string, unknown>;
    assert.equal(details.code, original.code);
    assert.equal(details.message, original.message);
    assert.deepEqual(details.cause, original.cause);
    assert.match(JSON.stringify(details.context), /停止.*pending frame port dead/);
  } finally {
    persistentPostError = null;
    activeLoop.stop();
    process.off("unhandledRejection", onUnhandled);
  }
});
