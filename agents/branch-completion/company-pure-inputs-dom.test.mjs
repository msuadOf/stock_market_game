import assert from "node:assert/strict";
import { test } from "node:test";
import path from "node:path";
import { createServer } from "node:http";
import { fileURLToPath } from "node:url";
import { build } from "../../apps/web/node_modules/vite/dist/node/index.js";
import { chromium, expect } from "../../apps/web/node_modules/@playwright/test/index.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
let server;
let browser;
let browserServer;
let baseUrl;

async function setup() {
  const bundle = await build({
    root: path.join(repoRoot, "apps/web"),
    configFile: false,
    logLevel: "error",
    define: { "process.env.NODE_ENV": JSON.stringify("production") },
    build: { write: false, minify: false, lib: { entry: "virtual:company-input-fixture", name: "CompanyInputFixture", formats: ["iife"] } },
    plugins: [{
      name: "company-input-dom-fixture",
      resolveId(id) { if (id.endsWith("virtual:company-input-fixture")) return "\0company-input-fixture"; },
      load(id) {
        if (id !== "\0company-input-fixture") return;
        return `
          import React from "react";
          import { createRoot } from "react-dom/client";
          import { ParValueInput } from "/src/components/ParValueInput.tsx";
          import { TaxModeInput } from "/src/components/TaxModeInput.tsx";
          const h = React.createElement;
          function Fixture() {
            const [par, setPar] = React.useState("100");
            const [writes, setWrites] = React.useState(0);
            const [bp, setBp] = React.useState(1000);
            const [mode, setMode] = React.useState("FlatWithholding");
            return h("main", null,
              h(ParValueInput, { value: par, onChange(value) { setPar(value); setWrites(n => n + 1); } }),
              h("output", { id: "par" }, par), h("output", { id: "writes" }, String(writes)),
              h(TaxModeInput, { value: mode, onChange: setMode, flatWithholdingBp: bp, onFlatWithholdingBpChange: setBp }),
              h("output", { id: "bp" }, String(bp)), h("button", null, "离开输入框"),
              h("button", { onClick() { setPar("123"); setBp(1234); } }, "外部更新"));
          }
          createRoot(document.getElementById("root")).render(h(Fixture));`;
      },
    }],
  });
  const script = (Array.isArray(bundle) ? bundle[0] : bundle).output.find((item) => item.type === "chunk").code;
  server = createServer((req, res) => {
    res.setHeader("Content-Type", "text/html; charset=utf-8");
    res.end(`<div id="root"></div><script>${script}</script>`);
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  baseUrl = `http://127.0.0.1:${server.address().port}/`;
  browserServer = await chromium.launchServer({ headless: true });
  browser = await chromium.connect(browserServer.wsEndpoint());
}

async function cleanup() {
  if (browserServer) await browserServer.kill();
  if (browser) await browser.close();
  if (server) { server.closeAllConnections(); await new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve())); }
}

/** 两个独立页面并发，验证浏览器实际 input、Enter 与焦点离开事件。 */
test("TaxModeInput 与 ParValueInput 的真实 DOM 编辑和提交", { timeout: 10000 }, async () => {
  try {
    await setup();
    const results = await Promise.allSettled([
      (async () => {
        const page = await browser.newPage();
        const errors = [];
        page.on("pageerror", (error) => { errors.push(error.message); console.error("browser page error:", error.message); });
        page.setDefaultTimeout(1000);
        try {
          await page.goto(new URL("company-input-fixture", baseUrl).href);
          const input = page.getByRole("textbox", { name: "每股面值（元）" });
          const blur = page.getByRole("button", { name: "离开输入框" });
          await input.fill("15.23");
          assert.equal(await page.locator("#par").textContent(), "100");
          await blur.click();
          assert.equal(await page.locator("#par").textContent(), "1523");
          for (const invalid of ["0", "0.001", "92233720368547758.08"]) {
            await input.fill(invalid);
            await blur.click();
            assert.equal(await input.inputValue(), invalid);
            assert.equal(await input.getAttribute("aria-invalid"), "true");
            assert.match(await page.getByRole("alert").textContent(), /已设面值仍为 15.23 元/);
            await input.press("Enter");
            assert.equal(await input.inputValue(), invalid);
            assert.equal(await page.locator("#par").textContent(), "1523");
            assert.equal(await page.locator("#writes").textContent(), "1");
          }
          await input.fill("90071992547409.93");
          await input.press("Enter");
          assert.equal(await page.locator("#par").textContent(), "9007199254740993");
          await blur.click();
          assert.equal(await page.locator("#writes").textContent(), "2");
          assert.equal(await page.getByRole("alert").count(), 0);
          await input.fill("非法文本");
          await page.getByRole("button", { name: "外部更新" }).click();
          await expect(input).toHaveValue("1.23", { timeout: 1000 });
          assert.equal(await page.getByRole("alert").count(), 0);
          assert.equal(await page.locator("#writes").textContent(), "2");
          assert.deepEqual(errors, []);
          console.log("DOM Par：input不提交；blur/Enter精确提交；非法保留原文+alert；Enter后blur不重复写回");
        } finally { await page.close(); }
      })(),
      (async () => {
        const page = await browser.newPage();
        const errors = [];
        page.on("pageerror", (error) => { errors.push(error.message); console.error("browser page error:", error.message); });
        page.setDefaultTimeout(1000);
        try {
          await page.goto(new URL("company-input-fixture", baseUrl).href);
          const input = page.getByRole("textbox", { name: "简税代扣比例（百分比）" });
          for (const invalid of ["-0.004", "100.004", "100.00000000000000001", "0x10", "0b10", "0o10"]) {
            await input.fill(invalid);
            assert.equal(await input.inputValue(), invalid);
            assert.equal(await input.getAttribute("aria-invalid"), "true");
            assert.equal(await page.locator("#bp").textContent(), "1000");
            assert.match(await page.getByRole("alert").textContent(), /已设比例仍为 10%/);
          }
          await input.fill("0.005");
          assert.equal(await page.locator("#bp").textContent(), "1");
          assert.equal(await page.getByRole("alert").count(), 0);
          await input.fill("1.005");
          assert.equal(await page.locator("#bp").textContent(), "101");
          await input.fill("文字");
          await page.getByRole("button", { name: "外部更新" }).click();
          await expect(input).toHaveValue("12.34", { timeout: 1000 });
          assert.equal(await page.getByRole("alert").count(), 0);
          await page.getByRole("radio", { name: /不扣税/ }).check();
          assert.equal(await input.count(), 0);
          assert.match(await page.locator("main").textContent(), /佣金.*过户费照付/);
          assert.deepEqual(errors, []);
          console.log("DOM Tax：越界原百分比拒绝并保留；0.005%→1bp；真实radio切换隐藏简税比例");
        } finally { await page.close(); }
      })(),
    ]);
    for (const result of results) if (result.status === "rejected") throw result.reason;
  } finally { await cleanup(); }
});
