import assert from "node:assert/strict";
import { availableParallelism } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { readFile } from "node:fs/promises";
import { chromium } from "../../apps/web/node_modules/@playwright/test/index.mjs";
import { createServer } from "../../apps/web/node_modules/vite/dist/node/index.js";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const productionBindings = await readFile(path.join(root, "apps/web/wasm-pkg/web_wasm.js"), "utf8");
assert.ok(!/export function verification_/.test(productionBindings), "正式 WASM 不得暴露 verification 阻塞控制");
const packagePath = path.join(root, "apps/web/wasm-verification-pkg");
const bindings = await readFile(path.join(packagePath, "web_wasm.js"), "utf8");
for (const name of ["verification_ingress_arm", "verification_ingress_snapshot", "verification_ingress_release", "verification_resting_orders"]) {
  assert.ok(bindings.includes(`export function ${name}(`), `缺少 ${name}，必须先由 root 重建独立 verification-harness WASM`);
}
const threads = Math.min(4, availableParallelism());
const seedArgument = process.argv[2] === undefined ? "2" : process.argv[2];
assert.match(seedArgument, /^(0|[1-9][0-9]*)$/, "seed 必须为规范非负十进制整数");
const seed = BigInt(seedArgument);
assert.ok(seed <= 18446744073709551615n, "seed 超出 u64");
const samePriceWitness = process.argv[3] === "--same-price";
assert.ok(process.argv[3] === undefined || samePriceWitness, "仅支持独立 --same-price 验证用例");
assert.ok(process.argv.length <= 4, "参数过多");
assert.ok(threads >= 2, "真实 NPC 并行验收至少需要 2 个 Rayon threads");
console.log(JSON.stringify({ classification: "真实 Browser 短验收", seed: seed.toString(), samePriceWitness, threads, engineWorkers: 1, intakeWorkers: 1, observerInstances: 1, caseTimeoutMs: 10000 }));
const server = await createServer({
  root: path.join(root, "apps/web"),
  mode: "e2e",
  optimizeDeps: { noDiscovery: true, include: [] },
  resolve: { alias: [
    { find: "../../wasm-pkg/web_wasm.js", replacement: path.join(packagePath, "web_wasm.js") },
    { find: path.join(root, "apps/web/wasm-pkg/web_wasm.js"), replacement: path.join(packagePath, "web_wasm.js") },
  ] },
  server: { host: "127.0.0.1", port: 0 },
});
server.middlewares.use((request, response, next) => {
  if (request.url !== "/q22-busy-runtime") return next();
  response.setHeader("Content-Type", "text/html");
  response.end("<!doctype html><title>Q22 忙碌 production step 验收</title>");
});
let browser;
try {
  await server.listen();
  browser = await chromium.launch({ headless: true });
  const page = await browser.newPage();
  page.setDefaultTimeout(10000);
  await page.goto(`${server.resolvedUrls.local[0]}q22-busy-runtime`);
  const result = await page.evaluate(async ({ threadCount, fixtureSeed, samePrice }) => {
    if (!crossOriginIsolated) throw new Error("真实 Browser 验收缺少 COOP/COEP");
    const { DEFAULT_SETUP } = await import("/src/config/defaults.ts");
    const verificationUrl = `${location.origin}/wasm-verification-pkg/web_wasm.js`;
    const ownerSource = `
      import init, * as wasm from ${JSON.stringify(verificationUrl)};
      let handle;
      self.addEventListener("message", async ({ data }) => {
        try {
          let result;
          switch (data.type) {
            case "init": {
              const module = await WebAssembly.compileStreaming(fetch(${JSON.stringify(`${location.origin}/wasm-verification-pkg/web_wasm_bg.wasm`)}));
              const exports = await init({ module_or_path: module });
              await wasm.initThreadPool(data.threads);
              handle = wasm.create_session(data.setup, data.seed);
              result = { module, memory: exports.memory, token: wasm.ingress_token(handle) };
              break;
            }
            case "step": result = wasm.step(handle); break;
            case "orders": result = wasm.player_working_orders(handle); break;
            case "book": result = wasm.verification_resting_orders(handle, data.account, data.code); break;
            default: throw new Error("未知 owner 消息：" + data.type);
          }
          self.postMessage({ requestId: data.requestId, result });
        } catch (error) {
          self.postMessage({ requestId: data.requestId, error: String(error) });
        }
      });
    `;
    const ownerUrl = URL.createObjectURL(new Blob([ownerSource], { type: "text/javascript" }));
    const owner = new Worker(ownerUrl, { type: "module" });
    const intake = new Worker("/src/host/wasm-ingress-worker.ts", { type: "module" });
    const pending = new Map();
    const failures = [];
    let requestId = 0;
    for (const worker of [owner, intake]) {
      worker.addEventListener("error", (event) => failures.push(event.message));
      worker.addEventListener("message", ({ data }) => {
        const waiter = pending.get(data.requestId);
        if (waiter === undefined) {
          failures.push(`未对应请求的 Worker 消息：${JSON.stringify(data)}`);
          return;
        }
        pending.delete(data.requestId);
        if (data.error !== undefined || data.type === "operationError") waiter.reject(new Error(JSON.stringify(data)));
        else waiter.resolve(data);
      });
    }
    const request = (worker, message) => new Promise((resolve, reject) => {
      const id = ++requestId;
      pending.set(id, { resolve, reject });
      worker.postMessage({ ...message, requestId: id });
    });
    const waitEntered = async (observer, token, phase) => {
      const deadline = performance.now() + 3000;
      while (performance.now() < deadline) {
        if (failures.length > 0) throw new Error(failures.join("\n"));
        const status = observer.verification_ingress_snapshot(token);
        if (status.entered && !status.released && status.point.phase === phase) return status;
        await new Promise((resolve) => setTimeout(resolve, 1));
      }
      throw new Error(`真实 Rust ${phase} 未进入握手`);
    };
    const requireIntake = (response, type) => {
      if (response.type !== type) throw new Error(`预期 ${type}：${JSON.stringify(response)}`);
    };
    const requireFrame = (update, tick) => {
      const frames = update.TickBatch?.frames;
      if (frames?.length !== 1 || frames[0].tick !== tick) throw new Error(`production step 未提交唯一 tick ${tick}`);
      return { tick: frames[0].tick, events: frames[0].events };
    };
    const buy = (qty) => ({ PlaceLimit: { code: "600101", side: "Buy", price: samePrice ? "Highest" : { Fixed: "1008" }, qty } });
    let observer;
    let observerInitialized = false;
    let token;
    try {
      const setup = {
        ...DEFAULT_SETUP,
        stocks: [samePrice ? { ...DEFAULT_SETUP.stocks[0], float_shares: 0 } : DEFAULT_SETUP.stocks[0]],
        config: samePrice ? { ...DEFAULT_SETUP.config, price_cage_enabled: false } : DEFAULT_SETUP.config,
        npcs: { ...DEFAULT_SETUP.npcs, retail_count: 1, inst_count: 0, hot_count: 0 },
        strategy_params: { ...DEFAULT_SETUP.strategy_params, retail: { arrival_rate: 1, order_size_mean: 300, chase_prob: 0 } },
        float_allocation: { between_kinds: "Random", within_kind: "Random" },
        ticks_per_day: 4, auction_ticks: 0, closing_auction_ticks: 0, start_date: "2030-01-02",
      };
      const bridge = (await request(owner, { type: "init", setup, seed: BigInt(fixtureSeed), threads: threadCount })).result;
      token = bridge.token;
      if (!(bridge.module instanceof WebAssembly.Module) || !(bridge.memory.buffer instanceof SharedArrayBuffer)) throw new Error("没有真实 shared WASM Module/Memory");
      observer = await import(verificationUrl);
      await observer.default({ module_or_path: bridge.module, memory: bridge.memory });
      observerInitialized = true;
      requireIntake(await request(intake, { type: "bindIngress", ...bridge, generation: 1 }), "ingressBound");
      observer.verification_ingress_arm(token, "npc_decision", 1n, 1n);
      const busyStep = request(owner, { type: "step" });
      await waitEntered(observer, token, "npc_decision");
      requireIntake(await request(intake, { type: "enqueue", generation: 1, intent: buy(100) }), "enqueued");
      const playerBeforeNpc = observer.verification_ingress_snapshot(token);
      if (!playerBeforeNpc.entered || playerBeforeNpc.released) throw new Error("真实 NPC 已释放，不能证明忙碌 production step 接收");
      const playerReceipt = playerBeforeNpc.players.find((receipt) => receipt.owner === 0 && receipt.intent.PlaceLimit?.qty === 100);
      if (playerReceipt === undefined || playerBeforeNpc.npc_receipts.length !== 0) throw new Error("忙碌 NPC 之前未实际登记 Player，或本轮 NPC 已提前登记");
      observer.verification_ingress_release(token);
      const first = requireFrame((await busyStep).result, 1);
      const firstOrders = (await request(owner, { type: "orders" })).result;
      if (firstOrders.length !== 0) throw new Error(`忙碌决策期间 cutoff 后登记的 Player 错误进入 tick 1：${JSON.stringify(firstOrders)}`);
      const afterNpc = observer.verification_ingress_snapshot(token);
      const npcReceipt = afterNpc.npc_receipts.find(({ receipt }) => receipt.owner === 1 && receipt.intent.PlaceLimit?.code === "600101");
      if (npcReceipt === undefined || BigInt(playerReceipt.stock_ordinal) >= BigInt(npcReceipt.receipt.stock_ordinal)) throw new Error(`真实 NPC 未出单或跨来源股票 receipt 次序错误：${JSON.stringify(afterNpc)}`);
      observer.verification_ingress_arm(token, "cutoff", 1n, undefined);
      const cutoffStep = request(owner, { type: "step" });
      await waitEntered(observer, token, "cutoff");
      requireIntake(await request(intake, { type: "enqueue", generation: 1, intent: buy(200) }), "enqueued");
      const cutoffStatus = observer.verification_ingress_snapshot(token);
      if (!cutoffStatus.entered || cutoffStatus.released) throw new Error("cutoff 已释放，无法证明截止后登记");
      const frozen = cutoffStatus.cutoffs.find((cutoff) => cutoff.observed_tick === 1);
      if (frozen === undefined || frozen.receipts.length !== 1 || frozen.receipts[0].intent.PlaceLimit?.qty !== 100 || frozen.receipts[0].stock_ordinal !== playerReceipt.stock_ordinal) throw new Error(`tick 2 cutoff 没有冻结真实 100 股 receipt：${JSON.stringify(cutoffStatus)}`);
      const late = cutoffStatus.players.find((receipt) => receipt.intent.PlaceLimit?.qty === 200);
      if (late === undefined || BigInt(late.account_ordinal) <= BigInt(playerReceipt.account_ordinal) || BigInt(late.stock_ordinal) <= BigInt(npcReceipt.receipt.stock_ordinal)) throw new Error("截止后 200 股请求未分配晚于先前 Player/NPC 的真实 receipt");
      observer.verification_ingress_release(token);
      const second = requireFrame((await cutoffStep).result, 2);
      const accepted = second.events.filter((event) => event.OrderAccepted !== undefined).map((event) => event.OrderAccepted);
      const npcIntent = npcReceipt.receipt.intent.PlaceLimit;
      const npcAccepted = accepted.filter((order) => order.account === 1 && order.code === npcIntent.code && order.side === npcIntent.side && order.remaining_qty === npcIntent.qty && (npcIntent.price.Fixed === undefined || order.price === npcIntent.price.Fixed));
      const playerAccepted = accepted.filter((order) => order.account === 0);
      if (npcAccepted.length !== 1 || playerAccepted.length !== 1 || playerAccepted[0].code !== "600101" || playerAccepted[0].side !== "Buy" || playerAccepted[0].remaining_qty !== 100 || (!samePrice && playerAccepted[0].price !== "1008")) throw new Error(`真实 NPC/Player receipts 未产生唯一对应受理：${JSON.stringify(second.events)}`);
      if (samePrice && (npcAccepted[0].side !== "Buy" || playerAccepted[0].side !== "Buy" || npcAccepted[0].price !== playerAccepted[0].price)) throw new Error(`独立同价 witness 未产生真实同向同价委托：${JSON.stringify(accepted)}`);
      const playerBook = (await request(owner, { type: "book", account: 0n, code: "600101" })).result;
      const npcBook = (await request(owner, { type: "book", account: 1n, code: "600101" })).result;
      const playerOrder = playerBook.find((order) => order.id === playerAccepted[0].id);
      const npcOrder = npcBook.find((order) => order.id === npcAccepted[0].id);
      if (playerOrder === undefined || npcOrder === undefined || playerOrder.owner !== 0 || npcOrder.owner !== 1 || playerOrder.side !== playerAccepted[0].side || playerOrder.price !== playerAccepted[0].price || playerOrder.qty !== playerAccepted[0].remaining_qty || npcOrder.side !== npcAccepted[0].side || npcOrder.price !== npcAccepted[0].price || npcOrder.qty !== npcAccepted[0].remaining_qty || playerOrder.seq >= npcOrder.seq) throw new Error(`真实股票 book 委托或时间序未保留对应受理与 Player→NPC 入口次序：${JSON.stringify({ playerBook, npcBook })}`);
      const secondOrders = (await request(owner, { type: "orders" })).result;
      if (secondOrders.length !== 1 || secondOrders[0].remainingQty !== 100 || secondOrders[0].price !== playerAccepted[0].price) throw new Error(`cutoff 后输入错误进入当前 tick：${JSON.stringify(secondOrders)}`);
      const third = requireFrame((await request(owner, { type: "step" })).result, 3);
      const lateAccepted = third.events.filter((event) => event.OrderAccepted?.account === 0).map((event) => event.OrderAccepted);
      if (lateAccepted.length !== 1 || lateAccepted[0].code !== "600101" || lateAccepted[0].side !== "Buy" || lateAccepted[0].remaining_qty !== 200 || lateAccepted[0].price !== playerAccepted[0].price) throw new Error(`迟到 Player receipt 未产生唯一对应受理：${JSON.stringify(third.events)}`);
      const thirdOrders = (await request(owner, { type: "orders" })).result;
      if (thirdOrders.length !== 2 || thirdOrders.filter((order) => order.remainingQty === 100).length !== 1 || thirdOrders.filter((order) => order.remainingQty === 200).length !== 1) throw new Error(`下一 tick 未 exactly-once 受理完整 payload：${JSON.stringify(thirdOrders)}`);
      const finalTrace = observer.verification_ingress_snapshot(token);
      const nextCutoff = finalTrace.cutoffs.find((cutoff) => cutoff.observed_tick === 2);
      if (nextCutoff === undefined || nextCutoff.receipts.length !== 1 || nextCutoff.receipts[0].intent.PlaceLimit?.qty !== 200 || nextCutoff.receipts[0].stock_ordinal !== late.stock_ordinal) throw new Error(`tick 3 未 exactly-once 消费真实迟到 receipt：${JSON.stringify(finalTrace)}`);
      return { threads: threadCount, productionStepBusyRuntimeVerified: true, npcCompetitionRuntimeVerified: true, cutoffRuntimeVerified: true, samePriceTimePriorityRuntimeVerified: samePrice, priceCageEnabled: setup.config.price_cage_enabled, floatShares: setup.stocks[0].float_shares, playerBeforeNpc, afterNpc, finalTrace, first, second, third, firstOrders, secondOrders, thirdOrders, playerBook, npcBook };
    } finally {
      try {
        if (observerInitialized && token !== undefined) {
          observer.verification_ingress_release(token);
        }
      } finally {
        owner.terminate();
        intake.terminate();
        URL.revokeObjectURL(ownerUrl);
      }
    }
  }, { threadCount: threads, fixtureSeed: seed.toString(), samePrice: samePriceWitness });
  console.log(JSON.stringify(result, null, 2));
} finally {
  if (browser !== undefined) await browser.close();
  await server.close();
}
