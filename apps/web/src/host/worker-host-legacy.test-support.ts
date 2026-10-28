import test from "node:test";
import { MessageChannel as LegacyChannel } from "node:worker_threads";
import { CAPACITY, GEN_BITS, HEADER_WORDS, SLOT_STRIDE, JOIN_STRIDE } from "./facade-slot-protocol.ts";
import { createWorkerHost } from "./worker-host.ts";

// 旧 direct 用例只补生产 bootstrap；原业务消息与断言继续由各用例控制。
const legacyMessageChannel = Object.getOwnPropertyDescriptor(globalThis, "MessageChannel");
const legacyWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
Object.defineProperty(globalThis, "window", { configurable: true, value: globalThis });
test.after(() => {
  if (legacyMessageChannel === undefined) Reflect.deleteProperty(globalThis, "MessageChannel");
  else Object.defineProperty(globalThis, "MessageChannel", legacyMessageChannel);
  if (legacyWindow === undefined) Reflect.deleteProperty(globalThis, "window");
  else Object.defineProperty(globalThis, "window", legacyWindow);
});
let manualLegacyIngress = false;
export class LegacyWorker extends EventTarget {
  configured?: Promise<void>;
  prepare(value: Record<string, unknown>): void {
    if (value.type === "init") {
      const port = value.controlPort as MessagePort;
      const memory = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true });
      const layout = { baseAddr: 0, headerWords: HEADER_WORDS, slotStride: SLOT_STRIDE, joinStride: JOIN_STRIDE, capacity: 4, genBits: 29 };
      const words = new Int32Array(memory.buffer);
      words[CAPACITY] = 4; words[GEN_BITS] = 29;
      this.configured = new Promise((resolve) => {
        port.addEventListener("message", (event) => {
          if (event.data.type === "facadeEngineConfigured") { port.close(); resolve(); }
        });
      });
      port.start();
      port.postMessage({ type: "facadeEngineSubstrate", module: new WebAssembly.Module(new Uint8Array([0,97,115,109,1,0,0,0])), memory, layout, diagnostics: false });
    }
    if (value.type === "bindIngress" && !manualLegacyIngress) queueMicrotask(() => {
      this.dispatchEvent(new MessageEvent("message", { data: { type: "ingressBound", generation: value.generation, requestId: value.requestId } }));
    });
  }
}
export async function bootstrapLegacy(worker: LegacyWorker, autoIngress = true): Promise<void> {
  await worker.configured;
  manualLegacyIngress = !autoIngress;
  worker.dispatchEvent(new MessageEvent("message", { data: { type: "created", generation: 1, capabilities: { npcDecisionDiagnostics: false, personalTradeHistory: false, indicatorCapabilities: { intradayAverage: false, macd: false, priceKdj: false, candleKdj: false } } } }));
  if (autoIngress) worker.dispatchEvent(new MessageEvent("message", { data: { type: "ingress", generation: 1, token: 42, shared: true } }));
}
export function createLegacyHost(setup: Parameters<typeof createWorkerHost>[0], seed: bigint) {
  // 保留 Node 原生控制 Channel，避免用同步桩绕过 configured 门禁。
  Object.defineProperty(globalThis, "MessageChannel", { configurable: true, writable: true, value: LegacyChannel });
  return createWorkerHost(setup, seed, { onSpawnFailure: (failure) => { throw new Error(failure.message); } });
}
