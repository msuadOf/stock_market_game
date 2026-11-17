import { observerOrdinal } from "./observer-resync.ts";
import { isMessagePort, reportBrowserError, transferMessageChannel } from "../../../../packages/libos/browser/browser-transport.ts";
import { SharedSubstratePublisher } from "../../../../packages/libos/browser/shared-substrate-publisher.ts";
import { parseFacadeEngineSubstrate } from "./facade-engine-protocol.ts";
import { bindFacadeThreadEnvironment, getArenaView, installFacadeSlotLayout } from "./engine-clock-env.ts";
import { instantiateFacadeModule, type FacadeSyncInitializer } from "./facade-wasm-module.ts";

type PublishBindings = FacadeSyncInitializer & {
  readonly market_publisher_signal: (token: number) => number;
  readonly market_publisher_bind: (token: number, generation: bigint) => bigint;
  readonly market_publisher_hold: (token: number, lease: bigint) => bigint;
  readonly market_publisher_start: (token: number, lease: bigint) => void;
  readonly market_publisher_flushed: (token: number, lease: bigint, ordinal: bigint) => void;
  readonly market_publisher_ack: (token: number, lease: bigint, ordinal: bigint) => void;
  readonly market_publisher_resume: (token: number, lease: bigint, ordinal: bigint) => boolean;
  readonly market_publisher_release: (token: number, lease: bigint) => void;
  readonly market_publisher_take: (token: number, lease: bigint) => unknown;
  readonly wasm_tombstone_bind_publisher: (address: number, workerId: number) => void;
};

const scope = self as unknown as {
  readonly location: Location;
  readonly close: () => void;
  readonly postMessage: (message: unknown) => void;
  readonly addEventListener: (type: "message", listener: (event: MessageEvent<unknown>) => void) => void;
};
let booting = false;
let failed = false;

function report(error: unknown): void {
  if (failed) return;
  failed = true;
  const detail = error instanceof Error ? error : new Error("发布 Worker 执行失败", { cause: error });
  reportBrowserError(detail);
}

class ObserverTransportError extends Error {}

function record(value: unknown, keys: string, label: string): Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value) || Object.keys(value).sort().join(",") !== keys) throw new Error(`${label} 字段无效`);
  return value as Record<string, unknown>;
}

function positive(value: unknown, label: string): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value <= 0) throw new Error(`${label} 必须为正安全整数`);
  return value;
}

function readOrdinal(value: unknown): bigint {
  return BigInt(observerOrdinal(value));
}

function observerInput<T>(read: () => T): T {
  try { return read(); }
  catch (error) { throw new ObserverTransportError(error instanceof Error ? error.message : "观察者输入校验失败", { cause: error }); }
}

async function bootstrap(raw: unknown): Promise<void> {
  if (booting) throw new Error("发布 Worker 不接受重复的 transport bootstrap");
  booting = true;
  const message = record(raw, "dataPort,diagnostics,instanceId,layout,memory,module,primary,type", "发布 transport");
  if (message.type !== "bindPublisherTransport" || !(isMessagePort(message.dataPort))) throw new Error("发布 Worker 缺少独立业务端口");
  const instanceId = positive(message.instanceId, "发布 Worker instanceId");
  const primary = record(message.primary, "address,workerId", "发布 Worker tombstone");
  if (primary.workerId !== 0xfffffffe) throw new Error("发布 Worker tombstone workerId 无效");
  const address = positive(primary.address, "发布 Worker tombstone address");
  if (!(message.memory instanceof WebAssembly.Memory) || address % 4 !== 0 || address + 256 > message.memory.buffer.byteLength) throw new Error("发布 Worker tombstone 地址无效");
  const substrate = parseFacadeEngineSubstrate({ type: "facadeEngineSubstrate", module: message.module, memory: message.memory, layout: message.layout, diagnostics: message.diagnostics });
  const port = message.dataPort;
  // 独立副实例与 ingress 共用禁止 spawn / terminate 的最小权限。
  bindFacadeThreadEnvironment({ memory: substrate.memory, role: "ingress", postControl() { throw new Error("发布 Worker 禁止发出 facade 线程控制消息"); } });
  let bindings: PublishBindings;
  if (substrate.diagnostics) {
    if (!import.meta.env.DEV) throw new Error("正式构建禁止加载 diagnostics WASM");
    const moduleUrl = new URL("/wasm-diagnostics-pkg/web_wasm.js", scope.location.origin);
    bindings = await import(/* @vite-ignore */ moduleUrl.href) as PublishBindings;
  } else bindings = await import("../../wasm-pkg/web_wasm.js") as unknown as PublishBindings;
  if (failed) return;
  const exports = instantiateFacadeModule(bindings, substrate.module, substrate.memory);
  if (typeof bindings.market_publisher_flushed !== "function" || typeof bindings.market_publisher_bind !== "function" || typeof bindings.market_publisher_hold !== "function" || typeof bindings.market_publisher_start !== "function" || typeof bindings.wasm_tombstone_bind_publisher !== "function" || typeof bindings.market_publisher_signal !== "function" || typeof bindings.market_publisher_take !== "function" || typeof bindings.market_publisher_release !== "function" || typeof bindings.market_publisher_resume !== "function" || typeof bindings.market_publisher_ack !== "function") throw new Error("WASM 缺少发布 Worker ABI");
  bindings.wasm_tombstone_bind_publisher(address, primary.workerId);
  installFacadeSlotLayout(substrate.layout);
  getArenaView();

  const eventQueue = transferMessageChannel();
  let observerFault = false;
  let currentGeneration = 0;
  let currentLease = 0n;
  function observerFailure(error: ObserverTransportError): void {
    if (observerFault) return;
    observerFault = true;
    // 用独立控制端报告，业务端口已经坏时也不会退回到进程级故障。
    scope.postMessage({ type: "publisherLifecycle", instanceId, state: "observerFailure", generation: currentGeneration, message: error.message });
  }
  function postObserver(value: unknown): void {
    try { port.postMessage({ ...(value as object), observerLease: currentLease.toString() }); }
    catch (error) { throw new ObserverTransportError("发布业务端口无法投递消息", { cause: error }); }
  }
  function classify(error: unknown): void {
    if (error instanceof ObserverTransportError) observerFailure(error);
    else report(error);
  }
  let resume: (() => void) | null = null;
  eventQueue.port1.onmessage = () => { const resolve = resume; resume = null; resolve?.(); };
  const publisher = new SharedSubstratePublisher<unknown>({
    memory: substrate.memory,
    signal: bindings.market_publisher_signal,
    take: (token) => bindings.market_publisher_take(token, currentLease),
    deliver(value, generation) {
      const flushed = (ordinal: unknown) => { if (currentToken === null) throw new Error("发布 Worker 未绑定"); bindings.market_publisher_flushed(currentToken, currentLease, readOrdinal(ordinal)); };
      if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Error("发布 ABI envelope 必须是对象");
      const kind = (value as Record<string, unknown>).kind;
      if (kind === "closed") { record(value, "kind", "发布关闭"); return "stop"; }
      if (kind === "gap") {
        const { oldestAfter } = record(value, "kind,oldestAfter", "发布缺口");
        readOrdinal(oldestAfter);
        postObserver({ type: "publicationGap", generation, oldestAfter });
        // 不冒充已恢复；等待同一发布业务协议重新提供有效 cursor。
        return "stop";
      }
      if (kind === "failure") {
        const hasOrdinal = Object.hasOwn(value, "publicationOrdinal");
        const { failure, publicationOrdinal } = record(value, hasOrdinal ? "failure,kind,publicationOrdinal" : "failure,kind", "发布权威故障");
        const recoverable = failure !== null && typeof failure === "object" && (failure as Record<string, unknown>).recoverable === true;
        if (recoverable !== hasOrdinal) throw new Error("发布权威故障与观察者额度契约不匹配");
        if (hasOrdinal) readOrdinal(publicationOrdinal);
        postObserver({ type: "publicationFailure", generation, failure, ...(hasOrdinal ? { publicationOrdinal } : {}) });
        if (hasOrdinal) flushed(publicationOrdinal);
        return recoverable ? "continue" : "stop";
      }
      if (kind === "update") {
        const { barrierPaused, publicationOrdinal, update } = record(value, "barrierPaused,kind,publicationOrdinal,update", "发布更新");
        readOrdinal(publicationOrdinal);
        if (typeof barrierPaused !== "boolean") throw new Error("发布更新日界状态无效");
        postObserver({ type: "protocol", generation, publicationOrdinal, update });
        flushed(publicationOrdinal);
        if (barrierPaused) postObserver({ type: "barrierPaused", generation });
        return "continue";
      }
      throw new Error("发布 ABI envelope kind 无效");
    },
    failure: classify,
    yieldTask: () => new Promise<void>((resolve) => { resume = resolve; eventQueue.port2.postMessage(0); }),
  });

  let pending: Promise<void> = Promise.resolve();
  let currentToken: number | null = null;
  let closed = false;
  const stopObserver = (error: ObserverTransportError): void => {
    observerFailure(error);
    if (currentGeneration !== 0) void publisher.pause(currentGeneration).catch(report);
  };
  port.addEventListener("messageerror", (event) => stopObserver(new ObserverTransportError("发布 Worker 业务端口 messageerror", { cause: event })));
  port.addEventListener("message", (event: MessageEvent<unknown>) => {
    pending = pending.then(async () => {
      if (failed || closed) return;
      if (event.data === null || typeof event.data !== "object") throw new ObserverTransportError("发布 Worker 请求必须是对象");
      const input = event.data;
      const type: unknown = (input as Record<string, unknown>).type;
      if (observerFault && type !== "quiescePublisher") return;
      if (type === "ackPublisher") {
        const [generation, lease, ordinal] = observerInput(() => {
          const ack = record(event.data, "generation,observerLease,ordinal,type", "发布 ACK");
          return [positive(ack.generation, "发布 ACK generation"), readOrdinal(ack.observerLease), readOrdinal(ack.ordinal)] as const;
        });
        // 迟到 ACK 只属于已撤下的观察者，禁止影响新 generation 的 credit。
        if (generation < currentGeneration || (generation === currentGeneration && lease < currentLease)) return;
        if (lease !== currentLease) throw new ObserverTransportError("STALE_OBSERVER_LEASE");
        if (generation !== currentGeneration || currentToken === null) throw new ObserverTransportError("发布 ACK generation 尚未绑定");
        try { bindings.market_publisher_ack(currentToken, currentLease, ordinal); }
        catch (error) {
          if (error === "发布确认序号不是最早未确认项") throw new ObserverTransportError(error);
          throw error;
        }
        return;
      }
      const [request, requestId, generation] = observerInput(() => {
        const request = record(event.data, type === "bindPublisher" ? "generation,requestId,token,type" : type === "resumePublisher" ? "generation,observerLease,ordinal,requestId,type" : type === "quiescePublisher" && Object.hasOwn(input, "retainObserver") ? "generation,observerLease,requestId,retainObserver,type" : "generation,observerLease,requestId,type", "发布 Worker 请求");
        // generation 0 只表示确实尚未绑定的 Worker 边界关闭。
        const generation = type === "quiescePublisher" && request.generation === 0 ? 0 : positive(request.generation, "发布 Worker generation");
        return [request, positive(request.requestId, "发布 Worker requestId"), generation] as const;
      });
      if (type !== "bindPublisher" && readOrdinal(request.observerLease) !== currentLease) {
        postObserver({ type: "operationError", requestId, generation, message: "STALE_OBSERVER_LEASE" });
        return;
      }
      if (type === "bindPublisher") {
        const token = observerInput(() => positive(request.token, "发布 Worker token"));
        await publisher.bind(generation, token, () => {
          if (currentToken !== null && currentToken !== token) bindings.market_publisher_release(currentToken, currentLease);
          currentLease = bindings.market_publisher_bind(token, BigInt(generation));
          currentToken = token;
          currentGeneration = generation;
          postObserver({ type: "publisherBound", requestId, generation });
          scope.postMessage({ type: "publisherLifecycle", instanceId, state: "configured", generation, token });
        });
      } else if (type === "pausePublisher") {
        if (generation !== currentGeneration) throw new ObserverTransportError("发布 Worker 暂停 generation 无效");
        await publisher.pause(generation);
        if (currentToken === null) throw new ObserverTransportError("发布 Worker 未绑定");
        currentLease = bindings.market_publisher_hold(currentToken, currentLease);
        postObserver({ type: "publisherPaused", requestId, generation });
      } else if (type === "startPublisher") {
        if (generation !== currentGeneration) throw new ObserverTransportError("发布 Worker 启动 generation 无效");
        if (currentToken === null) throw new ObserverTransportError("发布 Worker 未绑定");
        bindings.market_publisher_start(currentToken, currentLease);
        publisher.start(generation, () => postObserver({ type: "publisherStarted", requestId, generation }));
      } else if (type === "resumePublisher") {
        if (generation !== currentGeneration) throw new ObserverTransportError("发布 Worker 续接 generation 无效");
        const ordinal = observerInput(() => readOrdinal(request.ordinal));
        await publisher.resume(generation, (token) => {
          try { return bindings.market_publisher_resume(token, currentLease, ordinal); }
          catch (error) {
            if (error === "发布基线的 ordinal 晚于已交接内容") throw new ObserverTransportError(error);
            throw error;
          }
        }, (accepted) => {
          postObserver({ type: "publisherResumed", requestId, generation, accepted });
        }, false);
      } else if (type === "quiescePublisher") {
        if (generation !== currentGeneration) throw new Error("发布 Worker 拒绝过期的关闭请求");
        const retainObserver = request.retainObserver === true;
        if (Object.hasOwn(request, "retainObserver") && (!retainObserver || !import.meta.env.DEV || !substrate.diagnostics)) throw new ObserverTransportError("仅验证产物可交接 Outbound 所有权");
        closed = true;
        await publisher.close();
        if (currentToken !== null) {
          if (!retainObserver) bindings.market_publisher_release(currentToken, currentLease);
          currentToken = null;
        }
        eventQueue.port1.close(); eventQueue.port2.close();
        exports.__wbindgen_thread_destroy(0, 0, 0);
        port.postMessage({ type: "publisherQuiescent", requestId, generation });
        port.close();
        scope.postMessage({ type: "publisherLifecycle", instanceId, state: "quiescent" });
        scope.close();
      } else throw new ObserverTransportError("发布 Worker 收到未知请求");
    }).catch((error: unknown) => {
      if (error instanceof ObserverTransportError) stopObserver(error);
      else report(error);
    });
  });
  port.start();
}
scope.addEventListener("message", (event) => { void bootstrap(event.data).catch(report); });
