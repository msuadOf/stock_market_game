import assert from "node:assert/strict";
import test from "node:test";

const posted: unknown[] = [];
let messageListener: ((event: MessageEvent<Record<string, unknown> & { type: string }>) => void) | null = null;
const originalSelf = Object.getOwnPropertyDescriptor(globalThis, "self");
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
const { postFailure } = await import("./wasm-worker.ts");
if (originalSelf === undefined) delete (globalThis as { self?: unknown }).self;
else Object.defineProperty(globalThis, "self", originalSelf);

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
