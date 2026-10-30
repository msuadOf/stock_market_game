import { createEngineControlOwner } from "./engine-control-owner.ts";

const controlScope = self as unknown as Pick<Worker, "postMessage">;

createEngineControlOwner({
  createEngineWorker(name) { return new Worker(new URL("./wasm-worker.ts", import.meta.url), { type: "module", name }); },
  createChildWorker(name) { return new Worker(new URL("./facade-child-worker.ts", import.meta.url), { type: "module", name }); },
  createIngressWorker(name) { return new Worker(new URL("./wasm-ingress-worker.ts", import.meta.url), { type: "module", name }); },
  createChannel() { return new MessageChannel(); },
  postMessage(value, transfer) { controlScope.postMessage(value, transfer ?? []); },
  addMessageListener(listener) { self.addEventListener("message", listener); },
  reportError(error) {
    if (typeof self.reportError === "function") self.reportError(error);
    else queueMicrotask(() => { throw error; });
  },
});
