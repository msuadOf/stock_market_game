import { browserWorkerEntry, reportBrowserError, spawnModuleWorker, transferMessageChannel } from "../../../../packages/libos/browser/browser-transport.ts";
import { createEngineControlOwner } from "./engine-control-owner.ts";

// 编译器必须静态识别每个入口，实际 Worker 构造和消息通道由 LibOS 提供。
const workerAssets = import.meta.env === undefined ? undefined : import.meta.glob<string>(
  ["./wasm-worker.ts", "./facade-child-worker.ts", "./wasm-publisher-worker.ts", "./wasm-ingress-worker.ts"],
  { eager: true, import: "default", query: "?worker&url" },
);

function startWorker(source: string, name: string): Worker {
  return spawnModuleWorker(browserWorkerEntry(source, import.meta.url, workerAssets), name);
}

const controlScope = self as unknown as Pick<Worker, "postMessage">;

createEngineControlOwner({
  createEngineWorker(name) { return startWorker("./wasm-worker.ts", name); },
  createChildWorker(name) { return startWorker("./facade-child-worker.ts", name); },
  createPublisherWorker(name) { return startWorker("./wasm-publisher-worker.ts", name); },
  createIngressWorker(name) { return startWorker("./wasm-ingress-worker.ts", name); },
  createChannel: transferMessageChannel,
  postMessage(value, transfer) { controlScope.postMessage(value, transfer ?? []); },
  addMessageListener(listener) { self.addEventListener("message", listener); },
  reportError: reportBrowserError,
});
