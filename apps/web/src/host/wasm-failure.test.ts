import assert from "node:assert/strict";
import test from "node:test";
import { parseWasmFailure } from "./wasm-failure.ts";

test("Given a structured WASM HostFailure, when parsed, then its stable code and message survive", () => {
  assert.deepEqual(
    parseWasmFailure("wasm-host.step", {
      code: "STEP_FATAL",
      message: "invariant violation at web-wasm.step: receipt chain broke",
    }),
    {
      code: "STEP_FATAL",
      where: "wasm-host.step",
      message: "invariant violation at web-wasm.step: receipt chain broke",
    },
  );
});

test("Given an ordinary WASM exception, when parsed, then it stays an explicit boundary failure", () => {
  assert.deepEqual(parseWasmFailure("wasm-host.snapshot", new Error("snapshot decode failed")), {
    code: "WASM_PROTOCOL",
    where: "wasm-host.snapshot",
    message: "snapshot decode failed",
  });
});

test("Given a hostile structured error accessor, when parsed, then the accessor failure is explicit", () => {
  const hostile = Object.defineProperty({}, "code", {
    get() {
      throw new Error("getter exploded");
    },
  });

  assert.deepEqual(parseWasmFailure("wasm-worker.step", hostile), {
    code: "WASM_PROTOCOL",
    where: "wasm-worker.step",
    message: "读取结构化 WASM 错误失败：getter exploded",
  });
});

test("Given an accessor throws an unprintable object, when parsed, then failure reporting cannot throw again", () => {
  const unprintable: Record<PropertyKey, unknown> = {
    toJSON() {
      throw unprintable;
    },
    [Symbol.toPrimitive]() {
      throw unprintable;
    },
  };
  const hostile = Object.defineProperty({}, "code", {
    get() {
      throw unprintable;
    },
  });

  assert.deepEqual(parseWasmFailure("wasm-worker.step", hostile), {
    code: "WASM_PROTOCOL",
    where: "wasm-worker.step",
    message: "读取结构化 WASM 错误失败：非标准错误对象无法序列化：<无法读取错误详情>",
  });
});

test("Given a revoked Proxy, when parsed, then failure reporting cannot throw again", () => {
  const { proxy, revoke } = Proxy.revocable({}, {});
  revoke();
  let failure: ReturnType<typeof parseWasmFailure> | undefined;

  assert.doesNotThrow(() => {
    failure = parseWasmFailure("wasm-worker.step", proxy);
  });
  assert.equal(failure?.code, "WASM_PROTOCOL");
  assert.equal(failure?.where, "wasm-worker.step");
  assert.match(failure?.message ?? "", /^读取结构化 WASM 错误失败：/);
});

test("Given changing structured-error getters, when parsed, then each field is read exactly once", () => {
  let codeReads = 0;
  let messageReads = 0;
  const changing = {
    get code() {
      codeReads += 1;
      return codeReads === 1 ? "STEP_FATAL" : "";
    },
    get message() {
      messageReads += 1;
      return messageReads === 1 ? "receipt chain broke" : "";
    },
  };

  assert.deepEqual(parseWasmFailure("wasm-host.step", changing), {
    code: "STEP_FATAL",
    where: "wasm-host.step",
    message: "receipt chain broke",
  });
  assert.equal(codeReads, 1);
  assert.equal(messageReads, 1);
});

test("Given an Error-shaped object with a non-string message, when parsed, then its message is safely stringified", () => {
  const malformedError = Object.defineProperty(Object.create(Error.prototype) as Error, "message", {
    get() {
      return Symbol("bad");
    },
  });

  assert.deepEqual(parseWasmFailure("wasm-host.step", malformedError), {
    code: "WASM_PROTOCOL",
    where: "wasm-host.step",
    message: "Symbol(bad)",
  });
});

test("Given JSON serialization throws an Error-shaped object with a non-string message, when parsed, then reporting still succeeds", () => {
  const malformedError = Object.defineProperty(Object.create(Error.prototype) as Error, "message", {
    get() {
      return Symbol("bad");
    },
  });
  const malformed = {
    toJSON() {
      throw malformedError;
    },
  };

  assert.deepEqual(parseWasmFailure("wasm-host.step", malformed), {
    code: "WASM_PROTOCOL",
    where: "wasm-host.step",
    message: "非标准错误对象无法序列化：Symbol(bad)",
  });
});
