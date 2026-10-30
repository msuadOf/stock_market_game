import { WasmIngressReceiver } from "./wasm-ingress.ts";
import { describeWasmFailure } from "./wasm-failure.ts";
import { bindFacadeThreadEnvironment, installFacadeSlotLayout, getArenaView } from "./engine-clock-env.ts";
import { instantiateFacadeModule, type FacadeSyncInitializer } from "./facade-wasm-module.ts";
import { parseFacadeEngineSubstrate } from "./facade-engine-protocol.ts";

type IngressBindings = FacadeSyncInitializer & {
  ingress_enqueue(token: number, intent: unknown): void;
};

let bindings: IngressBindings | null = null;
const ingress = new WasmIngressReceiver((token, intent) => {
  if (bindings === null) throw new Error("Browser ingress WASM 尚未初始化");
  bindings.ingress_enqueue(token, intent);
});
let pending = Promise.resolve();
let closing = false;
let businessPort: MessagePort | undefined;
let ownerInstanceId: number | undefined;
let transportBound = false;
let cachedSubstrate: ReturnType<typeof parseFacadeEngineSubstrate> | undefined;
function post(message: unknown): void {
  if (businessPort === undefined) throw new Error("Browser ingress business port 尚未绑定");
  businessPort.postMessage(message);
}

function handleBusinessMessage(event: MessageEvent<Record<string, unknown>>): void {
  const message = event.data;
  pending = pending.then(async () => {
    try {
      if (closing) throw new Error("Browser ingress 已 quiesce，拒绝后续调用");
      switch (message.type) {
        case "bindIngress": {
          if (typeof message.generation !== "number" || !Number.isSafeInteger(message.generation) || message.generation < 1) throw new Error("Browser ingress generation 必须是正安全整数");
          if (typeof message.token !== "number" || !Number.isInteger(message.token) || message.token < 1 || message.token > 0xffffffff) throw new Error("Browser ingress token 必须是正 u32");
          if (message.shared !== true) throw new Error("Browser ingress 必须使用 engine 已配置的共享 substrate");
          if (bindings === null) {
            const substrate = cachedSubstrate;
            if (substrate === undefined) throw new Error("Browser ingress 缺少 owner 绑定的真实共享 substrate");
            bindFacadeThreadEnvironment({ memory: substrate.memory, role: "ingress", postControl() { throw new Error("ingress 禁止 facade 控制消息"); } });
            let loaded: IngressBindings;
            if (substrate.diagnostics) {
              if (!import.meta.env.DEV) throw new Error("正式构建禁止加载 diagnostics WASM");
              const moduleUrl = new URL("/wasm-diagnostics-pkg/web_wasm.js", self.location.origin);
              loaded = await import(/* @vite-ignore */ moduleUrl.href) as IngressBindings;
            } else {
              loaded = await import("../../wasm-pkg/web_wasm.js") as unknown as IngressBindings;
            }
            instantiateFacadeModule(loaded, substrate.module, substrate.memory);
            installFacadeSlotLayout(substrate.layout); getArenaView();
            bindings = loaded;
          } else if (["module", "memory", "layout", "diagnostics"].some((key) => Object.hasOwn(message, key))) {
            throw new Error("Browser ingress 换代只接受新 token，禁止重复初始化 substrate");
          }
          ingress.bind(message.generation, message.token);
          post({ type: "ingressBound", requestId: message.requestId, generation: message.generation });
          if (ownerInstanceId !== undefined) self.postMessage({ type: "ingressLifecycle", instanceId: ownerInstanceId, state: "configured", generation: message.generation, token: message.token });
          return;
        }
        case "quiesceIngress":
          closing = true;
          ingress.close();
          // 此 ACK 之前的同步 enqueue 已返回，后续 submit/bind 均由 closed 拒绝。
          bindings = null;
          cachedSubstrate = undefined;
          post({ type: "ingressQuiescent", requestId: message.requestId, generation: message.generation });
          if (ownerInstanceId !== undefined) self.postMessage({ type: "ingressLifecycle", instanceId: ownerInstanceId, state: "quiescent" });
          return;
        case "enqueue":
          ingress.submit(message.generation, message.intent);
          post({ type: "enqueued", requestId: message.requestId, generation: message.generation });
          return;
        default:
          throw new Error(`未知 Browser ingress 消息：${String(message.type)}`);
      }
    } catch (error) {
      const description = describeWasmFailure(error);
      try {
        post({ type: "operationError", requestId: message.requestId, generation: message.generation, message: description.trim().length === 0 ? "Browser ingress 错误没有非空 message，请反馈详情" : description });
      } catch (postError) {
        const fatal = new AggregateError([error, postError], `Browser ingress 错误无法投递：${description}；发送错误：${describeWasmFailure(postError)}`);
        if (typeof self.reportError === "function") self.reportError(fatal);
        else queueMicrotask(() => { throw fatal; });
      }
    }
  });
}

self.addEventListener("message", (event: MessageEvent<unknown>) => {
  if (transportBound) {
    const message = event.data;
    if (message !== null && typeof message === "object" && (message as Record<string, unknown>).type === "bindIngressTransport") {
      const source = message as Record<string, unknown>;
      try {
        if (Object.keys(source).sort().join(",") !== "dataPort,diagnostics,instanceId,layout,memory,module,type" || !Number.isSafeInteger(source.instanceId)
          || Number(source.instanceId) < 1 || !(source.dataPort instanceof MessagePort)) throw new Error("ingress transport 字段无效");
        throw new Error("Browser ingress transport 只允许首次绑定");
      } catch (error) { if (typeof self.reportError === "function") self.reportError(error); else queueMicrotask(() => { throw error; }); }
      return;
    }
    const error = new Error("Browser ingress 不接受 transport 绑定后的全局业务消息");
    if (typeof self.reportError === "function") self.reportError(error); else queueMicrotask(() => { throw error; });
    return;
  }
  const value = event.data;
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    const error = new Error("Browser ingress bootstrap 必须是对象");
    if (typeof self.reportError === "function") self.reportError(error); else queueMicrotask(() => { throw error; });
    return;
  }
  const source = value as Record<string, unknown>;
  if (source.type !== "bindIngressTransport") {
    const error = new Error("Browser ingress 只接受 bindIngressTransport");
    if (typeof self.reportError === "function") self.reportError(error); else queueMicrotask(() => { throw error; });
    return;
  }
  try {
    if (Object.keys(source).sort().join(",") !== "dataPort,diagnostics,instanceId,layout,memory,module,type" || !Number.isSafeInteger(source.instanceId)
      || Number(source.instanceId) < 1 || !(source.dataPort instanceof MessagePort)) throw new Error("ingress transport 字段无效");
    const substrate = parseFacadeEngineSubstrate({ type: "facadeEngineSubstrate", module: source.module, memory: source.memory, layout: source.layout, diagnostics: source.diagnostics });
    businessPort = source.dataPort;
    ownerInstanceId = Number(source.instanceId);
    transportBound = true;
    businessPort.addEventListener("message", (incoming) => handleBusinessMessage(incoming as MessageEvent<Record<string, unknown>>));
    businessPort.addEventListener("messageerror", (incoming) => {
      const error = new Error("Browser ingress business port messageerror", { cause: incoming });
      if (typeof self.reportError === "function") self.reportError(error); else queueMicrotask(() => { throw error; });
    });
    businessPort.start();
    // 只缓存真实 supervisor 发来的共享 substrate，bindIngress 仍使用原 handler。
    cachedSubstrate = substrate;
  } catch (error) { if (typeof self.reportError === "function") self.reportError(error); else queueMicrotask(() => { throw error; }); }
});
