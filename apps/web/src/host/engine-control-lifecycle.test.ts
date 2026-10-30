import assert from "node:assert/strict";
import test from "node:test";
import { createEngineControlClient } from "./engine-control-client.ts";
import { createEngineControlOwner } from "./engine-control-owner.ts";
import { setImmediate } from "node:timers/promises";
import { CAPACITY, GEN_BITS, FREE_HEAD, FREE_HEAD_EMPTY, HEADER_WORDS, SLOT_STRIDE, JOIN_STRIDE, SLOT_JOIN_INDEX, SlotState, JoinState, encodeW, encodeJoin, slotWordOffset, slotFieldOffset, joinWordOffset } from "./facade-slot-protocol.ts";

class WorkerHandle extends EventTarget {
  terminations = 0;
  readonly listeners = new Set<EventListenerOrEventListenerObject>();
  readonly sent: Record<string, unknown>[] = [];
  override addEventListener(type: string, listener: EventListenerOrEventListenerObject | null, options?: AddEventListenerOptions | boolean) {
    if (listener !== null) this.listeners.add(listener);
    super.addEventListener(type, listener, options);
  }
  override removeEventListener(type: string, listener: EventListenerOrEventListenerObject | null, options?: EventListenerOptions | boolean) {
    if (listener !== null) this.listeners.delete(listener);
    super.removeEventListener(type, listener, options);
  }
  postMessage(value: Record<string, unknown>) { this.sent.push(value); }
  terminate() { this.terminations++; }
  emit(value: unknown) { this.dispatchEvent(new MessageEvent("message", { data: value })); }
}

async function withClient(run: (client: ReturnType<typeof createEngineControlClient>, worker: WorkerHandle, failures: Error[], observations: unknown[]) => Promise<void>) {
  const original = Object.getOwnPropertyDescriptor(globalThis, "Worker");
  const worker = new WorkerHandle();
  Object.defineProperty(globalThis, "Worker", { configurable: true, value: class { constructor() { return worker; } } });
  const failures: Error[] = [], observations: unknown[] = [];
  const client = createEngineControlClient({ runtimeOverride: false, onFailure: (error) => { failures.push(error); }, onSpawnFailure() {}, onLifecycleObservation: (event) => { observations.push(event); } });
  void client.configured.catch(() => {});
  void client.release.catch(() => {});
  client.start();
  try { await run(client, worker, failures, observations); }
  finally {
    client.enginePort.close();
    const transferred = worker.sent[0]?.dataPort;
    if (transferred instanceof MessagePort) transferred.close();
    if (original === undefined) Reflect.deleteProperty(globalThis, "Worker");
    else Object.defineProperty(globalThis, "Worker", original);
  }
}

test("control client 接受首个 child 的 slot0/gen0", { timeout: 10000 }, async () => {
  await withClient(async (client, worker, failures, observations) => {
    const event = { type: "childSpawn", workerId: 1, slot: 0, gen: 0, name: "stock-game:1:child:1:0:0", runtimeName: "rt-group-0" };
    worker.emit({ type: "lifecycle", instanceId: client.instanceId, event });
    assert.deepEqual(observations, [event]);
    assert.deepEqual(failures, []);
    worker.emit({ type: "terminal", instanceId: client.instanceId, release: { kind: "not-allocated" } });
    client.completeShutdown();
    await client.release;
  });
});

test("control client 对外 release 前撤下业务端和 control Worker", { timeout: 10000 }, async () => {
  await withClient(async (client, worker) => {
    worker.emit({ type: "configured", instanceId: client.instanceId });
    worker.emit({ type: "terminal", instanceId: client.instanceId, release: { kind: "released", finalBytes: 65536, growthStopped: true, referencesDropped: true } });
    assert.equal(worker.terminations, 0, "owner 收据不能越过 main 业务回复校验");
    client.completeShutdown();
    assert.equal((await client.release).kind, "released");
    assert.equal(worker.terminations, 1);
    assert.equal(worker.listeners.size, 0);
    assert.throws(() => client.enginePort.postMessage({ type: "late" }), /关闭/);
  });
});

test("control owner 在 supervisor Channel 构造失败后清理已创建 engine 并保留 unknown", { timeout: 10000 }, () => {
  const channel = new MessageChannel(), engine = new WorkerHandle(), posted: Record<string, unknown>[] = [];
  let listener!: (event: MessageEvent<unknown>) => void;
  createEngineControlOwner({
    createEngineWorker: () => engine as unknown as Worker,
    createChildWorker: () => { throw new Error("本例不创建 child"); },
    createIngressWorker: () => { throw new Error("本例不创建 ingress"); },
    createChannel: () => { throw new Error("Channel 构造失败"); },
    postMessage: (value) => { posted.push(value); },
    addMessageListener: (value) => { listener = value; },
    reportError: (error) => { throw error; },
  });
  try {
    assert.doesNotThrow(() => listener(new MessageEvent("message", { data: { type: "start", instanceId: 1, runtimeOverride: false, dataPort: channel.port2 } })));
    assert.equal(engine.terminations, 1);
    const receipt = posted.find((value) => value.type === "terminal");
    assert.ok(receipt);
    assert.equal((receipt.release as { kind: string }).kind, "unknown");
  } finally {
    listener(new MessageEvent("message", { data: { type: "invalid", instanceId: 1 } }));
    channel.port1.close(); channel.port2.close();
  }
});

test("control client 在 listener 构造失败后撤下已经创建的 control Worker", { timeout: 10000 }, () => {
  const original = Object.getOwnPropertyDescriptor(globalThis, "Worker");
  class FailingListenerWorker extends WorkerHandle {
    override addEventListener(type: string, listener: EventListenerOrEventListenerObject | null, options?: AddEventListenerOptions | boolean) {
      if (type === "error") throw new Error("error listener 注册失败");
      super.addEventListener(type, listener, options);
    }
  }
  const worker = new FailingListenerWorker();
  Object.defineProperty(globalThis, "Worker", { configurable: true, value: class { constructor() { return worker; } } });
  try {
    assert.throws(() => createEngineControlClient({ runtimeOverride: false, onFailure() {}, onSpawnFailure() {} }), /listener 注册失败/);
    assert.equal(worker.terminations, 1);
    assert.equal(worker.listeners.size, 0);
    assert.equal(worker.sent.length, 0, "构造失败前不得投递可分配 Memory 的 start");
  } finally {
    if (original === undefined) Reflect.deleteProperty(globalThis, "Worker");
    else Object.defineProperty(globalThis, "Worker", original);
  }
});

async function withConfiguredOwner(run: (fixture: {
  send(value: Record<string, unknown>): void;
  readonly engine: WorkerHandle;
  readonly posted: Record<string, unknown>[];
  readonly words: Int32Array;
  readonly control: MessagePort;
}) => Promise<void>, phase: "configured" | "registered" | "unallocated" = "configured") {
  const channels: MessageChannel[] = [];
  const createChannel = () => { const channel = new MessageChannel(); channels.push(channel); return channel; };
  const data = createChannel(), engine = new WorkerHandle(), posted: Record<string, unknown>[] = [];
  let listener!: (event: MessageEvent<unknown>) => void;
  createEngineControlOwner({
    createEngineWorker: () => engine as unknown as Worker,
    createChildWorker: () => new WorkerHandle() as unknown as Worker,
    createIngressWorker: () => new WorkerHandle() as unknown as Worker,
    createChannel,
    postMessage: (value) => { posted.push(value); },
    addMessageListener: (value) => { listener = value; },
    reportError: (error) => { throw error; },
  });
  const send = (value: Record<string, unknown>) => listener(new MessageEvent("message", { data: value }));
  send({ type: "start", instanceId: 1, runtimeOverride: false, dataPort: data.port2 });
  const control = engine.sent[0].controlPort as MessagePort;
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true }), words = new Int32Array(memory.buffer);
  Atomics.store(words, CAPACITY, 4); Atomics.store(words, GEN_BITS, 29); Atomics.store(words, FREE_HEAD, FREE_HEAD_EMPTY | 0);
  if (phase === "configured") control.postMessage({ type: "facadeEngineSubstrate", module: new WebAssembly.Module(new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0])), memory,
    layout: { baseAddr: 0, headerWords: HEADER_WORDS, slotStride: SLOT_STRIDE, joinStride: JOIN_STRIDE, capacity: 4, genBits: 29 }, diagnostics: false });
  else if (phase === "registered") control.postMessage({ type: "facadeEngineMemory", allocationId: 1, memory });
  try {
    for (let count = 0; count < 8; count++) await setImmediate();
    assert.equal(posted.some((value) => value.type === "configured"), phase === "configured");
    await run({ send, engine, posted, words, control });
  } finally {
    send({ type: "invalid", instanceId: 1 });
    for (const channel of channels) { channel.port1.close(); channel.port2.close(); }
  }
}

test("control owner 正常清场后发送同一 Memory 的终值收据", { timeout: 10000 }, async () => {
  await withConfiguredOwner(async ({ send, engine, posted }) => {
    send({ type: "beginShutdown", instanceId: 1, deadline: performance.timeOrigin + performance.now() + 2000 });
    engine.emit({ type: "engineLifecycle", instanceId: 1, state: "quiescent" });
    for (let count = 0; count < 8; count++) await setImmediate();
    const terminal = posted.find((value) => value.type === "terminal");
    assert.ok(terminal);
    assert.deepEqual(terminal.release, { kind: "released", finalBytes: 65536, growthStopped: true, referencesDropped: true });
    assert.equal(engine.terminations, 1);
    assert.equal(engine.listeners.size, 0);
  });
});

for (const phase of ["unallocated", "registered"] as const) {
  test(`control owner ${phase} 阶段初始化真实关闭后无需 substrate 也能确认空 child drain`, { timeout: 10000 }, async () => {
    await withConfiguredOwner(async ({ send, engine, posted }) => {
      send({ type: "beginShutdown", instanceId: 1, deadline: performance.timeOrigin + performance.now() + 2000 });
      engine.emit({ type: "engineLifecycle", instanceId: 1, state: phase === "unallocated" ? "unallocated" : "quiescent" });
      for (let count = 0; count < 8; count++) await setImmediate();
      const terminal = posted.find((value) => value.type === "terminal");
      assert.ok(terminal, "真实零 child/manifest/pending 不得卡在未配置 substrate 的 early return");
      assert.deepEqual(terminal.release, phase === "unallocated" ? { kind: "not-allocated" } : { kind: "released", finalBytes: 65536, growthStopped: true, referencesDropped: true });
      assert.equal(engine.terminations, 1);
    }, phase);
  });
}

test("control owner 将实际 runtimeName 与唯一 child Worker 身份一起观察", { timeout: 10000 }, async () => {
  await withConfiguredOwner(async ({ words, control, posted }) => {
    Atomics.store(words, slotFieldOffset(0, SLOT_JOIN_INDEX), 0);
    Atomics.store(words, joinWordOffset(0, 4), encodeJoin(0, JoinState.Waiting, 29) | 0);
    Atomics.store(words, slotWordOffset(0), encodeW(0, SlotState.Req, 29) | 0);
    control.postMessage({ type: "facadeSpawn", slot: 0, gen: 0, name: "rt-group-0" });
    for (let count = 0; count < 8; count++) await setImmediate();
    const spawn = posted.find((value) => value.type === "lifecycle" && (value.event as { type: string }).type === "childSpawn");
    assert.ok(spawn);
    assert.deepEqual(spawn.event, { type: "childSpawn", workerId: 1, slot: 0, gen: 0, name: "stock-game:1:child:1:0:0", runtimeName: "rt-group-0" });
  });
});
