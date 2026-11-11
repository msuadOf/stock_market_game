import assert from "node:assert/strict";
import test from "node:test";
import { createFacadeEngineHandshake } from "./facade-engine-handshake.ts";
import { HEADER_WORDS, SLOT_STRIDE, JOIN_STRIDE } from "./facade-slot-protocol.ts";

class Port extends EventTarget {
  starts = 0; closes = 0; sent: unknown[] = []; postError?: unknown;
  start() { this.starts++; }
  close() { this.closes++; }
  postMessage(value: unknown) { if (this.postError !== undefined) throw this.postError; this.sent.push(value); }
  emit(value: unknown) { this.dispatchEvent(new MessageEvent("message", { data: value })); }
}
function fixture() {
  const port = new Port(), failures: unknown[] = [], timers = new Map<number, () => void>();
  let id = 0;
  const handshake = createFacadeEngineHandshake({ port: port as unknown as MessagePort, onFailure: (error) => { failures.push(error); }, timers: { setTimeout: (callback) => { timers.set(++id, callback); return id; }, clearTimeout: (timer) => { timers.delete(timer); } } });
  const substrate = { module: new WebAssembly.Module(new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0])), memory: new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true }), layout: { baseAddr: 0, headerWords: HEADER_WORDS, slotStride: SLOT_STRIDE, joinStride: JOIN_STRIDE, capacity: 4, genBits: 29 }, diagnostics: false };
  return { port, failures, timers, handshake, substrate };
}

test("engine 未 configured 时不可 create/ingress；确认前有界等待且保持同一共享身份", { timeout: 10000 }, async () => {
  const f = fixture(); assert.throws(() => f.handshake.assertConfigured(), /configured/);
  const waiting = f.handshake.configure(f.substrate);
  assert.equal(f.port.starts, 1); assert.equal(f.timers.size, 1);
  assert.equal((f.port.sent[0] as Record<string, unknown>).memory, f.substrate.memory);
  assert.throws(() => f.handshake.assertConfigured(), /configured/);
  f.port.emit({ type: "facadeEngineConfigured" }); await waiting;
  assert.doesNotThrow(() => f.handshake.assertConfigured()); assert.equal(f.timers.size, 0);
  f.handshake.dispose(); assert.equal(f.port.closes, 1);
});

test("engine 重复 configured 立即可见失败并关闭本人控制端", { timeout: 10000 }, async () => {
  const f = fixture(), waiting = f.handshake.configure(f.substrate);
  f.port.emit({ type: "facadeEngineConfigured" }); await waiting;
  f.port.emit({ type: "facadeEngineConfigured" });
  assert.equal(f.failures.length, 1); assert.match((f.failures[0] as Error).message, /重复/);
  assert.throws(() => f.handshake.assertConfigured()); assert.equal(f.port.closes, 1);
});

test("engine configured 等待超时或发送抛错均显式 reject 并清端/计时器", { timeout: 10000 }, async () => {
  const f = fixture(), waiting = f.handshake.configure(f.substrate);
  [...f.timers.values()][0](); await assert.rejects(waiting, /10000ms/);
  assert.equal(f.timers.size, 0); assert.equal(f.port.closes, 1);
  const g = fixture(); g.port.postError = new Error("substrate post failed");
  await assert.rejects(g.handshake.configure(g.substrate), /substrate post failed/);
  assert.equal(g.timers.size, 0); assert.equal(g.port.closes, 1);
});

test("不可字符串化的 post 异常在 reject 前仍关闭端与计时器", { timeout: 10000 }, async () => {
  const f = fixture();
  let conversions = 0;
  const error = { toString() { conversions++; throw new Error("toString denied"); }, toJSON() { conversions++; throw new Error("toJSON denied"); } };
  f.port.postError = error;
  // 此层尚未跨消息边界；保留原值交既有 a29 reporter，而不先字符串化丢型。
  await assert.rejects(f.handshake.configure(f.substrate), (failure) => failure === error);
  assert.equal(conversions, 0);
  assert.equal(f.port.closes, 1); assert.equal(f.timers.size, 0);
});

test("确认后 failure callback 抛错前已清理，dispose 后迟到确认不复活", { timeout: 10000 }, async () => {
  const listeners = new Map<string, (event: { data?: unknown }) => void>(); let closes = 0;
  const expected = new Error("failure callback failed");
  const handshake = createFacadeEngineHandshake({
    port: { addEventListener: (type: string, listener: (event: { data?: unknown }) => void) => { listeners.set(type, listener); }, removeEventListener: (type: string) => { listeners.delete(type); }, start() {}, close() { closes++; }, postMessage() {} } as unknown as MessagePort,
    onFailure() { assert.equal(closes, 1); throw expected; },
    timers: { setTimeout: () => 1, clearTimeout() {} },
  });
  const receive = listeners.get("message")!;
  const waiting = handshake.configure(fixture().substrate);
  receive({ data: { type: "facadeEngineConfigured" } }); await waiting;
  assert.throws(() => receive({ data: { type: "facadeEngineConfigured" } }), (error) => error === expected);
  handshake.dispose(); receive({ data: { type: "facadeEngineConfigured" } });
  assert.equal(closes, 1); assert.equal(listeners.size, 0); assert.throws(() => handshake.assertConfigured());
});

test("等待中 dispose 显式 reject，重复 configure/messageerror 不留下本地资源", { timeout: 10000 }, async () => {
  const f = fixture(); const waiting = f.handshake.configure(f.substrate);
  assert.throws(() => f.handshake.configure(f.substrate), /一次/);
  f.handshake.dispose(); await assert.rejects(waiting, /dispose/);
  f.port.emit({ type: "facadeEngineConfigured" }); assert.equal(f.port.closes, 1); assert.equal(f.timers.size, 0);
  const g = fixture(), pending = g.handshake.configure(g.substrate);
  g.port.dispatchEvent(new Event("messageerror")); await assert.rejects(pending, /messageerror/);
  assert.equal(g.port.closes, 1); assert.equal(g.timers.size, 0);
});

test("LibOS 故障布局只随 Memory 和 primary 确认后的配置消息传递", { timeout: 10000 }, async () => {
  const f = fixture();
  const fault = {globalStateAddress: 8};
  assert.throws(() => f.handshake.configure({...f.substrate, libosFault:fault}), /Memory|primary/);
  const memory = f.handshake.registerMemory(f.substrate.memory);
  f.port.emit({type:"facadeEngineMemoryRegistered", allocationId:1}); await memory;
  const death = f.handshake.registerDeath({});
  f.port.emit({type:"facadeEngineDeathRegistered", allocationId:1}); await death;
  const configured = f.handshake.configure({...f.substrate, libosFault:fault});
  const message = f.port.sent.at(-1) as Record<string, unknown>;
  assert.equal(message.type, "facadeEngineConfiguration"); assert.equal(message.libosFault, fault);
  f.port.emit({type:"facadeEngineConfigured"}); await configured;
  f.handshake.dispose();
});
