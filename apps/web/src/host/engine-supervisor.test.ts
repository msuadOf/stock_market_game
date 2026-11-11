import assert from "node:assert/strict";
import test from "node:test";
import { createEngineSupervisor, allocateFacadeWorkerId, FacadeSpawnError } from "./engine-supervisor.ts";
import { BUMP, CAPACITY, GEN_BITS, FREE_HEAD, FREE_HEAD_EMPTY, FREE_LEN, HEADER_WORDS, SLOT_STRIDE, JOIN_STRIDE, SLOT_JOIN_INDEX, SLOT_ANSWER_B, H1_LOCK, AnswerField, SlotState, JoinState, encodeW, encodeJoin, slotWordOffset, slotFieldOffset, joinWordOffset } from "./facade-slot-protocol.ts";
import type { HostFailure } from "./host-update.ts";
import { parseHostFailure } from "./protocol-failure.ts";

class Port extends EventTarget {
  starts = 0;
  closes = 0;
  peer?: Port;
  start() { this.starts += 1; }
  close() { this.closes += 1; }
  postMessage(data: unknown) { this.peer?.emit(data); }
  emit(data: unknown) { this.dispatchEvent(new MessageEvent("message", { data })); }
}
class WorkerHandle extends EventTarget {
  terminations = 0;
  sent: unknown[] = [];
  beforePost?: () => void;
  postMessage(value: unknown) { this.beforePost?.(); this.sent.push(value); }
  terminate() { this.terminations += 1; }
}
class Clock {
  now = 0;
  nextId = 0;
  tasks = new Map<number, { at: number; callback: () => void }>();
  setTimeout = (callback: () => void, delay: number) => {
    const id = ++this.nextId;
    this.tasks.set(id, { at: this.now + delay, callback });
    return id;
  };
  clearTimeout = (id: number) => { this.tasks.delete(id); };
  advance(milliseconds: number) {
    const end = this.now + milliseconds;
    for (;;) {
      const item = [...this.tasks].filter(([, task]) => task.at <= end).sort((a, b) => a[1].at - b[1].at)[0];
      if (item === undefined) break;
      this.now = item[1].at;
      this.tasks.delete(item[0]);
      item[1].callback();
    }
    this.now = end;
  }
}
function fixture(options: { primaryDeathAddress?: number; constructError?: unknown; postError?: Error; channelErrorAt?: number; spawnCallbackError?: Error; fatalCallbackError?: Error; onFatal?: () => void; withoutReporter?: boolean; beforePost?: (child: WorkerHandle, channel: { port1: Port; port2: Port }) => void } = {}) {
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 4, shared: true });
  const layout = { baseAddr: 0, headerWords: HEADER_WORDS, slotStride: SLOT_STRIDE, joinStride: JOIN_STRIDE, capacity: 4, genBits: 29 };
  const words = new Int32Array(memory.buffer);
  Atomics.store(words, CAPACITY, layout.capacity);
  Atomics.store(words, GEN_BITS, layout.genBits);
  Atomics.store(words, FREE_HEAD, FREE_HEAD_EMPTY | 0);
  const engine = new WorkerHandle(), clock = new Clock();
  const children: WorkerHandle[] = [], channels: { port1: Port; port2: Port }[] = [];
  let omitInitialEngineChannel = options.primaryDeathAddress !== undefined;
  const fatal: HostFailure[] = [], failures: HostFailure[] = [];
  const reporterErrors: Error[] = [];
  const supervisor = createEngineSupervisor({
    engine: engine as unknown as Worker,
    createChildWorker: () => {
      if (options.constructError !== undefined) throw options.constructError;
      const child = new WorkerHandle(); children.push(child);
      child.beforePost = () => {
        options.beforePost?.(child, channels.at(-1)!);
        if (options.postError !== undefined) throw options.postError;
      };
      return child as unknown as Worker;
    },
    createChannel: () => {
      if (channels.length === options.channelErrorAt) throw new Error("Channel construct failed");
      const port1 = new Port(), port2 = new Port(); port1.peer = port2; port2.peer = port1;
      const channel = { port1, port2 };
      if (omitInitialEngineChannel) omitInitialEngineChannel = false;
      else channels.push(channel);
      return channel as unknown as MessageChannel;
    },
    onFailurePending: () => {},
    onFatalFailure: (failure) => { options.onFatal?.(); fatal.push(failure); if (options.fatalCallbackError !== undefined) throw options.fatalCallbackError; },
    onSpawnFailure: (failure) => { failures.push(failure); if (options.spawnCallbackError !== undefined) throw options.spawnCallbackError; },
    ...(options.withoutReporter === true ? {} : { reportError: (error: Error) => { reporterErrors.push(error); } }),
    timers: { now: () => clock.now, setTimeout: clock.setTimeout, clearTimeout: clock.clearTimeout },
  });
  if (options.primaryDeathAddress === undefined) {
    supervisor.installSubstrate({ module: new WebAssembly.Module(new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0])), memory, layout, diagnostics: false });
  } else {
    supervisor.bootstrapEngine({ type: "init", runtimeOverride: false });
    const control = (engine.sent.at(-1) as { controlPort: Port }).controlPort;
    control.postMessage({ type: "facadeEngineMemory", allocationId: 1, memory });
    control.postMessage({ type: "facadeEngineDeath", allocationId: 1, primary: {
      engine: { workerId: 0, address: 4200 }, ingress: { workerId: 0xffffffff, address: 4456 },
      death: { address: options.primaryDeathAddress, bytes: 4 },
    } });
    control.postMessage({ type: "facadeEngineConfiguration", allocationId: 1, module: new WebAssembly.Module(new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0])), layout, diagnostics: false });
  }
  function publish(slot: number, gen = 0) {
    Atomics.store(words, BUMP, slot + 1);
    Atomics.store(words, slotFieldOffset(slot, SLOT_JOIN_INDEX), slot);
    Atomics.store(words, joinWordOffset(slot, layout.capacity), encodeJoin(gen, JoinState.Waiting, layout.genBits) | 0);
    Atomics.store(words, slotWordOffset(slot), encodeW(gen, SlotState.Req, layout.genBits) | 0);
  }
  function spawn(slot = 0, gen = 0, source = 0) {
    publish(slot, gen);
    channels[source].port1.emit({ type: "facadeSpawn", slot, gen, name: "fixture" });
    return children.at(-1)!;
  }
  function ready(index = 0, slot = 0, gen = 0) {
    channels[index + 1].port1.emit({ type: "facadeChildReady", workerId: index + 1, slot, gen });
  }
  return { supervisor, memory, words, layout, engine, children, channels, clock, fatal, failures, reporterErrors, publish, spawn, ready };
}

test("main 端监听与 start 先于 child bootstrap，真实句柄在 ready 前登记", { timeout: 10000 }, () => {
  const f = fixture();
  f.spawn();
  assert.equal(f.channels[1].port1.starts, 1);
  assert.equal(f.supervisor.snapshot().children, 1);
  const bootstrap = f.children[0].sent[0] as Record<string, unknown>;
  assert.equal(bootstrap.controlPort, f.channels[1].port2);
  assert.equal(bootstrap.memory, f.memory);
  f.ready();
  assert.equal(Atomics.load(f.words, slotWordOffset(0)) >>> 0, encodeW(0, SlotState.Ok, 29));
  f.supervisor.dispose();
});

test("child 的 H5 使用新请求 slot/gen，能建立 grandchild", { timeout: 10000 }, () => {
  const f = fixture();
  f.spawn(); f.ready();
  f.spawn(1, 2, 1); f.ready(1, 1, 2);
  assert.equal(f.children.length, 2);
  assert.equal(f.fatal.length, 0);
  assert.equal(Atomics.load(f.words, slotWordOffset(1)) >>> 0, encodeW(2, SlotState.Ok, 29));
  f.supervisor.dispose();
});

test("ready 在 CANCELLED 后迟到：A1 零载荷写，child/port/计时器清理", { timeout: 10000 }, () => {
  const f = fixture(); f.spawn();
  Atomics.store(f.words, slotWordOffset(0), encodeW(0, SlotState.Cancelled, 29) | 0);
  Atomics.store(f.words, slotFieldOffset(0, AnswerField.WorkerId), 0xBEEF);
  f.ready();
  assert.equal(Atomics.load(f.words, slotFieldOffset(0, AnswerField.WorkerId)), 0xBEEF);
  assert.equal(f.children[0].terminations, 1);
  assert.equal(f.channels[1].port1.closes, 1);
  assert.equal(f.supervisor.snapshot().children, 0);
  f.supervisor.dispose(); assert.equal(f.clock.tasks.size, 0);
});

test("exit 先于 CONSUMED：无后续消息也恰一次回收，真实资源即刻摘除", { timeout: 10000 }, () => {
  const f = fixture(); f.spawn(); f.ready();
  Atomics.store(f.words, joinWordOffset(0, 4), encodeJoin(0, JoinState.Done, 29) | 0);
  f.channels[1].port1.emit({ type: "facadeWorkerExit", workerId: 1, slot: 0, gen: 0 });
  assert.equal(f.supervisor.snapshot().children, 0);
  assert.equal(f.supervisor.snapshot().protocolEntries, 1);
  assert.equal(f.channels[1].port1.closes, 1);
  Atomics.store(f.words, joinWordOffset(0, 4), encodeJoin(0, JoinState.Consumed, 29) | 0);
  f.clock.advance(100);
  assert.equal(Atomics.load(f.words, FREE_LEN), 1);
  assert.equal(f.supervisor.snapshot().protocolEntries, 0);
  f.clock.advance(100); assert.equal(Atomics.load(f.words, FREE_LEN), 1);
  f.supervisor.dispose();
});

test("bootstrap 发送前已监听：同步到达 ready 也不能遗漏", { timeout: 10000 }, () => {
  const f = fixture({ beforePost: (_child, channel) => {
    assert.equal(channel.port1.starts, 1);
    channel.port1.emit({ type: "facadeChildReady", workerId: 1, slot: 0, gen: 0 });
  } });
  f.spawn();
  assert.equal(Atomics.load(f.words, slotWordOffset(0)) >>> 0, encodeW(0, SlotState.Ok, 29));
  f.supervisor.dispose();
});

test("H1 延迟回收后清 removed 协议记录，重复 exit 零二次 push", { timeout: 10000 }, () => {
  const f = fixture(); f.spawn(); f.ready();
  Atomics.store(f.words, joinWordOffset(0, 4), encodeJoin(0, JoinState.Consumed, 29) | 0);
  Atomics.store(f.words, H1_LOCK, 1);
  const exit = { type: "facadeWorkerExit", workerId: 1, slot: 0, gen: 0 };
  f.channels[1].port1.emit(exit);
  assert.equal(f.supervisor.snapshot().pendingRecovery, 1);
  Atomics.store(f.words, H1_LOCK, 0); f.clock.advance(25);
  assert.equal(Atomics.load(f.words, FREE_LEN), 1);
  assert.equal(f.supervisor.snapshot().protocolEntries, 0);
  f.channels[1].port1.emit(exit); f.clock.advance(25);
  assert.equal(Atomics.load(f.words, FREE_LEN), 1);
  f.supervisor.dispose();
});

test("构造与 clone 失败走 FAIL，保留原始原因而不强杀健康 requester", { timeout: 10000 }, () => {
  for (const option of [{ constructError: new TypeError("construct denied") }, { postError: new DOMException("clone denied", "DataCloneError") }]) {
    const f = fixture(option); f.spawn();
    assert.equal(Atomics.load(f.words, slotWordOffset(0)) >>> 0, encodeW(0, SlotState.Fail, 29));
    assert.equal(f.failures.length, 1);
    assert.match(f.failures[0].message, /denied/);
    assert.equal(f.fatal.length, 0); assert.equal(f.engine.terminations, 0);
    if (f.children.length > 0) {
      assert.equal(f.children[0].terminations, 1);
      assert.equal(f.channels[1].port1.closes, 1); assert.equal(f.channels[1].port2.closes, 1);
    }
    f.supervisor.dispose();
  }
});

test("instantiate failure 保留结构化原始 HostFailure", { timeout: 10000 }, () => {
  const f = fixture(); f.spawn();
  const failure = { code: "ABI_MISMATCH", message: "imports differ", where: "fixture.instantiate", cause: { import: "env" } };
  f.channels[1].port1.emit({ type: "facadeBootstrapFailure", phase: "instantiate", workerId: 1, slot: 0, gen: 0, failure });
  assert.deepEqual(f.failures[0], failure);
  assert.equal(f.fatal.length, 0); assert.equal(f.children[0].terminations, 1);
  f.supervisor.dispose();
});

test("当前 child 不能 ready 前发 H5，身份伪造/全局成功消息均可见失败", { timeout: 10000 }, () => {
  for (const send of [
    (f: ReturnType<typeof fixture>) => { f.publish(1); f.channels[1].port1.emit({ type: "facadeSpawn", slot: 1, gen: 0, name: "early" }); },
    (f: ReturnType<typeof fixture>) => f.channels[1].port1.emit({ type: "facadeChildReady", workerId: 9, slot: 0, gen: 0 }),
    (f: ReturnType<typeof fixture>) => f.supervisor.handleGlobalMessage(f.children[0] as unknown as Worker, { type: "facadeChildReady", workerId: 1, slot: 0, gen: 0 }),
  ]) {
    const f = fixture(); f.spawn(); send(f);
    assert.equal(f.fatal.length, 1); assert.equal(f.supervisor.snapshot().workers, 0);
    assert.equal(f.clock.tasks.size, 0);
  }
});

test("已成立 child 的 trampoline failure 先发布 hardtrap 并保留原始错误", { timeout: 10000 }, () => {
  let stateAtFatal = -1;
  const f = fixture({ onFatal: () => { stateAtFatal = Atomics.load(f.words, joinWordOffset(0, 4)) >>> 0; } }); f.spawn(); f.ready();
  const failure = { code: "FACADE_BOOTSTRAP_FAILURE", where: "facade-child-bootstrap.trampoline", message: "unreachable" };
  f.channels[1].port1.emit({ type: "facadeBootstrapFailure", phase: "trampoline", workerId: 1, slot: 0, gen: 0, failure });
  assert.equal(stateAtFatal, encodeJoin(0, JoinState.DeadHardtrap, 29));
  assert.deepEqual(f.fatal, [{ ...failure, context: { last_committed_tick: "unknown" } }]);
});

test("trampoline failure 在 fatal 回调前通知对应 join", { timeout: 10000 }, () => {
  const originalNotify = Atomics.notify;
  const notified: number[] = [];
  let notifiedAtFatal: number[] = [];
  const f = fixture({ onFatal: () => { notifiedAtFatal = [...notified]; } }); f.spawn(); f.ready();
  try {
    Atomics.notify = (array, index, count) => {
      if (array.buffer === f.memory.buffer) notified.push(index);
      return array instanceof Int32Array ? originalNotify(array, index, count) : originalNotify(array, index, count);
    };
    const failure = { code: "FACADE_BOOTSTRAP_FAILURE", where: "facade-child-bootstrap.trampoline", message: "unreachable" };
    f.channels[1].port1.emit({ type: "facadeBootstrapFailure", phase: "trampoline", workerId: 1, slot: 0, gen: 0, failure });
    assert.ok(notifiedAtFatal.includes(joinWordOffset(0, 4)));
  } finally {
    Atomics.notify = originalNotify;
  }
});

test("trampoline failure 的死亡信号写入失败仍保留原始错误", { timeout: 10000 }, () => {
  const f = fixture(); f.spawn(); f.ready();
  const failure = { code: "FACADE_BOOTSTRAP_FAILURE", where: "facade-child-bootstrap.trampoline", message: "unreachable" };
  Atomics.store(f.words, slotFieldOffset(0, SLOT_JOIN_INDEX), 99);
  f.channels[1].port1.emit({ type: "facadeBootstrapFailure", phase: "trampoline", workerId: 1, slot: 0, gen: 0, failure });
  assert.equal(f.fatal[0].code, "FACADE_WORKER_PROTOCOL_ERROR");
  assert.deepEqual((f.fatal[0].context as { originalFailure: unknown }).originalFailure, failure);
});

test("全局 trampoline failure 抢先于 ready 时唤醒 spawn 并保留错误", { timeout: 10000 }, () => {
  let slotAtFatal = -1, notifiedAtFatal = false, notified = false;
  const f = fixture({ onFatal: () => { slotAtFatal = Atomics.load(f.words, slotWordOffset(0)) >>> 0; notifiedAtFatal = notified; } }); f.spawn();
  const failure = { code: "FACADE_BOOTSTRAP_FAILURE", where: "facade-child-bootstrap.trampoline", message: "unreachable" };
  const originalNotify = Atomics.notify;
  try {
    Atomics.notify = (array, index, count) => {
      if (array.buffer === f.memory.buffer && index === slotWordOffset(0)) notified = true;
      return array instanceof Int32Array ? originalNotify(array, index, count) : originalNotify(array, index, count);
    };
    f.supervisor.handleGlobalMessage(f.children[0] as unknown as Worker, { type: "facadeBootstrapFailure", phase: "trampoline", workerId: 1, slot: 0, gen: 0, failure });
  } finally { Atomics.notify = originalNotify; }
  assert.equal(slotAtFatal, encodeW(0, SlotState.Fail, 29));
  assert.equal(notifiedAtFatal, true);
  assert.equal(Atomics.load(f.words, slotFieldOffset(0, SLOT_ANSWER_B)), FacadeSpawnError.BOOTSTRAP_FAILURE);
  assert.deepEqual(f.fatal, [{ ...failure, context: { last_committed_tick: "unknown" } }]);
});

test("全局 trampoline failure 晚于取消时不覆盖槽", { timeout: 10000 }, () => {
  const f = fixture(); f.spawn();
  const cancelled = encodeW(0, SlotState.Cancelled, 29);
  Atomics.store(f.words, slotWordOffset(0), cancelled | 0);
  const before = [...f.words];
  const failure = { code: "FACADE_BOOTSTRAP_FAILURE", where: "facade-child-bootstrap.trampoline", message: "unreachable" };
  f.supervisor.handleGlobalMessage(f.children[0] as unknown as Worker, { type: "facadeBootstrapFailure", phase: "trampoline", workerId: 1, slot: 0, gen: 0, failure });
  assert.deepEqual([...f.words], before);
  assert.deepEqual(f.fatal, [{ ...failure, context: { last_committed_tick: "unknown" } }]);
});

test("DONE 后 cleanup 失败独立 fatal，结果不变并无条件清所有本人 Worker", { timeout: 10000 }, () => {
  const f = fixture(); f.spawn(); f.ready(); f.spawn(1); f.ready(1, 1);
  const ingress = new WorkerHandle(); f.supervisor.attachIngress(ingress as unknown as Worker);
  const done = encodeJoin(0, JoinState.Done, 29);
  Atomics.store(f.words, joinWordOffset(0, 4), done | 0);
  const failure = { code: "DESTROY_FAILED", where: "fixture.cleanup", message: "destroy failed" };
  f.channels[1].port1.emit({ type: "facadeBootstrapFailure", phase: "cleanup", workerId: 1, slot: 0, gen: 0, failure });
  assert.deepEqual(f.fatal, [{ ...failure, context: { last_committed_tick: "unknown" } }]);
  assert.equal(Atomics.load(f.words, joinWordOffset(0, 4)) >>> 0, done);
  for (const worker of [f.engine, ...f.children, ingress]) assert.equal(worker.terminations, 1);
  assert.equal(f.clock.tasks.size, 0); assert.equal(f.supervisor.snapshot().ports, 0);
  f.supervisor.dispose(); for (const worker of f.children) assert.equal(worker.terminations, 1);
});

test("exit ACK 只从首次 DONE 起算，不限制闭包寿命，且只收紧外层期限", { timeout: 10000 }, () => {
  const f = fixture(); f.spawn(); f.ready();
  f.clock.advance(15000); assert.equal(f.fatal.length, 0);
  Atomics.store(f.words, joinWordOffset(0, 4), encodeJoin(0, JoinState.Done, 29) | 0);
  f.supervisor.checkpoint(); f.clock.advance(9000); assert.equal(f.fatal.length, 0);
  f.supervisor.setDeadline(f.clock.now + 50); f.supervisor.setDeadline(f.clock.now + 5000);
  f.clock.advance(50); assert.equal(f.fatal.length, 1);
  assert.equal(f.fatal[0].code, "FACADE_EXIT_ACK_TIMEOUT");
  assert.equal(f.clock.tasks.size, 0);
});

test("shutdown H5 不建 child；dispose 后迟到事件零槽写且清所有计时器", { timeout: 10000 }, () => {
  const f = fixture(); f.supervisor.beginShutdown(); f.spawn();
  assert.equal(f.children.length, 0);
  assert.equal(Atomics.load(f.words, slotWordOffset(0)) >>> 0, encodeW(0, SlotState.Fail, 29));
  f.supervisor.dispose(); const before = [...f.words];
  f.channels[0].port1.emit({ type: "facadeSpawn", slot: 0, gen: 0, name: "late" });
  assert.deepEqual([...f.words], before); assert.equal(f.clock.tasks.size, 0);
});

test("Worker id 最大值可用，耗尽显式失败且不回绕", { timeout: 10000 }, () => {
  assert.deepEqual(allocateFacadeWorkerId(0xfffffffe), { workerId: 0xfffffffe, next: 0xffffffff });
  for (const value of [0, -1, 1.5, 0xffffffff, 0x100000000, NaN]) assert.throws(() => allocateFacadeWorkerId(value), /Worker id/);
});

test("Channel 构造抛错也清构造出的 child 并返回 FAIL", { timeout: 10000 }, () => {
  const f = fixture({ channelErrorAt: 1 }); f.spawn();
  assert.equal(f.children[0].terminations, 1);
  assert.equal(f.supervisor.snapshot().children, 0);
  assert.equal(f.failures.length, 1); assert.match(f.failures[0].message, /Channel construct failed/);
  assert.equal(f.fatal.length, 0); f.supervisor.dispose();
});

test("source port messageerror 与全局无身份 validate failure 有独立错误出口", { timeout: 10000 }, () => {
  const f = fixture(); f.spawn();
  f.channels[1].port1.dispatchEvent(new Event("messageerror"));
  assert.equal(f.fatal[0].code, "FACADE_CONTROL_ERROR"); assert.equal(f.clock.tasks.size, 0);
  const g = fixture(); g.spawn();
  const failure = { code: "BAD_BOOTSTRAP", where: "fixture.validate", message: "invalid payload" };
  assert.equal(g.supervisor.handleGlobalMessage(g.children[0] as unknown as Worker, { type: "facadeBootstrapFailure", phase: "validate", failure }), true);
  assert.deepEqual(g.failures, [failure]); assert.equal(g.fatal.length, 0);
  assert.equal(g.children[0].terminations, 1); g.supervisor.dispose();
});

test("Worker error preOK 只 FAIL；postOK D5 赢则 fatal，输 DONE 则等真实 exit", { timeout: 10000 }, () => {
  function error(worker: WorkerHandle) { worker.dispatchEvent(Object.assign(new Event("error"), { error: new Error("script trap"), message: "script trap" })); }
  const f = fixture(); f.spawn(); error(f.children[0]);
  assert.equal(f.failures[0].code, "SCRIPT_LOAD"); assert.equal(f.fatal.length, 0); f.supervisor.dispose();
  const g = fixture(); g.spawn(); g.ready(); error(g.children[0]);
  assert.equal(g.fatal.length, 1); assert.equal(g.supervisor.snapshot().workers, 0);
  const h = fixture(); h.spawn(); h.ready();
  const done = encodeJoin(0, JoinState.Done, 29); Atomics.store(h.words, joinWordOffset(0, 4), done | 0);
  error(h.children[0]); assert.equal(h.fatal.length, 0); assert.equal(Atomics.load(h.words, joinWordOffset(0, 4)) >>> 0, done);
  h.supervisor.checkpoint(); h.clock.advance(10000);
  assert.equal(h.fatal[0].code, "FACADE_EXIT_ACK_TIMEOUT");
});

test("spawn/fatal callback 自身抛错仍先清全部资源并有最终报告", { timeout: 10000 }, () => {
  const f = fixture({ spawnCallbackError: new Error("spawn callback broke"), fatalCallbackError: new Error("fatal callback broke") });
  f.spawn();
  f.children[0].dispatchEvent(Object.assign(new Event("error"), { error: new Error("load failed") }));
  assert.equal(f.supervisor.snapshot().workers, 0); assert.equal(f.clock.tasks.size, 0);
  assert.equal(f.reporterErrors.length, 1); assert.match(f.reporterErrors[0].message, /fatal/);
});

test("engine bootstrap 发送失败由 supervisor 清端和所有本人 Worker", { timeout: 10000 }, () => {
  const f = fixture(); f.spawn();
  f.engine.beforePost = () => { throw new DOMException("engine clone failed", "DataCloneError"); };
  f.supervisor.bootstrapEngine({ type: "init" });
  assert.equal(f.fatal[0].code, "FACADE_ENGINE_BOOTSTRAP");
  assert.equal(f.channels[0].port1.closes, 1); assert.equal(f.channels[0].port2.closes, 1);
  assert.equal(f.children[0].terminations, 1); assert.equal(f.clock.tasks.size, 0);
});

test("构造 HostFailure 与标准 Error.cause 原始详情不降格", { timeout: 10000 }, () => {
  const structured = { code: "WORKER_DENIED", where: "fixture.construct", message: "policy denied", cause: { permission: "worker-src" } };
  const f = fixture({ constructError: structured }); f.spawn();
  assert.deepEqual(f.failures[0], structured); f.supervisor.dispose();
  const cause = { reason: "origin policy", details: ["worker-src"] };
  const g = fixture({ constructError: new Error("construct failed", { cause }) }); g.spawn();
  assert.deepEqual((g.failures[0].cause as { cause: unknown }).cause, cause); g.supervisor.dispose();
});

test("关闭初始化在途 child 不等待 ready，迟到端消息不写 CANCELLED 槽", { timeout: 10000 }, () => {
  const f = fixture(); f.spawn();
  f.supervisor.beginShutdown();
  assert.equal(f.children[0].terminations, 1);
  assert.equal(f.supervisor.snapshot().children, 0);
  assert.equal(Atomics.load(f.words, slotWordOffset(0)) >>> 0, encodeW(0, SlotState.Fail, 29));
  const before = [...f.words]; f.ready(); assert.deepEqual([...f.words], before);
  f.supervisor.dispose();
});

test("ACK 外层期限在两个 sweep 之间也不延长，拒绝 NaN shutdown deadline", { timeout: 10000 }, () => {
  const f = fixture(); f.spawn(); f.ready();
  assert.throws(() => f.supervisor.beginShutdown(NaN), /deadline/);
  Atomics.store(f.words, joinWordOffset(0, 4), encodeJoin(0, JoinState.Done, 29) | 0);
  f.supervisor.checkpoint();
  f.supervisor.setDeadline(11); f.clock.advance(11);
  assert.equal(f.fatal.length, 1); assert.equal(f.clock.now, 11);
});

test("CANCELLED 初始化 child 无新 H5/ready 也由 sweep 终止，零槽写", { timeout: 10000 }, () => {
  const f = fixture(); f.spawn();
  Atomics.store(f.words, slotWordOffset(0), encodeW(0, SlotState.Cancelled, 29) | 0);
  const before = [...f.words]; f.clock.advance(25);
  assert.equal(f.children[0].terminations, 1); assert.equal(f.supervisor.snapshot().children, 0);
  assert.deepEqual([...f.words], before);
  f.ready(); assert.deepEqual([...f.words], before); f.supervisor.dispose();
});

test("terminate 目标必须已由本实例分配，过期合法目标保持幂等", { timeout: 10000 }, () => {
  const f = fixture(); f.spawn();
  f.channels[0].port1.emit({ type: "facadeTerminateChild", workerId: 0xffffffff });
  assert.equal(f.fatal.length, 1); assert.equal(f.supervisor.snapshot().workers, 0);
  const g = fixture(); g.spawn();
  Atomics.store(g.words, slotWordOffset(0), encodeW(0, SlotState.Cancelled, 29) | 0);
  g.channels[0].port1.emit({ type: "facadeTerminateChild", workerId: 1 });
  g.channels[0].port1.emit({ type: "facadeTerminateChild", workerId: 1 });
  assert.equal(g.children[0].terminations, 1); assert.equal(g.fatal.length, 0); g.supervisor.dispose();
});

test("engine Error.message getter 抛错仍清全部本人资源并显式报读取失败", { timeout: 10000 }, () => {
  const f = fixture();
  const error = new Error("original"); Object.defineProperty(error, "message", { get() { throw new Error("message getter broke"); } });
  f.engine.dispatchEvent(Object.assign(new Event("error"), { error }));
  assert.equal(f.supervisor.snapshot().workers, 0); assert.equal(f.clock.tasks.size, 0);
  assert.equal(f.fatal[0].code, "FACADE_FAILURE_REPORTER_ERROR");
  assert.match(f.fatal[0].message, /message getter broke/);
});

test("已 OK 的 terminate 必须废弃实例，不能伪造 exit 或覆盖 DONE", { timeout: 10000 }, () => {
  const f = fixture(); f.spawn(); f.ready();
  const done = encodeJoin(0, JoinState.Done, 29); Atomics.store(f.words, joinWordOffset(0, 4), done | 0);
  f.channels[0].port1.emit({ type: "facadeTerminateChild", workerId: 1 });
  assert.equal(f.fatal[0].code, "FACADE_ESTABLISHED_CHILD_TERMINATED");
  assert.equal(f.supervisor.snapshot().workers, 0); assert.equal(Atomics.load(f.words, FREE_LEN), 0);
  assert.equal(Atomics.load(f.words, joinWordOffset(0, 4)) >>> 0, done);
});

test("DONE 后双发送失败 marker 经 native error 独立 fatal，含 null error 的浏览器形态", { timeout: 10000 }, () => {
  const message = "[FACADE_BOOTSTRAP_REPORTER_FAILURE] FACADE_BOOTSTRAP_FAILURE（fixture.cleanup）：destroy failed；控制：clone denied；全局：clone denied";
  for (const shape of [
    { error: new AggregateError([new Error("destroy failed")], message), message: "" },
    { error: null, message: `Uncaught AggregateError: ${message}` },
  ]) {
    const f = fixture(); f.spawn(); f.ready();
    const done = encodeJoin(0, JoinState.Done, 29); Atomics.store(f.words, joinWordOffset(0, 4), done | 0);
    f.children[0].dispatchEvent(Object.assign(new Event("error"), shape));
    assert.equal(f.fatal.length, 1); assert.equal(f.fatal[0].code, "FACADE_BOOTSTRAP_REPORTER_FAILURE");
    assert.match(f.fatal[0].message, /destroy failed/);
    assert.equal(f.supervisor.snapshot().workers, 0); assert.equal(f.clock.tasks.size, 0);
    assert.equal(Atomics.load(f.words, joinWordOffset(0, 4)) >>> 0, done);
  }
});

test("removed 抢 H1 失败而同次 pending 成功，精确清已回收 pair 的协议登记", { timeout: 10000 }, () => {
  const f = fixture(); f.spawn(); f.ready();
  Atomics.store(f.words, joinWordOffset(0, 4), encodeJoin(0, JoinState.Consumed, 29) | 0);
  Atomics.store(f.words, H1_LOCK, 1);
  const original = Atomics.compareExchange; let attempts = 0;
  try {
    Atomics.compareExchange = ((words: Int32Array, index: number, expected: number, replacement: number) => {
      const previous = original(words, index, expected, replacement);
      if (words.buffer === f.words.buffer && index === H1_LOCK && expected === 0 && replacement === 1 && ++attempts === 2) Atomics.store(f.words, H1_LOCK, 0);
      return previous;
    }) as typeof Atomics.compareExchange;
    f.channels[1].port1.emit({ type: "facadeWorkerExit", workerId: 1, slot: 0, gen: 0 });
  } finally { Atomics.compareExchange = original; }
  f.clock.advance(100);
  assert.equal(Atomics.load(f.words, FREE_LEN), 1); assert.equal(f.supervisor.snapshot().pendingRecovery, 0);
  assert.equal(f.supervisor.snapshot().protocolEntries, 0); f.supervisor.dispose();
});

test("ACK timer 触发 fatal callback 抛错且 reporter 不可用时不静默吞错", { timeout: 10000 }, () => {
  const callbackError = new Error("timer fatal callback failed");
  const f = fixture({ withoutReporter: true, fatalCallbackError: callbackError }); f.spawn(); f.ready();
  Atomics.store(f.words, joinWordOffset(0, 4), encodeJoin(0, JoinState.Done, 29) | 0); f.supervisor.checkpoint();
  assert.throws(() => f.clock.advance(10000), (error) => error === callbackError);
  assert.equal(f.supervisor.snapshot().workers, 0); assert.equal(f.clock.tasks.size, 0);
});

test("postOK 原生错误遇损坏 joinIndex，先全员清理再报告协议错误", { timeout: 10000 }, () => {
  const f = fixture(); f.spawn(); f.ready();
  Atomics.store(f.words, slotFieldOffset(0, SLOT_JOIN_INDEX), 0xffffffff | 0);
  f.children[0].dispatchEvent(Object.assign(new Event("error"), { error: new Error("trap with corrupt index") }));
  assert.equal(f.supervisor.snapshot().workers, 0); assert.equal(f.clock.tasks.size, 0);
  assert.equal(f.fatal.length, 1);
});

test("空 Error.message 仍产生可解析的显式 HostFailure 并保留原详情", { timeout: 10000 }, () => {
  for (const constructError of [new Error(""), ""]) {
    const f = fixture({ constructError }); f.spawn();
    assert.doesNotThrow(() => parseHostFailure(f.failures[0]));
    assert.match(f.failures[0].message, /非空 message/); f.supervisor.dispose();
  }
});

test("真实 trap 后 D5 二级协议错误保留首次原 trap/cause 快照", { timeout: 10000 }, () => {
  const f = fixture(); f.spawn(); f.ready();
  Atomics.store(f.words, slotFieldOffset(0, SLOT_JOIN_INDEX), 0xffffffff | 0);
  f.children[0].dispatchEvent(Object.assign(new Event("error"), { error: new Error("ROOT_TRAP_993", { cause: { wasmFrame: "original-wasm-frame" } }), message: "native trap detail" }));
  assert.equal(f.supervisor.snapshot().workers, 0);
  assert.match(JSON.stringify(f.fatal[0]), /ROOT_TRAP_993/);
  assert.match(JSON.stringify(f.fatal[0]), /original-wasm-frame/);
  assert.match(JSON.stringify(f.fatal[0]), /native trap detail/);
  assert.match(JSON.stringify(f.fatal[0]), /RangeError/);
});

test("正常 drain 必须等真实 childExit，join CONSUMED 不能替代且不提前 terminate", { timeout: 10000 }, async () => {
  const f = fixture(); f.spawn(); f.ready();
  const pending = f.supervisor.waitForChildrenExit(1000); let resolved = false;
  void pending.then(() => { resolved = true; });
  Atomics.store(f.words, joinWordOffset(0,4), encodeJoin(0,JoinState.Consumed,29) | 0);
  f.supervisor.checkpoint(); await Promise.resolve();
  assert.equal(resolved, false, "CONSUMED_IS_NOT_CHILD_EXIT");
  assert.equal(f.children[0].terminations, 0);
  f.channels[1].port1.emit({ type: "facadeWorkerExit", workerId: 1, slot: 0, gen: 0 });
  await pending;
  assert.equal(f.supervisor.snapshot().protocolEntries, 0);
  assert.equal(f.engine.terminations, 0);
  f.supervisor.dispose();
});

test("强制 dispose 清 manifest 不得把真实 drain waiter 改成成功", { timeout: 10000 }, async () => {
  const f = fixture(); f.spawn(); f.ready();
  const pending = f.supervisor.waitForChildrenExit(1000);
  f.supervisor.dispose();
  await assert.rejects(pending, /dispose|废弃|终止/);
});

test("V2 child trap 报告本 worker 的已发布 tombstone", { timeout: 10000 }, () => {
  const f = fixture(); f.publish(0);
  f.channels[0].port1.emit({ type: "facadeSpawnV2", slot: 0, gen: 0, name: "fixture", recordAddr: 8192 });
  f.ready();
  const bytes = new Uint8Array(f.memory.buffer, 8192, 256);
  const words = new Uint32Array(f.memory.buffer, 8192, 4);
  const halves = new Uint16Array(f.memory.buffer, 8192, 8);
  words[1] = 1; halves[4] = 3; bytes[10] = 1; halves[6] = 4; bytes.set(new TextEncoder().encode("oops"), 16); Atomics.store(words, 0, 2);
  const event = new Event("error"); Object.assign(event, { message: "trap", error: new Error("trap") });
  f.children[0].dispatchEvent(event);
  assert.equal(f.fatal.length, 1);
  assert.deepEqual((f.fatal[0].context as Record<string, unknown>).tombstone, { state: "live", kind: "panic", phase: 3, message: "oops", truncated: false });
});

test("V2 记录地址不能越界、重叠 arena 或被不同 spawn 重用", { timeout: 10000 }, () => {
  for (const recordAddr of [0, 1, 4, 65500]) {
    const f = fixture(); f.publish(0);
    f.channels[0].port1.emit({ type: "facadeSpawnV2", slot: 0, gen: 0, name: "fixture", recordAddr });
    assert.equal(f.fatal.length, 1); assert.equal(f.children.length, 0);
  }
  const f = fixture(); f.publish(0);
  f.channels[0].port1.emit({ type: "facadeSpawnV2", slot: 0, gen: 0, name: "fixture", recordAddr: 8192 });
  f.ready(); f.publish(1);
  f.channels[0].port1.emit({ type: "facadeSpawnV2", slot: 1, gen: 0, name: "fixture", recordAddr: 8192 });
  assert.equal(f.fatal.length, 1); assert.equal(f.children.length, 1);
});

test("V2 损坏记录保留原错误，未发布记录维持 hardtrap", { timeout: 10000 }, () => {
  for (const magic of [0, 1, 7]) {
    const f = fixture(); f.publish(0);
    f.channels[0].port1.emit({ type: "facadeSpawnV2", slot: 0, gen: 0, name: "fixture", recordAddr: 8192 });
    f.ready(); Atomics.store(new Uint32Array(f.memory.buffer, 8192, 4), 0, magic);
    const event = new Event("error"); Object.assign(event, { message: "trap", error: new Error("original trap") });
    f.children[0].dispatchEvent(event);
    assert.equal(f.fatal[0].message, "original trap");
    const context = f.fatal[0].context as Record<string, unknown>;
    if (magic === 7) assert.match(context.tombstoneReadError as string, /magic/);
    else assert.deepEqual(context.tombstone, { state: magic === 0 ? "absent" : "writing" });
    assert.equal(Atomics.load(f.words, joinWordOffset(0, 4)) >>> 0, encodeJoin(0, JoinState.DeadHardtrap, 29));
  }
});

test("过期 V2 spawn 不登记地址，当前代可以使用该地址", { timeout: 10000 }, () => {
  const f = fixture(); f.publish(0, 1);
  f.channels[0].port1.emit({ type: "facadeSpawnV2", slot: 0, gen: 0, name: "stale", recordAddr: 8192 });
  f.channels[0].port1.emit({ type: "facadeSpawnV2", slot: 0, gen: 1, name: "current", recordAddr: 8192 });
  assert.equal(f.children.length, 1); assert.equal(f.fatal.length, 0); f.supervisor.dispose();
});

test("Rust 先发布 DeadPanic 时仍报告 child fatal 和记录", { timeout: 10000 }, () => {
  const f = fixture(); f.publish(0);
  f.channels[0].port1.emit({ type: "facadeSpawnV2", slot: 0, gen: 0, name: "fixture", recordAddr: 8192 });
  f.ready(); Atomics.store(f.words, joinWordOffset(0, 4), encodeJoin(0, JoinState.DeadPanic, 29));
  const event = new Event("error"); Object.assign(event, { message: "panic", error: new Error("panic") });
  f.children[0].dispatchEvent(event);
  assert.equal(f.fatal.length, 1);
  assert.deepEqual((f.fatal[0].context as Record<string, unknown>).tombstone, { state: "absent" });
});


test("全局死亡通知先于记录发布时显式报告未发布状态并终止实例", { timeout: 10000 }, async () => {
  const f = fixture({ primaryDeathAddress: 16384 }); f.publish(0);
  f.channels[0].port1.emit({ type: "facadeSpawnV2", slot: 0, gen: 0, name: "fixture", recordAddr: 8192 });
  f.ready();
  const death = new Int32Array(f.memory.buffer, 16384, 1);
  Atomics.store(death, 0, 1); Atomics.notify(death, 0);
  await new Promise<void>((resolve) => setTimeout(resolve, 30));
  assert.equal(f.fatal.length, 0);
  f.supervisor.setDeadline(f.clock.now + 100); f.clock.advance(100);
  assert.equal(f.fatal.length, 1);
  assert.equal(f.fatal[0].code, "WASM_WORKER_DEATH");
  assert.deepEqual((f.fatal[0].context as { records: Array<{ tombstone: unknown }> }).records[0].tombstone, { state: "absent" });
  assert.equal(f.supervisor.snapshot().disposed, true);
});

test("dispose 后迟到的全局死亡通知不得报告", { timeout: 10000 }, async () => {
  const f = fixture({ primaryDeathAddress: 16384 }); f.supervisor.dispose();
  const death = new Int32Array(f.memory.buffer, 16384, 1);
  Atomics.store(death, 0, 1); Atomics.notify(death, 0);
  await new Promise<void>((resolve) => setTimeout(resolve, 30));
  assert.equal(f.fatal.length, 0);
});

test("报告器故障保留 V2 tombstone 快照", { timeout: 10000 }, () => {
  const f = fixture(); f.publish(0);
  f.channels[0].port1.emit({ type: "facadeSpawnV2", slot: 0, gen: 0, name: "fixture", recordAddr: 8192 });
  f.ready();
  const event = new Event("error"); Object.assign(event, { message: "FACADE_BOOTSTRAP_REPORTER_FAILURE", error: new Error("reporter failure") });
  f.children[0].dispatchEvent(event);
  assert.equal(f.fatal[0].code, "FACADE_BOOTSTRAP_REPORTER_FAILURE");
  assert.deepEqual((f.fatal[0].context as Record<string, unknown>).tombstone, { state: "absent" });
});

test("死亡宽限内 child 错误优先，取消延迟报告", { timeout: 10000 }, async () => {
  const f = fixture({ primaryDeathAddress: 16384 }); f.publish(0);
  f.channels[0].port1.emit({ type: "facadeSpawnV2", slot: 0, gen: 0, name: "fixture", recordAddr: 8192 });
  f.ready();
  const death = new Int32Array(f.memory.buffer, 16384, 1);
  Atomics.store(death, 0, 1); Atomics.notify(death, 0);
  await new Promise<void>((resolve) => setTimeout(resolve, 30));
  const event = new Event("error"); Object.assign(event, { message: "trap", error: new Error("原始 child 故障") });
  f.children[0].dispatchEvent(event);
  assert.equal(f.fatal.length, 0);
  f.supervisor.setDeadline(f.clock.now + 100); f.clock.advance(100);
  assert.equal(f.fatal.length, 1); assert.equal(f.fatal[0].code, "WASM_WORKER_DEATH");
});

test("DEAD 布局不得与 arena 或 abort 重叠", { timeout: 10000 }, () => {
  const f = fixture({ primaryDeathAddress: 4 });
  assert.equal(f.fatal.length, 1);
  assert.match(f.fatal[0].message, /重叠/);
  f.supervisor.dispose();
});

test("DEAD 后 writer 可完成发布，期限前不终止并保留首错", { timeout: 10000 }, async () => {
  const f = fixture({ primaryDeathAddress: 16384 }); f.publish(0);
  f.channels[0].port1.emit({ type: "facadeSpawnV2", slot: 0, gen: 0, name: "fixture", recordAddr: 8192 }); f.ready();
  const words = new Uint32Array(f.memory.buffer, 8192, 4), halves = new Uint16Array(f.memory.buffer, 8192, 8), bytes = new Uint8Array(f.memory.buffer, 8192, 256);
  Atomics.store(words, 0, 1);
  const death = new Int32Array(f.memory.buffer, 16384, 1); Atomics.store(death, 0, 1); Atomics.notify(death, 0);
  await new Promise<void>((resolve) => setTimeout(resolve, 30));
  f.supervisor.setDeadline(f.clock.now + 100); f.clock.advance(99);
  assert.equal(f.children[0].terminations, 0); assert.equal(f.fatal.length, 0);
  words[1] = 1; bytes[10] = 1; halves[6] = 4; bytes.set(new TextEncoder().encode("oops"), 16); Atomics.store(words, 0, 2);
  f.clock.advance(1);
  assert.equal(f.fatal[0].code, "WASM_WORKER_DEATH");
  assert.deepEqual((f.fatal[0].context as { records: Array<{ tombstone: unknown }> }).records[0].tombstone, { state: "live", kind: "panic", phase: 0, message: "oops", truncated: false });
});

test("failurePending 保留首错并追加清理错误", { timeout: 10000 }, async () => {
  const f = fixture({ primaryDeathAddress: 16384 });
  const death = new Int32Array(f.memory.buffer, 16384, 1); Atomics.store(death, 0, 1); Atomics.notify(death, 0);
  await new Promise<void>((resolve) => setTimeout(resolve, 30));
  f.supervisor.noteFailureDuringPending({ code: "CLEANUP", where: "fixture", message: "清理失败" });
  f.supervisor.setDeadline(f.clock.now + 1); f.clock.advance(1);
  assert.equal(f.fatal[0].code, "WASM_WORKER_DEATH");
  assert.equal((f.fatal[0].context as { subsequentFailures: HostFailure[] }).subsequentFailures[0].code, "CLEANUP");
});

test("已到期的终态清理同步释放 Worker 并保留首错", { timeout: 10000 }, () => {
  const f = fixture({ primaryDeathAddress: 16384 }); f.spawn(); f.ready();
  f.supervisor.setDeadline(f.clock.now);
  f.supervisor.finishFailureAtDeadline({ code: "TIMEOUT", where: "fixture", message: "清理超时" });
  assert.equal(f.children[0].terminations, 1);
  assert.equal(f.fatal[0].code, "TIMEOUT");
  assert.equal(f.supervisor.snapshot().disposed, true);
});

test("最终失败明确声明 last_committed_tick 未知并保留原诊断", { timeout: 10000 }, () => {
  const f = fixture(); f.spawn(); f.ready();
  const event = new Event("error"); Object.assign(event, { message: "trap", error: new Error("trap") });
  f.children[0].dispatchEvent(event);
  const context = f.fatal[0].context as Record<string, unknown>;
  assert.equal(context.last_committed_tick, "unknown");
  assert.equal(context.nativeEventMessage, "trap");
});

test("已接管的 Worker error 阻止冒泡并保留故障与清理", { timeout: 10000 }, () => {
  for (const role of ["engine", "child"] as const) {
    for (const primaryDeathAddress of [undefined, 16384]) {
      const f = fixture({ primaryDeathAddress });
      const worker = role === "engine" ? f.engine : f.spawn();
      if (role === "child") f.ready();
      const event = Object.assign(new Event("error", { cancelable: true }), { error: new Error("原始 trap"), message: "原始 trap" });
      worker.dispatchEvent(event);
      assert.equal(event.defaultPrevented, true);
      if (primaryDeathAddress === undefined) assert.equal(f.fatal.length, 1);
      f.supervisor.dispose();
      assert.equal(worker.terminations, 1);
    }
  }
});
