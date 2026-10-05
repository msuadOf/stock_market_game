import { WasmIngressReceiver } from "./wasm-ingress.ts";
import { describeWasmFailure } from "./wasm-failure.ts";

type IngressBindings = {
  initSync(options: { module: WebAssembly.Module; memory: WebAssembly.Memory }): unknown;
  ingress_enqueue(token: number, intent: unknown): void;
};

let bindings: IngressBindings | null = null;
const ingress = new WasmIngressReceiver((token, intent) => {
  if (bindings === null) throw new Error("Browser ingress WASM 尚未初始化");
  bindings.ingress_enqueue(token, intent);
});
let pending = Promise.resolve();

self.addEventListener("message", (event: MessageEvent<Record<string, unknown>>) => {
  const message = event.data;
  pending = pending.then(async () => {
    try {
      switch (message.type) {
        case "bindIngress": {
          if (bindings === null) {
            if (message.diagnostics === true) {
              const moduleUrl = new URL("/wasm-diagnostics-pkg/web_wasm.js", self.location.origin);
              bindings = await import(/* @vite-ignore */ moduleUrl.href) as IngressBindings;
            } else {
              bindings = await import("../../wasm-pkg/web_wasm.js") as unknown as IngressBindings;
            }
            bindings.initSync({ module: message.module as WebAssembly.Module, memory: message.memory as WebAssembly.Memory });
          }
          ingress.bind(Number(message.generation), Number(message.token));
          self.postMessage({ type: "ingressBound", requestId: message.requestId, generation: message.generation });
          return;
        }
        case "enqueue":
          ingress.submit(message.generation, message.intent);
          self.postMessage({ type: "enqueued", requestId: message.requestId, generation: message.generation });
          return;
        default:
          throw new Error(`未知 Browser ingress 消息：${String(message.type)}`);
      }
    } catch (error) {
      self.postMessage({ type: "operationError", requestId: message.requestId, generation: message.generation, message: describeWasmFailure(error) });
    }
  });
});
