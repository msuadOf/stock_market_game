// 真实 Chromium Worker，按 wasm import 采样点注入；不使用墙钟猜测队列与超时的竞态。
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { createServer } from "node:http";
import path from "node:path";

const [wasmPath, chromiumPath] = process.argv.slice(2);
assert.equal(process.argv.length, 4);
assert.ok([wasmPath, chromiumPath].every(path.isAbsolute), "要求探针及 Chromium 的绝对路径");
const bytes = await readFile(wasmPath);
const workerCode = `
async function instantiate(clock, beforeSleep) {
  const memory = new WebAssembly.Memory({ initial: 256, maximum: 32768, shared: true });
  const result = await WebAssembly.instantiateStreaming(fetch("/contract.wasm"), {
    env: { memory, engine_clock_now: clock, wait_contract_before_sleep: beforeSleep },
  });
  return result.instance.exports;
}
async function check(name, variant, coarse) {
  if (!crossOriginIsolated || !(self instanceof DedicatedWorkerGlobalScope)) throw new Error("需要隔离的真实 Worker");
  if (name === "shared" || name === "initialization") {
    let now = 10000;
    const api = await instantiate(() => now += 0.001, () => { throw new Error("零时限合同不应等待"); });
    if (name === "initialization") api.wait_contract_initialization();
    else {
      api.wait_contract_init();
      api.wait_contract_shared();
    }
    return;
  }
  // 每个用例独立 instance、memory、时钟；由等待阶段推进，不钉死取时次数。
  const anchor = 10000;
  let now = anchor;
  let sleeps = 0;
  let eventCount = 0;
  let onNextClock;
  let api;
  function publish() {
    api.wait_contract_publish(variant);
    eventCount++;
  }
  function beforeSleep() {
    sleeps++;
    if (sleeps > 2 || onNextClock) throw new Error("等待边界用例没有向判定点推进");
    // 进入最后等待片之前发布，模拟：首次轮询之后、超时判定之前。
    if (name === "oversleep-ready") publish();
    onNextClock = () => {
      if (variant === 3) now = anchor + (sleeps === 1 ? 10.5 : 10.25);
      else if (variant === 4) now = anchor + 9.5;
      else {
        now = anchor + (name === "oversleep-ready" ? 12 : 11);
        if (variant <= 1 && name !== "oversleep-ready") publish();
      }
    };
  }
  api = await instantiate(() => {
    const transition = onNextClock;
    onNextClock = undefined;
    transition?.();
    return now;
  }, beforeSleep);
  api.wait_contract_init();
  now = anchor + 10;
  api.wait_contract_case(variant, coarse);
  if (sleeps === 0 || onNextClock || eventCount !== (variant <= 1 ? 1 : 0)) throw new Error("没有实际访问预期等待与发布阶段");
}
self.onmessage = ({ data: [name, variant, coarse] }) => {
  check(name, variant, coarse).then(
    () => postMessage({ passed: name }),
    (error) => postMessage({ failure: name + ": " + error.name + ": " + error.message }),
  );
};
`;

const routes = new Map([
  ["/", ["text/html; charset=utf-8", "<!doctype html><title>LibOS 等待合同</title>"]],
  ["/worker.mjs", ["text/javascript; charset=utf-8", workerCode]],
  ["/contract.wasm", ["application/wasm", bytes]],
]);
const server = createServer((req, res) => {
  const resource = req.method === "GET" && routes.get(req.url);
  if (!resource) return res.writeHead(404).end();
  res.writeHead(200, {
    "content-type": resource[0], "cache-control": "no-store",
    "cross-origin-opener-policy": "same-origin", "cross-origin-embedder-policy": "require-corp",
  }).end(resource[1]);
});
let browser;
try {
  await new Promise((done, fail) => {
    server.once("error", fail);
    server.listen(0, "127.0.0.1", done);
  });
  const { chromium } = await import("../../apps/web/node_modules/@playwright/test/index.mjs");
  browser = await chromium.launch({ executablePath: chromiumPath, headless: true, timeout: 20_000 });
  const page = await browser.newPage();
  await page.goto("http://127.0.0.1:" + server.address().port, { timeout: 10_000 });
  const cases = [["shared", -1, 0], ["initialization", -1, 0], ...[0, 1].flatMap((coarse) => [
    ["ready-at-decision", 0, coarse],
    ["oversleep-ready", 0, coarse],
    ["disconnect-at-decision", 1, coarse],
    ["publish-after-decision", 2, coarse],
    ["clock-above-call-start", 3, coarse],
    ["clock-below-call-start", 4, coarse],
  ])];
  const results = await page.evaluate((scenarios) => Promise.all(scenarios.map((scenario) => {
    const worker = new Worker("/worker.mjs", { type: "module" });
    return new Promise((done) => {
      const finish = (result) => {
        clearTimeout(deadline);
        worker.terminate();
        done({ scenario, ...result });
      };
      const deadline = setTimeout(() => finish({ failure: "Worker 测例 10 秒内未完成" }), 10_000);
      worker.onmessage = ({ data }) => finish(data);
      worker.onerror = (event) => finish({ failure: event.message });
      worker.postMessage(scenario);
    });
  })), cases);
  for (const result of results) {
    console.log(JSON.stringify(result));
  }
  assert.deepEqual(results.map(({ passed }) => passed), cases.map(([name]) => name), "真实 Browser Worker 契约失败");
} finally {
  try { await browser?.close(); }
  finally {
    if (server.listening) await new Promise((done, fail) => server.close((error) => error ? fail(error) : done()));
  }
}
