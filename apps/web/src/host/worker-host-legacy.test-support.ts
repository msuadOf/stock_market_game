import test from "node:test";
import { MessageChannel as LegacyChannel } from "node:worker_threads";
import { setImmediate } from "node:timers/promises";
import { CAPACITY, GEN_BITS, HEADER_WORDS, SLOT_STRIDE, JOIN_STRIDE } from "./facade-slot-protocol.ts";
import { createWorkerHost } from "./worker-host.ts";
import { createEngineControlOwner } from "./engine-control-owner.ts";

// 旧 direct 用例只补生产 bootstrap；原业务消息与断言继续由各用例控制。
const legacyMessageChannel = Object.getOwnPropertyDescriptor(globalThis, "MessageChannel");
const legacyWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
const legacyPorts = new Set<MessagePort>();
const legacyPeers = new Map<MessagePort, MessagePort>();
Object.defineProperty(globalThis, "window", { configurable: true, value: globalThis });
test.after(() => {
  for (const port of legacyPorts) port.close();
  legacyPorts.clear();
  legacyPeers.clear();
  if (legacyMessageChannel === undefined) Reflect.deleteProperty(globalThis, "MessageChannel");
  else Object.defineProperty(globalThis, "MessageChannel", legacyMessageChannel);
  if (legacyWindow === undefined) Reflect.deleteProperty(globalThis, "window");
  else Object.defineProperty(globalThis, "window", legacyWindow);
});
let manualLegacyIngress = false;
export class LegacyWorker extends EventTarget {
  configured?: Promise<void>;
  controlPort?: MessagePort;
  dataPort?: MessagePort;
  instanceId?: number;
  private receiverPort?: MessagePort;
  private engineConfigured?: (value: void) => void;
  postMessage(value: Record<string, unknown>): void { this.prepare(value); }
  prepare(value: Record<string, unknown>): void {
    if (value.type === "bindEngineTransport") {
      this.controlPort = value.controlPort as MessagePort;
      this.dataPort = value.dataPort as MessagePort;
      this.instanceId = Number(value.instanceId);
      this.configured = new Promise((resolve) => { this.engineConfigured = resolve; });
      this.controlPort.addEventListener("message", (event) => {
        if ((event.data as { type?: string }).type === "facadeEngineConfigured") this.engineConfigured?.();
      });
      this.controlPort.start();
      this.dataPort.addEventListener("message", (event) => this.postMessage(event.data as Record<string, unknown>));
      this.dataPort.start();
    }
    if (value.type === "bindIngressTransport") {
      this.dataPort = value.dataPort as MessagePort;
      this.instanceId = Number(value.instanceId);
      this.dataPort.addEventListener("message", (event) => this.postMessage(event.data as Record<string, unknown>));
      this.dataPort.start();
    }
    if (value.type === "init") {
      const port = this.controlPort;
      if (port === undefined) throw new Error("legacy engine 尚未绑定 controlPort");
      const memory = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true });
      const layout = { baseAddr: 0, headerWords: HEADER_WORDS, slotStride: SLOT_STRIDE, joinStride: JOIN_STRIDE, capacity: 4, genBits: 29 };
      const words = new Int32Array(memory.buffer);
      words[CAPACITY] = 4; words[GEN_BITS] = 29;
      port.postMessage({ type: "facadeEngineSubstrate", module: new WebAssembly.Module(new Uint8Array([0,97,115,109,1,0,0,0])), memory, layout, diagnostics: false });
    }
    if (value.type === "bindIngress") {
      if (this.dataPort === undefined || this.instanceId === undefined) throw new Error("legacy ingress 尚未绑定 dataPort");
      this.dispatchEvent(new MessageEvent("message", { data: { type: "ingressLifecycle", instanceId: this.instanceId, state: "configured", generation: value.generation, token: value.token } }));
      if (!manualLegacyIngress) queueMicrotask(() => this.dataPort?.postMessage({ type: "ingressBound", generation: value.generation, requestId: value.requestId }));
    }
    // 仅模拟无 World/child 的 cooperative fixture 协议响应，不冒充真实 Rust join。
    if (value.type === "shutdown") queueMicrotask(() => {
      this.dataPort?.postMessage({
        type: "shutdownCompleted",
        requestId: value.requestId,
        generation: value.generation,
        report: { kind: "success", handle: 7, runtime: null },
      });
      if (this.instanceId !== undefined) this.dispatchEvent(new MessageEvent("message", { data: { type: "engineLifecycle", instanceId: this.instanceId, state: "quiescent" } }));
    });
    if (value.type === "quiesceIngress") queueMicrotask(() => {
      this.dataPort?.postMessage({
        type: "ingressQuiescent",
        requestId: value.requestId,
        generation: value.generation,
      });
      if (this.instanceId !== undefined) this.dispatchEvent(new MessageEvent("message", { data: { type: "ingressLifecycle", instanceId: this.instanceId, state: "quiescent" } }));
    });
  }
  emit(value: unknown): void {
    const type = value !== null && typeof value === "object" ? (value as { type?: unknown }).type : undefined;
    if (this.dataPort !== undefined && typeof type === "string" && type !== "engineLifecycle" && type !== "ingressLifecycle") this.dataPort.postMessage(value);
    else this.dispatchEvent(new MessageEvent("message", { data: value }));
  }
  emitRaw(value: unknown): void {
    if (this.receiverPort === undefined) throw new Error("legacy raw response 缺少真实 business port 对端");
    this.receiverPort.dispatchEvent(new MessageEvent("message", { data: value }));
  }
  bindReceiver(port: MessagePort): void {
    const receiver = legacyPeers.get(port);
    if (receiver === undefined) throw new Error("legacy transport 缺少已登记的 business port 对端");
    this.receiverPort = receiver;
  }
}
export async function drainLegacyPorts(): Promise<void> {
  for (let count = 0; count < 8; count++) await setImmediate();
}
export async function bootstrapLegacy(worker: LegacyWorker, autoIngress = true): Promise<void> {
  await worker.configured;
  manualLegacyIngress = !autoIngress;
  worker.emit({ type: "created", generation: 1, capabilities: { npcDecisionDiagnostics: false, personalTradeHistory: false, indicatorCapabilities: { intradayAverage: false, macd: false, priceKdj: false, candleKdj: false } } });
  if (autoIngress) worker.emit({ type: "ingress", generation: 1, token: 42, shared: true });
  await drainLegacyPorts();
}
export function createLegacyHost(setup: Parameters<typeof createWorkerHost>[0], seed: bigint) {
  // 保留 Node 原生控制 Channel，避免用同步桩绕过 configured 门禁。
  class TrackedLegacyChannel extends LegacyChannel {
    constructor() {
      super();
      const first = this.port1 as unknown as MessagePort, second = this.port2 as unknown as MessagePort;
      legacyPorts.add(first); legacyPorts.add(second); legacyPeers.set(second, first);
    }
  }
  Object.defineProperty(globalThis, "MessageChannel", { configurable: true, writable: true, value: TrackedLegacyChannel });
  const LegacyWorkerConstructor = globalThis.Worker;
  if (typeof LegacyWorkerConstructor !== "function") throw new Error("legacy fixture 缺少测试 Worker 构造器");
  class LegacyController extends EventTarget {
    private ownerListener: ((event: MessageEvent<unknown>) => void) | undefined;
    private terminated = false;
    constructor() {
      super();
      createEngineControlOwner({
        createEngineWorker: () => this.createLegacyWorker(new URL("./wasm-worker.ts", import.meta.url)),
        createIngressWorker: () => this.createLegacyWorker(new URL("./wasm-ingress-worker.ts", import.meta.url)),
        createChildWorker: () => this.createLegacyWorker(new URL("./facade-child-worker.ts", import.meta.url)),
        createChannel: () => {
          const channel = new TrackedLegacyChannel();
          legacyPorts.add(channel.port1 as unknown as MessagePort); legacyPorts.add(channel.port2 as unknown as MessagePort);
          return channel as unknown as MessageChannel;
        },
        postMessage: (message) => this.dispatchEvent(new MessageEvent("message", { data: message })),
        addMessageListener: (listener) => { this.ownerListener = listener; },
        reportError: (error) => { throw error; },
      });
    }
    private createLegacyWorker(url: URL): Worker {
      const worker = new (LegacyWorkerConstructor as unknown as new (...args: never[]) => LegacyWorker)(url as never);
      const originalPostMessage = worker.postMessage.bind(worker);
      const originalEmit = worker.emit.bind(worker);
      const originalDispatchEvent = worker.dispatchEvent.bind(worker);
      const terminable = worker as LegacyWorker & { terminate(): void };
      const originalTerminate = terminable.terminate.bind(worker);
      terminable.terminate = () => {
        try { originalTerminate(); }
        finally { worker.controlPort?.close(); worker.dataPort?.close(); }
      };
      worker.postMessage = (value: Record<string, unknown>) => {
        if (value.type === "bindEngineTransport" || value.type === "bindIngressTransport") {
          if (value.controlPort instanceof MessagePort) legacyPorts.add(value.controlPort);
          if (value.dataPort instanceof MessagePort) legacyPorts.add(value.dataPort);
          worker.bindReceiver(value.dataPort as MessagePort);
          worker.prepare(value);
        }
        else originalPostMessage(value);
      };
      worker.emit = (value: unknown) => {
        const type = value !== null && typeof value === "object" ? (value as { type?: unknown }).type : undefined;
        if (worker.dataPort !== undefined && typeof type === "string" && type !== "engineLifecycle" && type !== "ingressLifecycle") worker.dataPort.postMessage(value);
        else originalEmit(value);
      };
      worker.dispatchEvent = (event: Event) => {
        if (event.type === "message" && worker.dataPort !== undefined) {
          const value = (event as MessageEvent<unknown>).data;
          const type = value !== null && typeof value === "object" ? (value as { type?: unknown }).type : undefined;
          if (typeof type === "string" && type !== "engineLifecycle" && type !== "ingressLifecycle") {
            worker.dataPort.postMessage(value);
            return true;
          }
        }
        return originalDispatchEvent(event);
      };
      return worker as unknown as Worker;
    }
    postMessage(value: Record<string, unknown>): void {
      this.ownerListener?.(new MessageEvent("message", { data: value }));
    }
    terminate(): void { this.terminated = true; }
    get isTerminated(): boolean { return this.terminated; }
  }
  const original = Object.getOwnPropertyDescriptor(globalThis, "Worker");
  Object.defineProperty(globalThis, "Worker", { configurable: true, writable: true, value: LegacyController });
  try { return createWorkerHost(setup, seed, { onSpawnFailure: (failure) => { throw new Error(failure.message); } }); }
  finally {
    if (original === undefined) Reflect.deleteProperty(globalThis, "Worker");
    else Object.defineProperty(globalThis, "Worker", original);
  }
}
