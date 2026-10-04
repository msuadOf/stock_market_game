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
    console.log(JSON.stringify({ kind: "Pages browser smoke", contexts: 2, wasmThreads: workers, cpuCount: availableParallelism(), isolationHeadersAtOrigin: false }));
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
            if (message.type === "init") message = { ...message, threads: count };
            if (message.type === "create") {
              message = { ...message, setup: { ...message.setup, stocks: message.setup.stocks.slice(0, 2),
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
        assert.equal(await page.evaluate(() => crossOriginIsolated && typeof SharedArrayBuffer === "function"), true);
        assert.equal(new URL(page.url()).searchParams.has("__stock_pages_isolation"), false);
        assert.equal(await page.evaluate(() => window.__PAGES_SAVE_READS__), 0, "isolation bootstrap must not read the quick save");
        assert.equal(await page.evaluate(() => "__STOCK_GAME_E2E__" in window), false);
        if (entry === "") {
          await page.getByRole("button", { name: "启动游戏", exact: true }).click();
          await page.locator(".app-root").waitFor({ timeout: 15000 });
          console.log("Representative two-stock/seven-NPC session loaded in actual WASM Workers (test-only input instrumentation, unchanged production assets).");
          assert.equal(await page.locator(".app-error").count(), 0);
          assert.equal(page.workers().length, workers + 1, "expected one main WASM Worker and the configured Rayon pool");
          const mainWorker = page.workers().find((worker) => /\/wasm-worker-[^/]+\.js$/.test(new URL(worker.url()).pathname));
          assert.ok(mainWorker, "missing main WASM Worker");
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
  console.log("Pages smoke passed including cleanup: repository subpath, first-visit isolation, SharedArrayBuffer, representative local game, real threaded Workers and direct index.html.");
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const argv = process.argv.slice(2);
  if (argv.length !== 4 || argv[0] !== "--input" || argv[2] !== "--base") throw new Error("usage: smoke-pages.mjs --input apps/web/dist-pages --base /REPOSITORY/");
  smokePages(argv[1], argv[3]).catch((error) => { console.error(`Pages browser smoke failed: ${error.stack}`); process.exitCode = 1; });
}
