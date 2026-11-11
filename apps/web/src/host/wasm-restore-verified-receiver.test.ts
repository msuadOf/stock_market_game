import assert from "node:assert/strict";
import test from "node:test";
import { registerHooks } from "node:module";
import { createSha256Chain } from "../save/sha256-chain.ts";
import { sendRestoreChunks } from "./restore-chunk-port.ts";
import { receiveVerifiedRestore } from "./wasm-restore-verified-receiver.ts";

const timeout = { timeout: 10000 }; const budget = 8 * 1024 * 1024;
async function descriptor(bytes: Uint8Array) { const hash = await createSha256Chain(); await hash.update(bytes); return hash.finish(); }
async function* source(bytes: Uint8Array) { yield bytes; }
function glue() {
  const calls: string[] = [], bytes: number[] = [];
  return { calls, bytes, bindings: {
    facade_restore_chunks_begin(total: number) { calls.push(`begin:${total}`); return 7; },
    facade_restore_chunks_push(token: number, value: Uint8Array) { assert.equal(token, 7); calls.push("push"); bytes.push(...value); },
    facade_restore_chunks_finish(token: number) { assert.equal(token, 7); calls.push("finish"); },
    facade_restore_chunks_clear(token: number) { assert.equal(token, 7); calls.push("clear"); return true; },
  } };
}

test("实际字节与完整摘要一致后才调用 WASM finish", timeout, async () => {
  const bytes = new TextEncoder().encode("中国😀"), expected = await descriptor(bytes), channel = new MessageChannel(), f = glue();
  const receiver = receiveVerifiedRestore(channel.port2, 4, expected, budget, f.bindings);
  const sender = sendRestoreChunks(channel.port1, 4, bytes.length, source(bytes));
  assert.equal(await receiver, 7); await sender;
  assert.deepEqual(f.bytes, [...bytes]); assert.deepEqual(f.calls, [`begin:${bytes.length}`, "push", "finish"]);
});

test("同长篡改、短流与取消均不 finish 且只清理本 token", timeout, async () => {
  const good = new TextEncoder().encode("abc"), expected = await descriptor(good);
  for (const scenario of ["tamper", "short", "abort"] as const) {
    const channel = new MessageChannel(), f = glue(), abort = new AbortController();
    const receiver = receiveVerifiedRestore(channel.port2, 4, expected, budget, f.bindings, abort.signal);
    const sent = sendRestoreChunks(channel.port1, 4, good.length, source(new TextEncoder().encode(scenario === "tamper" ? "abd" : scenario === "short" ? "a" : "abc")), abort.signal);
    const settledSent = Promise.allSettled([sent]);
    if (scenario === "abort") abort.abort();
    await assert.rejects(receiver); assert.equal((await settledSent)[0]?.status, "rejected");
    assert.ok(!f.calls.includes("finish"));
    if (f.calls.some(call => call.startsWith("begin:"))) assert.equal(f.calls.at(-1), "clear");
  }
});

test("拒绝 SharedArrayBuffer，普通缓冲传送后改写不影响已接收字节", timeout, async () => {
  const expected = await descriptor(Uint8Array.of(65)), sharedChannel = new MessageChannel(), f = glue();
  const receiver = receiveVerifiedRestore(sharedChannel.port2, 4, expected, budget, f.bindings);
  const shared = new Uint8Array(new SharedArrayBuffer(1)); shared[0] = 65;
  sharedChannel.port1.postMessage({ type: "chunk", generation: 4, seq: 0, bytes: shared });
  await assert.rejects(receiver, /共享内存/); sharedChannel.port1.close();
  assert.ok(!f.calls.includes("finish")); assert.equal(f.calls.at(-1), "clear");

  const channel = new MessageChannel(), g = glue(), original = Uint8Array.of(65);
  const accepted = receiveVerifiedRestore(channel.port2, 5, expected, budget, g.bindings);
  channel.port1.postMessage({ type: "chunk", generation: 5, seq: 0, bytes: original }); original[0] = 66;
  await new Promise<MessageEvent>(resolve => { channel.port1.onmessage = resolve; });
  channel.port1.postMessage({ type: "eof", generation: 5, seq: 1, length: 1 });
  await accepted; assert.deepEqual(g.bytes, [65]); assert.equal(g.calls.at(-1), "finish"); channel.port1.close();
});

test("begin 失败不得清理已有其他 token", timeout, async () => {
  const expected = await descriptor(Uint8Array.of(65)), channel = new MessageChannel(), f = glue();
  const failure = new Error("已有其他 token"); f.bindings.facade_restore_chunks_begin = () => { throw failure; };
  await assert.rejects(receiveVerifiedRestore(channel.port2, 4, expected, budget, f.bindings), error => error === failure);
  assert.deepEqual(f.calls, []); channel.port1.close();
});

test("摘要 await 期间对端关闭后不得继续 push 或 finish", timeout, async () => {
  const key = Symbol.for("verified-receiver-hash-gate");
  const hooks = registerHooks({
    resolve(specifier, context, next) {
      if (specifier === "../save/sha256-chain.ts" && context.parentURL?.includes("wasm-restore-verified-receiver.ts?gate")) return { url: "virtual:receiver-gated-hash", shortCircuit: true };
      return next(specifier, context);
    },
    load(url, context, next) {
      if (url === "virtual:receiver-gated-hash") return { format: "module", shortCircuit: true, source: `export const SHA256_CHAIN_ALGORITHM="sha256-chain-v1", SHA256_CHAIN_BLOCK_BYTES=65536; export async function createSha256Chain(){return globalThis[Symbol.for("verified-receiver-hash-gate")].chain}` };
      return next(url, context);
    },
  });
  try {
    const moduleUrl: string = "./wasm-restore-verified-receiver.ts?gate";
    const { receiveVerifiedRestore: receive } = await import(moduleUrl);
    const expected = await descriptor(Uint8Array.of(65));
    for (const phase of ["update", "finish"] as const) {
      let started!: () => void, release!: () => void;
      const waiting = new Promise<void>(resolve => { started = resolve; });
      const gate = new Promise<void>(resolve => { release = resolve; });
      Object.defineProperty(globalThis, key, { configurable: true, value: { chain: {
        async update() { if (phase === "update") { started(); await gate; } },
        async finish() { if (phase === "finish") { started(); await gate; } return expected; },
      } } });
      const channel = new MessageChannel(), f = glue();
      const closed = new Promise<void>(resolve => channel.port2.addEventListener("close", () => resolve(), { once: true }));
      const receiver = receive(channel.port2, 4, expected, budget, f.bindings);
      channel.port1.postMessage({ type: "chunk", generation: 4, seq: 0, bytes: Uint8Array.of(65) });
      if (phase === "finish") {
        await new Promise<MessageEvent>(resolve => { channel.port1.onmessage = resolve; });
        channel.port1.postMessage({ type: "eof", generation: 4, seq: 1, length: 1 });
      }
      await waiting; channel.port1.close(); await closed; release();
      await assert.rejects(receiver);
      assert.equal(f.calls.includes(phase === "update" ? "push" : "finish"), false);
      assert.equal(f.calls.at(-1), "clear");
    }
  } finally { hooks.deregister(); Reflect.deleteProperty(globalThis, key); }
});

test("重复序号不可完成，非法 descriptor 在 begin 前拒绝", timeout, async () => {
  const expected = await descriptor(Uint8Array.of(65, 66)), channel = new MessageChannel(), f = glue();
  const receiver = receiveVerifiedRestore(channel.port2, 4, expected, budget, f.bindings);
  channel.port1.postMessage({ type: "chunk", generation: 4, seq: 0, bytes: Uint8Array.of(65) });
  channel.port1.postMessage({ type: "chunk", generation: 4, seq: 0, bytes: Uint8Array.of(66) });
  await assert.rejects(receiver); channel.port1.close();
  assert.ok(!f.calls.includes("finish")); assert.equal(f.calls.at(-1), "clear");
  const second = new MessageChannel(), g = glue();
  await assert.rejects(receiveVerifiedRestore(second.port2, 4, { ...expected, digest: "坏" }, budget, g.bindings));
  assert.deepEqual(g.calls, []); second.port1.close();
});
