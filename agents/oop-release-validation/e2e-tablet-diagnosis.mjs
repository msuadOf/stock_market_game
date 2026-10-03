import { spawn } from "node:child_process";
import { chromium, expect } from "../../apps/web/node_modules/@playwright/test/index.mjs";

const preview = spawn("pnpm", ["--filter", "web", "preview", "--host", "127.0.0.1", "--port", "4187"], { stdio: "inherit", detached: true });
let browser;
try {
  for (let attempt = 0; attempt < 100; attempt += 1) {
    try {
      const response = await fetch("http://127.0.0.1:4187");
      if (response.ok) break;
      throw new Error(`preview HTTP ${response.status}`);
    } catch (error) {
      if (attempt === 99) throw error;
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
  }
  browser = await chromium.launch();
  await Promise.all([0, 1].map(async (worker) => {
    const page = await browser.newPage({ viewport: { width: 768, height: 700 } });
    await page.addInitScript(() => {
      window.tabletDiagnosis = [];
      const record = (kind, value) => window.tabletDiagnosis.push({ time: performance.now(), kind, value });
      const originalPostMessage = Worker.prototype.postMessage;
      Worker.prototype.postMessage = function (message, transfer) {
        if (message?.type === "publicReports") record("query", message);
        return originalPostMessage.call(this, message, transfer);
      };
      const observer = new MutationObserver((mutations) => {
        for (const mutation of mutations) {
          if (mutation.type === "childList") {
            for (const node of mutation.removedNodes) {
              if (node instanceof Element && (node.matches(".company-panel") || node.querySelector(".company-panel"))) record("removed", node.outerHTML.slice(0, 200));
            }
          }
        }
      });
      observer.observe(document, { subtree: true, childList: true });
      document.addEventListener("click", (event) => {
        if (event.target instanceof Element && event.target.closest(".company-panel")) record("click", event.target.outerHTML.slice(0, 300));
      }, true);
    });
    await page.goto("http://127.0.0.1:4187");
    const panel = page.getByRole("region", { name: "公司信息" });
    await expect(panel.getByRole("list", { name: "公开报告列表" })).toBeVisible({ timeout: 30000 });
    await panel.evaluate((element) => element.scrollIntoView({ block: "start" }));
    await panel.getByLabel("选择公司").selectOption("C-002156");
    await expect(panel.getByRole("list", { name: "公开报告列表" })).toBeVisible();
    for (let attempt = 0; attempt < 15; attempt += 1) {
      await panel.getByRole("listitem").filter({ hasText: "半年度报告 · 2028-06-30" }).click();
      await expect(panel).toContainText("公开编号 9 · 版本 1");
      // 观察多个真实刷新周期，只用于排查；不替代产品断言或验收。
      await page.waitForTimeout(700);
      const summary = await panel.locator(".company-report-summary").innerText();
      console.log(JSON.stringify({ worker, attempt, summary, events: await page.evaluate(() => window.tabletDiagnosis) }));
      await page.evaluate(() => { window.tabletDiagnosis = []; });
      await panel.getByRole("listitem").filter({ hasText: "季度报告 · 2028-03-31" }).click();
    }
    await page.close();
  }));
} finally {
  await browser?.close();
  if (preview.pid !== undefined) {
    try { process.kill(-preview.pid, "SIGKILL"); }
    catch (error) { if (error.code !== "ESRCH") throw error; }
  }
}
