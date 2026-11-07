// 在正式 dist 中验证真实页面启动和游戏时钟推进。
import assert from "node:assert/strict";
import { chromium } from "../apps/web/node_modules/@playwright/test/index.mjs";
import { preview } from "../apps/web/node_modules/vite/dist/node/index.js";
import path from "node:path";

const server = await preview({ root: path.resolve("apps/web"), preview: { host: "127.0.0.1", port: 0 } });
let browser;
try {
  browser = await chromium.launch({ headless: true });
  const page = await browser.newPage();
  await page.goto(server.resolvedUrls.local[0]);
  assert.equal(await page.evaluate(() => crossOriginIsolated), true, "dist 必须跨源隔离");
  await page.getByRole("button", { name: "启动游戏" }).click();
  await page.locator(".simulation-button").waitFor({ state: "visible", timeout: 30000 });
  const clock = page.locator("[role=timer]").first();
  await clock.waitFor({ state: "attached", timeout: 30000 });
  const before = await clock.textContent();
  await page.locator(".speed-select select").selectOption("Infinity");
  await page.waitForFunction((initial) => document.querySelector('[role="timer"]')?.textContent !== initial, before, { timeout: 30000 });
  console.log(JSON.stringify({ dist: true, crossOriginIsolated: true, before, after: await clock.textContent() }));
} finally {
  if (browser) await browser.close();
  await new Promise((resolve, reject) => server.httpServer.close((error) => error ? reject(error) : resolve()));
}
