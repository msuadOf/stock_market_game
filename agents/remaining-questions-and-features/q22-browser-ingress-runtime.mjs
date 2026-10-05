import assert from "node:assert/strict";
import { availableParallelism } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { readFile } from "node:fs/promises";
import { chromium } from "../../apps/web/node_modules/@playwright/test/index.mjs";
import { createServer } from "../../apps/web/node_modules/vite/dist/node/index.js";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const bindings = await readFile(path.join(root, "apps/web/wasm-pkg/web_wasm.js"), "utf8");
assert.ok(/export function ingress_token\(/.test(bindings), "当前 WASM bindings 缺少 ingress_token，必须先由 root 重建");
assert.ok(/export function ingress_enqueue\(/.test(bindings), "当前 WASM bindings 缺少 ingress_enqueue，必须先由 root 重建");
const threads = Math.min(4, availableParallelism());
console.log(JSON.stringify({ classification: "真实 Browser 代表性验收", threads, engineWorkers: 1, intakeWorkers: 1, caseTimeoutMs: 10000 }));
const server = await createServer({ root: path.join(root, "apps/web"), mode: "e2e", server: { host: "127.0.0.1", port: 0 } });
server.middlewares.use((request, response, next) => {
  if (request.url !== "/q22-runtime") return next();
  response.setHeader("Content-Type", "text/html");
  response.end("<!doctype html><title>Q22 Browser ingress 运行验收</title>");
});
let browser;
try {
  await server.listen();
  browser = await chromium.launch({ headless: true });
  const page = await browser.newPage();
  page.setDefaultTimeout(10000);
  await page.goto(`${server.resolvedUrls.local[0]}q22-runtime`);
  const result = await page.evaluate(async (threadCount) => {
    if (!crossOriginIsolated) throw new Error("真实 Browser 验收缺少 COOP/COEP");
    const { DEFAULT_SETUP } = await import("/src/config/defaults.ts");
    const source = `
      self.addEventListener("message", (event) => {
        if (event.data.type !== "holdOwner") return;
        event.stopImmediatePropagation();
        const gate = new Int32Array(event.data.gate);
        Atomics.store(gate, 1, 1);
        self.postMessage({ type: "ownerHeld" });
        Atomics.wait(gate, 0, 0, 5000);
        Atomics.store(gate, 1, 2);
      });
      await import(${JSON.stringify(`${location.origin}/src/host/wasm-worker.ts`)});
      self.postMessage({ type: "ownerBooted" });
    `;
    const ownerUrl = URL.createObjectURL(new Blob([source], { type: "text/javascript" }));
    const owner = new Worker(ownerUrl, { type: "module" });
    const intake = new Worker("/src/host/wasm-ingress-worker.ts", { type: "module" });
    const gate = new Int32Array(new SharedArrayBuffer(8));
    const messages = new Map([[owner, []], [intake, []]]);
    const failures = [];
    for (const worker of [owner, intake]) {
      worker.addEventListener("error", (event) => failures.push(event.message));
      worker.addEventListener("message", ({ data }) => {
        messages.get(worker).push(data);
        if (data.type === "failure") failures.push(JSON.stringify(data));
        if (data.type === "protocol" || data.type === "protocolBatch") owner.postMessage({ type: "uiFrame", generation: data.generation, deliveryId: data.deliveryId });
      });
    }
    const wait = async (worker, predicate) => {
      const deadline = performance.now() + 5000;
      while (performance.now() < deadline) {
        if (failures.length > 0) throw new Error(failures.join("\n"));
        const queue = messages.get(worker);
        const index = queue.findIndex(predicate);
        if (index !== -1) return queue.splice(index, 1)[0];
        await new Promise((resolve) => setTimeout(resolve, 1));
      }
      throw new Error(`等待真实 Worker 响应超时：${predicate.toString()}`);
    };
    let requestId = 0;
    const request = async (worker, message) => {
      const id = ++requestId;
      worker.postMessage({ ...message, requestId: id });
      return wait(worker, (response) => response.requestId === id);
    };
    const requireSuccess = (response, expected) => {
      if (response.type !== expected) throw new Error(`预期 ${expected}，实际 ${JSON.stringify(response)}`);
      return response;
    };
    const buy = (quantity) => ({ PlaceLimit: { code: "600101", side: "Buy", price: { Fixed: "1008" }, qty: quantity } });
    try {
      await wait(owner, (message) => message.type === "ownerBooted");
      owner.postMessage({ type: "init", threads: threadCount });
      const ready = await wait(owner, (message) => message.type === "ready");
      const setup = {
        ...DEFAULT_SETUP,
        stocks: [DEFAULT_SETUP.stocks[0]],
        npcs: { ...DEFAULT_SETUP.npcs, retail_count: 0, inst_count: 0, hot_count: 0 },
        float_allocation: { between_kinds: "Random", within_kind: "Random" },
        ticks_per_day: 30, auction_ticks: 9, closing_auction_ticks: 3, start_date: "2030-01-02",
      };
      owner.postMessage({ type: "create", setup, seed: 42n });
      const bridge = await wait(owner, (message) => message.type === "ingress");
      if (!(bridge.module instanceof WebAssembly.Module) || !(bridge.memory.buffer instanceof SharedArrayBuffer)) throw new Error("没有真实 shared WASM Module/Memory");
      requireSuccess(await request(intake, { ...bridge, type: "bindIngress" }), "ingressBound");
      owner.postMessage({ type: "holdOwner", gate: gate.buffer });
      await wait(owner, (message) => message.type === "ownerHeld");
      requireSuccess(await request(intake, { type: "enqueue", generation: bridge.generation, intent: buy(100) }), "enqueued");
      if (Atomics.load(gate, 1) !== 1 || Atomics.load(gate, 0) !== 0) throw new Error("intake 完成前 owner 已释放，不能证明独立受理");
      Atomics.store(gate, 0, 1);
      Atomics.notify(gate, 0);
      const firstStep = requireSuccess(await request(owner, { type: "stepOnce", generation: bridge.generation }), "stepped");
      const orders = requireSuccess(await request(owner, { type: "playerWorkingOrders", generation: bridge.generation }), "playerWorkingOrders").orders;
      if (firstStep.tick !== 1 || orders.length !== 1 || orders[0].code !== "600101" || orders[0].price !== "1008" || orders[0].remainingQty !== 100 || orders[0].side !== "Buy") throw new Error(`完整 payload 未在下一生产 step 生效：${JSON.stringify(orders)}`);
      for (let index = 0; index < 30; index += 1) requireSuccess(await request(owner, { type: "stepOnce", generation: bridge.generation }), "stepped");
      const saved = requireSuccess(await request(owner, { type: "save", generation: bridge.generation }), "saved");
      const restored = requireSuccess(await request(owner, { type: "restore", generation: bridge.generation, slot: saved.slot }), "restored");
      const nextBridge = await wait(owner, (message) => message.type === "ingress" && message.generation === restored.nextGeneration);
      const closedToken = await request(intake, { type: "enqueue", generation: bridge.generation, intent: buy(200) });
      if (closedToken.type !== "operationError" || !closedToken.message.includes("已关闭或不存在")) throw new Error(`旧 WASM token 没有显式拒绝：${JSON.stringify(closedToken)}`);
      requireSuccess(await request(intake, { ...nextBridge, type: "bindIngress" }), "ingressBound");
      const staleGeneration = await request(intake, { type: "enqueue", generation: bridge.generation, intent: buy(200) });
      if (staleGeneration.type !== "operationError" || !staleGeneration.message.includes("已过期 generation")) throw new Error(`旧 generation 没有显式拒绝：${JSON.stringify(staleGeneration)}`);
      const restoredOrders = requireSuccess(await request(owner, { type: "playerWorkingOrders", generation: nextBridge.generation }), "playerWorkingOrders").orders;
      if (restoredOrders.length !== 0) throw new Error("旧 generation 委托污染恢复后的会话");
      requireSuccess(await request(intake, { type: "enqueue", generation: nextBridge.generation, intent: buy(200) }), "enqueued");
      let continuedTick = saved.slot.snapshot.tick;
      for (let attempt = 0; attempt < 3 && continuedTick === saved.slot.snapshot.tick; attempt += 1) {
        continuedTick = requireSuccess(await request(owner, { type: "stepOnce", generation: nextBridge.generation }), "stepped").tick;
      }
      const continuedOrders = requireSuccess(await request(owner, { type: "playerWorkingOrders", generation: nextBridge.generation }), "playerWorkingOrders").orders;
      if (continuedTick !== saved.slot.snapshot.tick + 1 || continuedOrders.length !== 1 || continuedOrders[0].remainingQty !== 200) throw new Error(`恢复后新 generation 未在生产 step 生效：${JSON.stringify(continuedOrders)}`);
      return { threads: ready.threads, blockedOwnerFullPayloadReceived: true, nextProductionTick: firstStep.tick, orders, producerSaveTick: saved.slot.snapshot.tick, previousToken: bridge.token, restoredToken: nextBridge.token, restoredGeneration: nextBridge.generation, restoredProductionTick: continuedTick, continuedOrders, closedTokenError: closedToken.message, staleGenerationError: staleGeneration.message, npcCompetitionRuntimeVerified: false, productionStepBusyRuntimeVerified: false };
    } finally {
      Atomics.store(gate, 0, 1);
      Atomics.notify(gate, 0);
      owner.terminate();
      intake.terminate();
      URL.revokeObjectURL(ownerUrl);
    }
  }, threads);
  console.log(JSON.stringify(result, null, 2));
} finally {
  if (browser !== undefined) await browser.close();
  await server.close();
}
