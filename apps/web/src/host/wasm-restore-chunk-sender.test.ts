import assert from "node:assert/strict";
import test from "node:test";
import { sendRestoreChunks, RESTORE_CHUNK_MAX_BYTES, RESTORE_WASM_HEADROOM_BYTES } from "./wasm-restore-chunk-sender.ts";

function fixture() {
  const calls: string[] = []; const sent: number[] = [];
  return { calls, sent, glue: {
    facade_restore_chunks_begin(n: number) { calls.push(`begin:${n}`); return 1; },
    facade_restore_chunks_push(_token: number, chunk: Uint8Array) { calls.push(`push:${chunk.length}`); sent.push(...chunk); },
    facade_restore_chunks_finish(_token: number) { calls.push("finish"); },
    facade_restore_chunks_clear(_token: number) { calls.push("clear"); return true; },
  } };
}
async function* chunks(...parts: Uint8Array[]) { yield* parts; }
const allowance = RESTORE_WASM_HEADROOM_BYTES + RESTORE_CHUNK_MAX_BYTES + 1024;

test("分块 UTF-8 跨字节边界完整传递后才 finish", async () => {
  const f = fixture(); const bytes = new TextEncoder().encode('{"名":"🚀"}');
  assert.equal(await sendRestoreChunks(f.glue, chunks(...Array.from(bytes, byte => Uint8Array.of(byte))), bytes.length, allowance), 1);
  assert.deepEqual(f.sent, [...bytes]); assert.equal(f.calls[0], `begin:${bytes.length}`);
  assert.equal(f.calls.at(-1), "finish"); assert.ok(!f.calls.includes("clear"));
});

test("过长、过短、块过大、非法 UTF-8 均清理且不 finish", async () => {
  for (const [parts, total] of [
    [[Uint8Array.of(1, 2)], 1], [[Uint8Array.of(1)], 2],
    [[new Uint8Array(RESTORE_CHUNK_MAX_BYTES + 1)], RESTORE_CHUNK_MAX_BYTES + 1],
    [[Uint8Array.of(0xf0, 0x9f)], 2],
  ] as const) {
    const f = fixture();
    await assert.rejects(sendRestoreChunks(f.glue, chunks(...parts), total, allowance + RESTORE_CHUNK_MAX_BYTES));
    assert.equal(f.calls.at(-1), "clear"); assert.ok(!f.calls.includes("finish"));
  }
});

test("预算不足与非法声明拒绝且不清理其他所有者", async () => {
  for (const [total, budget] of [[1, RESTORE_WASM_HEADROOM_BYTES + RESTORE_CHUNK_MAX_BYTES], [-1, allowance], [2 ** 32, 2 ** 33]]) {
    const f = fixture(); await assert.rejects(sendRestoreChunks(f.glue, chunks(), total, budget));
    assert.deepEqual(f.calls, []);
  }
});

test("取消与 push 或 finish 失败均清理并保留原始异常", async () => {
  for (const failureAt of ["cancel", "push", "finish"] as const) {
    const f = fixture(); const error = new Error(failureAt); const controller = new AbortController();
    if (failureAt === "cancel") controller.abort(error);
    if (failureAt === "push") f.glue.facade_restore_chunks_push = () => { throw error; };
    if (failureAt === "finish") f.glue.facade_restore_chunks_finish = () => { throw error; };
    await assert.rejects(sendRestoreChunks(f.glue, chunks(Uint8Array.of(65)), 1, allowance, controller.signal), value => value === error);
    if (failureAt === "cancel") assert.deepEqual(f.calls, []);
    else assert.equal(f.calls.at(-1), "clear");
  }
});

test("begin 拒绝时保留已有恢复档，不调用 clear", async () => {
  const f = fixture(); const existing = new Error("已有恢复档");
  f.glue.facade_restore_chunks_begin = () => { throw existing; };
  await assert.rejects(sendRestoreChunks(f.glue, chunks(Uint8Array.of(65)), 1, allowance), value => value === existing);
  assert.deepEqual(f.calls, []);
});

test("清理本身失败时同时保留原始错误", async () => {
  const f = fixture(); const cause = new Error("push"); const cleanup = new Error("clear");
  f.glue.facade_restore_chunks_push = () => { throw cause; }; f.glue.facade_restore_chunks_clear = () => { throw cleanup; };
  await assert.rejects(sendRestoreChunks(f.glue, chunks(Uint8Array.of(65)), 1, allowance), error => error instanceof AggregateError && error.errors[0] === cause && error.errors[1] === cleanup);
});

test("接收途中取消或读取源失败均不完成恢复", async () => {
  for (const kind of ["cancel", "source"] as const) {
    const f = fixture(); const error = new Error(kind); const controller = new AbortController();
    async function* input() {
      yield Uint8Array.of(65);
      if (kind === "cancel") { controller.abort(error); yield Uint8Array.of(66); }
      else throw error;
    }
    await assert.rejects(sendRestoreChunks(f.glue, input(), 2, allowance, controller.signal), value => value === error);
    assert.deepEqual(f.calls, ["begin:2", "push:1", "clear"]);
  }
});

test("旧 sender 晚到不能写入或清理新的接收所有者", async () => {
  let next = 0; let owner = 0; const writes: number[] = [];
  const glue = {
    facade_restore_chunks_begin() { owner = ++next; return owner; },
    facade_restore_chunks_push(token: number, chunk: Uint8Array) { if (token !== owner) throw new Error("token 不匹配"); writes.push(...chunk); },
    facade_restore_chunks_finish(token: number) { if (token !== owner) throw new Error("token 不匹配"); },
    facade_restore_chunks_clear(token: number) { if (token !== owner) return false; owner = 0; return true; },
  };
  let resume!: () => void; let ready!: () => void;
  const waiting = new Promise<void>(resolve => { resume = resolve; });
  const started = new Promise<void>(resolve => { ready = resolve; });
  async function* input() { ready(); await waiting; yield Uint8Array.of(65); }
  const old = sendRestoreChunks(glue, input(), 1, allowance);
  await started;
  assert.equal(glue.facade_restore_chunks_clear(1), true);
  assert.equal(glue.facade_restore_chunks_begin(), 2);
  resume();
  await assert.rejects(old, /token 不匹配/);
  assert.equal(owner, 2); assert.deepEqual(writes, []);
  glue.facade_restore_chunks_push(2, Uint8Array.of(66));
  assert.deepEqual(writes, [66]);
});
