import assert from "node:assert/strict";
import test from "node:test";
import { WasmSessionSlot } from "./wasm-session-slot.ts";
import { describeWasmFailure } from "./wasm-failure.ts";

function fixture(shutdown?: (handle: number) => unknown) {
  const drops: number[] = [], calls: number[] = [];
  const bindings = { create_session: () => 7, restore_json: () => 8, snapshot: () => ({}), drop_session: (handle: number) => { drops.push(handle); },
    ...(shutdown === undefined ? {} : { shutdown_session: (handle: number) => { calls.push(handle); return shutdown(handle); } }) };
  const slot = new WasmSessionSlot(() => bindings);
  slot.create({}, 1n);
  return { slot, drops, calls };
}
test("shutdown 消费一次 handle，成功后 drop 不重复调用 Rust", { timeout: 10000 }, () => {
  const f = fixture((handle) => ({ handle, result: { runtime: undefined } }));
  assert.deepEqual(f.slot.shutdown(), { kind: "success", handle: 7, runtime: null });
  f.slot.drop();
  assert.throws(() => f.slot.shutdown(), /尚未就绪/);
  assert.deepEqual(f.calls, [7]); assert.deepEqual(f.drops, []);
});
test("shutdown 失败保留 primary/secondary/completion，仍只消费一次", { timeout: 10000 }, () => {
  const raw = { handle: 7, result: { primary: { stage: "join", worker: "group-0", detail: "panic" }, secondary: [{ stage: "final", worker: undefined, detail: "missing" }], completion: { owned_workers: 1, joined_workers: 1, worker_results: [{ worker: "group-0", result: { Err: "panic" } }], group_finals: 0, desk_finals: 0 } } };
  const f = fixture(() => { throw raw; });
  const result = f.slot.shutdown();
  assert.equal(result.kind, "failure");
  if (result.kind !== "failure") throw new Error("应为失败");
  assert.equal(result.primary.detail, "panic"); assert.equal(result.secondary[0].detail, "missing"); assert.equal(result.completion?.joinedWorkers, 1);
  f.slot.drop(); assert.deepEqual(f.drops, []); assert.deepEqual(f.calls, [7]);
});
test("缺少 shutdown export 显式失败，不假装 drop 为 join", { timeout: 10000 }, () => {
  const f = fixture();
  assert.throws(() => f.slot.shutdown(), /shutdown_session/);
  assert.deepEqual(f.drops, []);
  assert.equal(f.slot.requireHandle()[0], 7);
});
test("shutdown FFI 非 envelope 异常保留原值与 decoder 次错，且 handle 已消费", { timeout: 10000 }, () => {
  const original = "shutdown report serialize failed: 原始具体原因";
  const f = fixture(() => { throw original; });
  let actual: unknown;
  try { f.slot.shutdown(); } catch (error) { actual = error; }
  assert.ok(actual instanceof Error);
  assert.equal((actual as Error & { code?: string }).code, "WASM_SHUTDOWN_REPORT_INVALID");
  assert.equal((actual as Error & { where?: string }).where, "wasm-session-slot.shutdown");
  const cause = (actual as Error & { cause: { ffiError: unknown; decoderError: { name: string; message: string } } }).cause;
  assert.deepEqual(cause.ffiError, original);
  assert.equal((cause.decoderError as Error).name, "Error");
  assert.match(cause.decoderError.message, /必须为对象/);
  const clonedCause = structuredClone(cause);
  assert.equal(clonedCause.ffiError, original);
  assert.equal(describeWasmFailure(clonedCause.decoderError), describeWasmFailure(cause.decoderError));
  f.slot.drop();
  assert.deepEqual(f.calls, [7]); assert.deepEqual(f.drops, []);
  assert.throws(() => f.slot.requireHandle(), /尚未就绪/);
});

test("shutdown 未知结构的 decoder 失败也显式保留两项原因", { timeout: 10000 }, () => {
  const original = { foreign: "shape" };
  const f = fixture(() => { throw original; });
  let actual: unknown;
  try { f.slot.shutdown(); } catch (error) { actual = error; }
  assert.ok(actual instanceof Error);
  const cause = (actual as Error & { cause: { ffiError: unknown; decoderError: { name: string; message: string } } }).cause;
  assert.deepEqual(cause.ffiError, original);
  assert.equal((cause.decoderError as Error).name, "Error");
  assert.match(cause.decoderError.message, /字段不完整/);
  const serialized = structuredClone(actual as Error & { code: string; where: string; message: string; cause: unknown });
  assert.match(serialized.message, /shutdown report/);
  assert.deepEqual((serialized.cause as { ffiError: unknown }).ffiError, original);
  assert.equal(describeWasmFailure((serialized.cause as { decoderError: unknown }).decoderError), describeWasmFailure(cause.decoderError));
});

test("shutdown decoder getter 异常同时保留原 FFI 值", { timeout: 10000 }, () => {
  const decoderError = new Error("getter decoder failed");
  const original = Object.defineProperty({ result: {} }, "handle", { enumerable: true, get() { throw decoderError; } });
  const f = fixture(() => { throw original; });
  let actual: unknown;
  try { f.slot.shutdown(); } catch (error) { actual = error; }
  assert.ok(actual instanceof Error);
  const cause = (actual as Error & { cause: { ffiError: unknown; decoderError: { name: string; message: string } } }).cause;
  assert.deepEqual((cause.ffiError as Record<string, unknown>).result, {});
  assert.match(String((cause.ffiError as Record<string, unknown>).handle), /accessor|读取/);
  assert.equal((cause.decoderError as Error).message, decoderError.message);
});

test("shutdown 原 FFI Error 的自定义字段通过 cause 克隆保留", { timeout: 10000 }, () => {
  const original = Object.assign(new Error("FFI shutdown failed"), { code: "FFI_SERIALIZE", context: { stage: "serialize-sentinel" } });
  const f = fixture(() => { throw original; });
  assert.throws(() => f.slot.shutdown(), (actual: unknown) => {
    const transported = structuredClone((actual as Error).cause) as { ffiError: { message: string; code: string; context: unknown }; decoderError: { message: string } };
    assert.equal(transported.ffiError.message, original.message);
    assert.equal(transported.ffiError.code, original.code);
    assert.deepEqual(transported.ffiError.context, original.context);
    assert.match(transported.decoderError.message, /字段不完整/);
    return true;
  });
});


test("shutdown 非 envelope TypeError 的 name、nested cause 与资源不可用诊断经克隆保留", { timeout: 10000 }, () => {
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 1, shared: true });
  const original = Object.assign(new TypeError("FFI type error"), { cause: Object.assign(new Error("nested original"), { code: "NESTED_FFI" }), context: { bytes: memory.buffer } });
  const f = fixture(() => { throw original; });
  assert.throws(() => f.slot.shutdown(), (actual: unknown) => {
    const { ffiError } = structuredClone((actual as Error).cause) as { ffiError: { name: string; cause: { message: string; code: string }; context: { bytes: { unavailable: string } } } };
    assert.equal(ffiError.name, "TypeError");
    assert.equal(ffiError.cause.message, "nested original");
    assert.equal(ffiError.cause.code, "NESTED_FFI");
    assert.match(ffiError.context.bytes.unavailable, /内存资源/);
    return true;
  });
});
