import assert from "node:assert/strict";
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { availableParallelism } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "../apps/web/node_modules/@playwright/test/index.mjs";
import { verifyWebReleaseWasm } from "./check-web-release-wasm.mjs";

export async function smokePages(input, base) {
  if (base !== base.trim() || !/^\/(?:[A-Za-z0-9_.-]+\/)*$/.test(base) || base.split("/").some((part) => part === "." || part === "..")) throw new Error("unsafe Pages smoke base path");
  const root = path.resolve(input);
  await verifyWebReleaseWasm(root);
  const failures = [];
  const server = createServer((request, response) => {
    void (async () => {
      const url = new URL(request.url, "http://localhost");
      if (!url.pathname.startsWith(base)) { response.writeHead(404).end(); return; }
      const relative = decodeURIComponent(url.pathname.slice(base.length));
      const filename = path.resolve(root, relative.length === 0 ? "index.html" : relative);
      if (!filename.startsWith(`${root}${path.sep}`)) throw new Error(`static smoke path escapes root: ${request.url}`);
      const extension = path.extname(filename);
      const contentType = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".json": "application/json", ".wasm": "application/wasm", ".svg": "image/svg+xml", ".woff2": "font/woff2" }[extension];
      const bytes = await readFile(filename);
      response.writeHead(200, { "Content-Type": contentType === undefined ? "application/octet-stream" : contentType, "Cache-Control": "no-store" });
      response.end(bytes);
    })().catch((error) => {
      failures.push(String(error));
      response.writeHead(500).end(String(error));
    });
  });
  await new Promise((resolve, reject) => { server.once("error", reject); server.listen(0, "127.0.0.1", resolve); });
  let browser;
  try {
    const workers = Math.max(2, Math.min(4, availableParallelism()));
    // wasm P0 清单 5：恢复 wasmThreads / isolationHeadersAtOrigin 日志字段。
    // 串行 wasm=单 WASM Worker（无 Rayon 池），真线程批落地时恢复池数；
    // 静态伺服源不发隔离头，隔离由 pages-isolation.js SW 注入，故 false。
    console.log(JSON.stringify({ kind: "Pages browser smoke", contexts: 2, wasmThreads: 1, cpuCount: availableParallelism(), isolationHeadersAtOrigin: false }));
    browser = await chromium.launch({ headless: true });
    const origin = `http://127.0.0.1:${server.address().port}`;
    await Promise.all(["", "index.html"].map(async (entry) => {
      const context = await browser.newContext();
      try {
        await context.addInitScript((count) => {
          Object.defineProperty(navigator, "hardwareConcurrency", { get: () => count });
          window.__PAGES_SAVE_READS__ = 0;
          const getItem = Storage.prototype.getItem;
          Storage.prototype.getItem = function (key) {
            if (key === "stock-game-save") window.__PAGES_SAVE_READS__++;
            return getItem.call(this, key);
          };
          const postMessage = Worker.prototype.postMessage;
          Worker.prototype.postMessage = function (message, ...options) {
            if (message.type === "create") {
              // 不裁剪 stocks：initialAllocation 的 expectedCodes 取自主线程
              // createWorkerHost 闭包里的完整 setup，裁剪 engine 侧股票集合会令
              // 两者必然错配（初始分配响应"遗漏本局证券"）。小局提速由
              // npcs/history_len/ticks 裁剪承担（该 stocks 裁剪自 6ac55d83 引入、
              // 7a94c401 保留，d803b937 起局断裂后从未被端到端执行过——一直被
              // ingress 桥 DataCloneError 挡住，w3-bridge-fix 批修复后首次暴露
              // 并在此更正）。
              message = { ...message, setup: { ...message.setup,
                npcs: { ...message.setup.npcs, retail_count: 4, inst_count: 2, hot_count: 1 },
                float_allocation: { between_kinds: "Random", within_kind: "Random" }, history_len: 2, ticks_per_day: 30, auction_ticks: 9, closing_auction_ticks: 3 } };
            }
            return postMessage.call(this, message, ...options);
          };
        }, workers);
        const page = await context.newPage();
        const errors = [];
        page.on("pageerror", (error) => { errors.push(String(error)); console.error(error); });
        await page.goto(`${origin}${base}${entry}`);
        await page.getByRole("heading", { name: "股票模拟游戏 · 启动选择" }).waitFor({ timeout: 15000 });
        console.log(`Pages bootstrap ready: ${entry || "repository root"}`);
        // wasm P0 清单 5：恢复批 9 A10 收尾删除的三断言——SW 注入的 COOP/COEP
        // 必须真实生效（跨源隔离 + SharedArrayBuffer 可用），首访刷新后的
        // __stock_pages_isolation 标记必须已清理。串行 wasm 下隔离链与引擎
        // 线程面无关，断言口径确定。
        assert.equal(await page.evaluate(() => crossOriginIsolated && typeof SharedArrayBuffer === "function"), true);
        assert.equal(new URL(page.url()).searchParams.has("__stock_pages_isolation"), false);
        assert.equal(await page.evaluate(() => window.__PAGES_SAVE_READS__), 0, "bootstrap must not read the quick save");
        assert.equal(await page.evaluate(() => "__STOCK_GAME_E2E__" in window), false);
        if (entry === "") {
          await page.getByRole("button", { name: "启动游戏", exact: true }).click();
          await page.locator(".app-root").waitFor({ timeout: 15000 });
          console.log("Representative five-stock/seven-NPC session loaded in actual WASM Worker (test-only input instrumentation, unchanged production assets).");
          assert.equal(await page.locator(".app-error").count(), 0);
          const mainWorker = page.workers().find((worker) => /\/wasm-worker-[^/]+\.js$/.test(new URL(worker.url()).pathname));
          assert.ok(mainWorker, "missing main WASM Worker");
          // wasm P0 清单 5：主 WASM Worker 同处跨源隔离的浏览上下文组——
          // 真线程批（共享 linear memory）依赖该环境。wasm-bindgen-rayon 池
          // 计数断言（workers + 1）与 init 线程注入随 Rayon 池架构退役不复刻，
          // 真线程批按新池形态重写。
          assert.equal(await mainWorker.evaluate(() => crossOriginIsolated), true);
        }
        assert.deepEqual(errors, []);
      } finally { await context.close(); }
    }));
    assert.deepEqual(failures, []);
  } finally {
    try { if (browser) await browser.close(); }
    finally {
      server.closeAllConnections();
      await new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
    }
  }
  // wasm P0 清单 5：尾日志与实际断言面同步勘正——批 9 A10 收尾删除断言时
  // 未同步本行（"real threaded Workers" 失实残留）；当前断言面=隔离/SAB/
  // 标记清理/串行 WASM Worker 会话，Rayon 池断言已随池架构退役。
  console.log("Pages smoke passed including cleanup: repository subpath, first-visit isolation, SharedArrayBuffer, marker-free URL, representative local game in serial WASM Worker, isolated main Worker and direct index.html.");
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const argv = process.argv.slice(2);
  if (argv.length !== 4 || argv[0] !== "--input" || argv[2] !== "--base") throw new Error("usage: smoke-pages.mjs --input apps/web/dist-pages --base /REPOSITORY/");
  smokePages(argv[1], argv[3]).catch((error) => { console.error(`Pages browser smoke failed: ${error.stack}`); process.exitCode = 1; });
}
