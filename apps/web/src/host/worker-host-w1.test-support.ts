import assert from "node:assert/strict";
import { catalogWorkerFixture } from "./catalog-test-fixture.ts";
import { setImmediate } from "node:timers/promises";
import { MessageChannel as NodeMessageChannel } from "node:worker_threads";
import { createWorkerHost } from "./worker-host.ts";
import type { HostFailure } from "./host-update.ts";
import { snapshot } from "./protocol-test-fixtures.ts";
import { CAPACITY, GEN_BITS, FREE_HEAD, FREE_HEAD_EMPTY, HEADER_WORDS, SLOT_STRIDE, JOIN_STRIDE, SLOT_JOIN_INDEX, SlotState, JoinState, encodeW, encodeJoin, slotWordOffset, slotFieldOffset, joinWordOffset } from "./facade-slot-protocol.ts";

export async function drain() { for (let count = 0; count < 8; count++) await setImmediate(); }
export class Clock {
  now = 0;
  sequence = 0;
  readonly tasks = new Map<number, { at: number; callback: () => void }>();
  readonly registrations: number[] = [];
  setTimeout = (callback: () => void, milliseconds: number) => {
    const id = ++this.sequence;
    this.tasks.set(id, { at: this.now + milliseconds, callback });
    this.registrations.push(this.now + milliseconds);
    return id;
  };
  clearTimeout = (id: number) => { this.tasks.delete(id); };
  advance(milliseconds: number) {
    const until = this.now + milliseconds;
    for (;;) {
      const task = [...this.tasks].filter(([, value]) => value.at <= until).sort((left, right) => left[1].at - right[1].at)[0];
      if (task === undefined) break;
      this.tasks.delete(task[0]); this.now = task[1].at; task[1].callback();
    }
    this.now = until;
  }
}
export class WorkerHandle extends EventTarget {
  readonly sent: Record<string, unknown>[] = [];
  readonly transfers: unknown[][] = [];
  readonly listeners = new Map<string, Set<EventListenerOrEventListenerObject>>();
  terminations = 0;
  controlPort?: MessagePort;
  onPost?: (value: Record<string, unknown>, transfer: unknown[]) => void;
  override addEventListener(type: string, callback: EventListenerOrEventListenerObject | null, options?: AddEventListenerOptions | boolean) {
    if (callback !== null) {
      let listeners = this.listeners.get(type);
      if (listeners === undefined) { listeners = new Set(); this.listeners.set(type, listeners); }
      listeners.add(callback);
    }
    super.addEventListener(type, callback, options);
  }
  override removeEventListener(type: string, callback: EventListenerOrEventListenerObject | null, options?: EventListenerOptions | boolean) {
    if (callback !== null) this.listeners.get(type)?.delete(callback);
    super.removeEventListener(type, callback, options);
  }
  postMessage(value: Record<string, unknown>, transfer: unknown[] = []) {
    this.sent.push(value); this.transfers.push(transfer);
    if (value.controlPort instanceof MessagePort) this.controlPort = value.controlPort;
    this.onPost?.(value, transfer);
  }
  terminate() { this.terminations++; }
  emit(value: unknown) { this.dispatchEvent(new MessageEvent("message", { data: value })); }
  error(error: unknown, message = "脚本失败") { this.dispatchEvent(Object.assign(new Event("error"), { error, message })); }
  listenerCount() { return [...this.listeners.values()].reduce((count, listeners) => count + listeners.size, 0); }
}
export type Host = Awaited<ReturnType<typeof createWorkerHost>>;
export type Outcome = { status: "pending" } | { status: "fulfilled"; host: Host } | { status: "rejected"; error: unknown };
export const setup = { stocks: [{ code: "000812" }] } as Parameters<typeof createWorkerHost>[0];
const capabilities = { npcDecisionDiagnostics: false, personalTradeHistory: false, indicatorCapabilities: { intradayAverage: false, macd: false, priceKdj: false, candleKdj: false } };

/** 同时支持 pre-host init 与新 bootstrap；ready 只有新控制端真实 ACK 后才产生。 */
export function fixture(options: { initError?: unknown; initAfterMemoryFailure?: unknown; initBeforeMemoryFailure?: unknown; claimUnallocatedAfterMemory?: boolean; engineConstructError?: unknown; channelError?: boolean; childConstructError?: unknown; spawnSink?: (failure: HostFailure) => void; beforeInitPost?: (clock: Clock) => void } = {}) {
  const originals = new Map<string, PropertyDescriptor | undefined>();
  function install(name: string, value: unknown) {
    originals.set(name, Object.getOwnPropertyDescriptor(globalThis, name));
    Object.defineProperty(globalThis, name, { configurable: true, writable: true, value });
  }
  const clock = new Clock();
  const workers: WorkerHandle[] = [], channels: NodeMessageChannel[] = [], portCloses = new Map<object, number>();
  const failures: HostFailure[] = [];
  const module = new WebAssembly.Module(new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0]));
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true });
  const layout = { baseAddr: 0, headerWords: HEADER_WORDS, slotStride: SLOT_STRIDE, joinStride: JOIN_STRIDE, capacity: 4, genBits: 29 };
  const words = new Int32Array(memory.buffer);
  Atomics.store(words, CAPACITY, layout.capacity); Atomics.store(words, GEN_BITS, layout.genBits); Atomics.store(words, FREE_HEAD, FREE_HEAD_EMPTY | 0);
  const substrate = { module, memory, layout, diagnostics: false };
  let configured = 0;
  let configuredSubstrate: typeof substrate | undefined;
  class FixtureChannel extends NodeMessageChannel {
    constructor() {
      if (options.channelError === true) throw new Error("Channel 构造失败");
      super(); channels.push(this);
      this.port1.addEventListener("message", (event) => {
        const data = (event as MessageEvent).data;
        if (data.type === "facadeEngineSubstrate") configuredSubstrate = data as typeof substrate;
      });
      for (const port of [this.port1, this.port2]) {
        const close = port.close.bind(port);
        port.close = () => { portCloses.set(port, (portCloses.get(port) ?? 0) + 1); close(); };
      }
    }
  }
  class FixtureWorker extends WorkerHandle {
    readonly kind: "engine" | "ingress" | "child";
    constructor(url: URL) {
      super();
      this.kind = url.pathname.endsWith("wasm-worker.ts") ? "engine" : url.pathname.endsWith("wasm-ingress-worker.ts") ? "ingress" : "child";
      if (this.kind === "engine" && options.engineConstructError !== undefined) throw options.engineConstructError;
      if (this.kind === "child" && options.childConstructError !== undefined) throw options.childConstructError;
      workers.push(this);
      this.onPost = (value) => {
        if (value.type === "init") {
          options.beforeInitPost?.(clock);
          if (options.initBeforeMemoryFailure !== undefined) {
            this.emit({ type: "failure", generation: 0, code: "PACKAGE_FIXTURE", where: "fixture.package", message: String(options.initBeforeMemoryFailure), engineQuiescent: "initialization-failed", memoryAllocation: "not-allocated" });
            return;
          }
          if (options.initError !== undefined) throw options.initError;
          if (this.controlPort !== undefined) {
            this.controlPort.addEventListener("message", (event) => {
              if ((event.data as { type?: string }).type === "facadeEngineMemoryRegistered" && options.initAfterMemoryFailure !== undefined) {
                memory.grow(1);
                this.emit({ type: "failure", generation: 0, code: "INIT_FIXTURE", where: "fixture.init", message: String(options.initAfterMemoryFailure), engineQuiescent: "initialization-failed", ...(options.claimUnallocatedAfterMemory === true ? { memoryAllocation: "not-allocated" } : {}) });
              }
              if ((event.data as { type?: string }).type === "facadeEngineConfigured") { configured++; this.emit({ type: "ready" }); }
            });
            this.controlPort.start();
            this.controlPort.postMessage(options.initAfterMemoryFailure === undefined ? { type: "facadeEngineSubstrate", ...substrate } : { type: "facadeEngineMemory", allocationId: 1, memory });
          } else queueMicrotask(() => this.emit({ type: "ready" }));
        }
        if (value.type === "shutdown") queueMicrotask(() => this.emit({ type: "shutdownCompleted", requestId: value.requestId, generation: value.generation, report: { kind: "success", handle: 7, runtime: null } }));
        if (value.type === "quiesceIngress") queueMicrotask(() => this.emit({ type: "ingressQuiescent", requestId: value.requestId, generation: value.generation }));
        if (value.type === "create") queueMicrotask(() => this.emit({ type: "created", generation: 1, capabilities }));
      };
    }
  }
  install("Worker", FixtureWorker); install("MessageChannel", FixtureChannel);
  install("setTimeout", clock.setTimeout); install("clearTimeout", clock.clearTimeout);
  install("window", { setTimeout: clock.setTimeout, clearTimeout: clock.clearTimeout });
  const sink = (failure: HostFailure) => { failures.push(failure); options.spawnSink?.(failure); };
  let outcome: Outcome = { status: "pending" };
  function launch(...args: [] | [unknown]) {
    const hostOptions = args.length === 0 ? { onSpawnFailure: sink } : args[0];
    const creating = createWorkerHost(setup, 42n, hostOptions as Parameters<typeof createWorkerHost>[2]);
    void creating.then((host) => { outcome = { status: "fulfilled", host }; }, (error: unknown) => { outcome = { status: "rejected", error }; });
    return creating;
  }
  function baseline(generation = 1) { workers[0].emit({ ...catalogWorkerFixture(snapshot(generation, generation)), type: "baseline", civilDate: "2030-01-02", generation, snapshot: snapshot(generation, generation) }); }
  function ingress(generation = 1, token: unknown = generation, extra: Record<string, unknown> = {}) { workers[0].emit({ type: "ingress", generation, token, shared: true, ...extra }); }
  function ack(worker: WorkerHandle, generation: number, failure?: string) {
    const request = worker.sent.findLast((item) => item.type === "bindIngress" && item.generation === generation);
    if (request === undefined) throw new Error(`fixture 缺少 generation ${generation} bindIngress`);
    worker.emit({ type: failure === undefined ? "ingressBound" : "operationError", requestId: request.requestId, generation, ...(failure === undefined ? {} : { message: failure }) });
  }
  function spawn(slot = 0) {
    Atomics.store(words, slotFieldOffset(slot, SLOT_JOIN_INDEX), slot);
    Atomics.store(words, joinWordOffset(slot, layout.capacity), encodeJoin(0, JoinState.Waiting, 29) | 0);
    Atomics.store(words, slotWordOffset(slot), encodeW(0, SlotState.Req, 29) | 0);
    // pre-host 没有控制端：仍投递普通消息，以实际 sink 缺失断言呈现行为红。
    if (workers[0].controlPort === undefined) workers[0].emit({ type: "facadeSpawn", slot, gen: 0, name: "fixture" });
    else workers[0].controlPort.postMessage({ type: "facadeSpawn", slot, gen: 0, name: "fixture" });
  }
  async function cleanup(expectedDisposalFailure?: RegExp) {
    if (outcome.status === "fulfilled") {
      if (expectedDisposalFailure === undefined) await outcome.host.dispose();
      else await assert.rejects(outcome.host.dispose(), expectedDisposalFailure);
    }
    else if (outcome.status === "pending" && workers[0] !== undefined) { workers[0].emit({ type: "failure", code: "FIXTURE_CLEANUP", where: "fixture", message: "测试收尾" }); await drain(); }
    // fixture 的防泄漏兜底发生在产品清理断言之后，不能用它证明产品已清理。
    for (const channel of channels) { channel.port1.close(); channel.port2.close(); }
    for (const worker of workers) if (worker.terminations === 0) worker.terminate();
    clock.tasks.clear();
    for (const [name, descriptor] of originals) {
      if (descriptor === undefined) Reflect.deleteProperty(globalThis, name);
      else Object.defineProperty(globalThis, name, descriptor);
    }
    await drain();
  }
  return { clock, workers, channels, portCloses, failures, substrate, words, layout, launch, baseline, ingress, ack, spawn, cleanup, get outcome() { return outcome; }, readOutcome(): Outcome { return outcome; }, get configuredSubstrate() { return configuredSubstrate; }, get configured() { return configured; } };
}
