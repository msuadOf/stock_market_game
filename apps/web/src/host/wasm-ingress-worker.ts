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

self.addEventListener("message", (event: MessageEvent<Record<string, unknown>>) => {
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
            const substrate = parseFacadeEngineSubstrate({ type: "facadeEngineSubstrate", module: message.module, memory: message.memory, layout: message.layout, diagnostics: message.diagnostics });
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
          self.postMessage({ type: "ingressBound", requestId: message.requestId, generation: message.generation });
          return;
        }
        case "quiesceIngress":
          closing = true;
          ingress.close();
          // 此 ACK 之前的同步 enqueue 已返回，后续 submit/bind 均由 closed 拒绝。
          bindings = null;
          self.postMessage({ type: "ingressQuiescent", requestId: message.requestId, generation: message.generation });
          return;
        case "enqueue":
          ingress.submit(message.generation, message.intent);
          self.postMessage({ type: "enqueued", requestId: message.requestId, generation: message.generation });
          return;
        default:
          throw new Error(`未知 Browser ingress 消息：${String(message.type)}`);
      }
    } catch (error) {
      const description = describeWasmFailure(error);
      try {
        self.postMessage({ type: "operationError", requestId: message.requestId, generation: message.generation, message: description.trim().length === 0 ? "Browser ingress 错误没有非空 message，请反馈详情" : description });
      } catch (postError) {
        const fatal = new AggregateError([error, postError], `Browser ingress 错误无法投递：${description}；发送错误：${describeWasmFailure(postError)}`);
        if (typeof self.reportError === "function") self.reportError(fatal);
        else queueMicrotask(() => { throw fatal; });
      }
    }
  });
});
