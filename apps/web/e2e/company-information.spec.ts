import { expect, test, type Page } from "@playwright/test";
import { gunzipSync } from "node:zlib";
import { readQuickArchive } from "./quick-archive.ts";

test.beforeEach(async ({ page }) => {
  // 精确披露日期、公开编号和金额来自 seed42；仅固定测试页的新局熵输入。
  // 首局使用现有 tradingE2E 的 seed42/零 NPC fixture；固定熵也覆盖手动新局。
  // 不替换真实 WASM、自然日历或公开报告查询返回值。
  await page.addInitScript({ content: `
    {
      const originalGetRandomValues = Crypto.prototype.getRandomValues;
      Crypto.prototype.getRandomValues = function(array) {
        if (array instanceof Uint32Array && array.length === 2) {
          array.set([0, 42]);
          return array;
        }
        return originalGetRandomValues.call(this, array);
      };
    }
  ` });
});

async function expectEngineReady(page: Page): Promise<void> {
  await expect(page.locator(".app-root")).toBeVisible({ timeout: 30_000 });
  await expect(page.locator(".app-error")).toHaveCount(0);
}

function companyPanel(page: Page) {
  return page.getByRole("region", { name: "公司信息" });
}

async function expectPublicReportReady(page: Page): Promise<void> {
  const panel = companyPanel(page);
  await panel.evaluate((element) => element.scrollIntoView({ block: "start" }));
  await expect(panel.getByRole("list", { name: "公开报告列表" })).toBeVisible({ timeout: 30_000 });
  await expect(panel.getByRole("alert")).toHaveCount(0);
}

test("桌面端以真实 WASM 报告渲染规范期间、版本、四张表和附注", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto("/?tradingE2E=1");
  await expectEngineReady(page);
  await page.getByRole("navigation", {name:"桌面主导航"}).getByRole("button", {name:"个股", exact:true}).click();
  await page.getByRole("button", {name:"公司资料 F10", exact:true}).click();
  await expectPublicReportReady(page);

  const panel = companyPanel(page);
  await expect(panel.getByRole("listitem")).toHaveCount(7);
  await expect(panel).toContainText("季度报告 · 2028-03-31");
  await expect(panel).toContainText("公开编号 1 · 版本 1");
  await expect(panel).toContainText("2028-04-22 18:00 发布 · 版本 1");
  await expect(panel).toContainText("暂无上年同期：无上年历史");
  await expect(panel).toContainText("单体 · C-600101");
  await expect(panel.getByRole("table", { name: "已披露科目明细" })).toContainText("1002 · 银行存款");
  await expect(panel.getByRole("table", { name: "已披露科目明细" })).toContainText("报告窗口净借方变动");

  const statementTabs = ["资产负债表", "利润表", "现金流量表", "所有者权益变动表"] as const;
  for (const name of statementTabs) {
    await panel.getByRole("tab", { name, exact: true }).click();
    await expect(panel.getByRole("tabpanel", { name })).toBeVisible();
    await expect(panel.getByRole("table", { name })).toBeVisible();
  }
  await panel.getByRole("tab", { name: "利润表", exact: true }).click();
  const incomeTable = panel.getByRole("table", { name: "利润表" });
  await expect(incomeTable).toContainText("营业收入");
  await expect(incomeTable).toContainText("净利润");
  await expect(incomeTable.getByRole("columnheader", { name: "当季", exact: true })).toBeVisible();
  await expect(incomeTable.getByRole("columnheader", { name: "年初至今累计", exact: true })).toBeVisible();
  await expect(incomeTable.getByRole("columnheader", { name: "上年同期（报告窗口）", exact: true })).toBeVisible();
  await expect(incomeTable.locator("caption")).toHaveText("金额（缩写，元/万元/亿元）");
  await panel.getByRole("button", { name: "查看精确值" }).click();
  await expect(incomeTable.locator("caption")).toHaveText("金额（元，精确值）");
  // 期末折旧接入后的 seed42 gold；独立反事实与科目勾稽见 company-causal-audit.md。
  await expect(incomeTable.getByRole("row", { name: /管理费用/ })).toContainText("2059278651.49");
  await expect(incomeTable.getByRole("row", { name: /减值损失/ })).toContainText("411543090.41");
  await expect(incomeTable.getByRole("row", { name: /净利润/ })).toContainText("12822166575.42");
  await expect(panel.getByRole("table", { name: "已披露科目明细" }).getByRole("row", { name: /1602.*累计折旧/ })).toContainText("-109278690.49");
  await page.screenshot({ path: "../../.tmp/evidence/company-information/statement-details/desktop-1280.png" });
});

test("平板端以真实 WASM 切换公司和报告期间并保持规范公开内容", async ({ page }) => {
  await page.setViewportSize({ width: 768, height: 700 });
  await page.goto("/?tradingE2E=1");
  await expectEngineReady(page);
  await page.getByRole("navigation", {name:"桌面主导航"}).getByRole("button", {name:"个股", exact:true}).click();
  await page.getByRole("button", {name:"公司资料 F10", exact:true}).click();
  await expectPublicReportReady(page);

  const panel = companyPanel(page);
  await page.getByRole("navigation", {name:"个股列表"}).getByRole("button", {name:/芯片科技/}).click();
  await expect(panel).toHaveAttribute("data-company-id", "C-002156");
  await expect(panel.getByRole("list", { name: "公开报告列表" })).toBeVisible();
  await expect(panel).toContainText("芯片科技股份有限公司");
  await expect(panel).toContainText("季度报告 · 2028-03-31");
  const semiannualReport = panel.getByRole("button", { name: "半年度报告 · 2028-06-30 2028-08-21 18:00 发布 · 版本 1", exact: true });
  await semiannualReport.click();
  await expect(semiannualReport).toHaveAttribute("aria-pressed", "true");
  await expect(panel).toContainText("公开编号 9 · 版本 1");
  await expect(panel).toContainText("半年度报告 · 期间 2028-06-30");
  await panel.getByRole("tab", { name: "资产负债表", exact: true }).focus();
  await page.keyboard.press("ArrowRight");
  await expect(panel.getByRole("tab", { name: "利润表", exact: true })).toBeFocused();
  await expect(semiannualReport).toHaveAttribute("aria-pressed", "true");
  await expect(panel).toContainText("半年度报告 · 期间 2028-06-30");
  expect(await page.locator("html").evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(true);
  await page.screenshot({ path: "../../.tmp/evidence/company-information/company-and-report-selection/tablet-768.png" });
});

test("移动端复用真实 WASM 公司内容并保持表格滚动在面板内", async ({ page }) => {
  await page.setViewportSize({ width: 375, height: 812 });
  await page.goto("/?tradingE2E=1");
  await expectEngineReady(page);
  await page.locator(".mobile-market-row").first().click();
  await page.getByRole("tab", { name: "财务", exact: true }).click();
  await expectPublicReportReady(page);
  const panel = companyPanel(page);
  await expect(panel).toHaveAttribute("data-company-id", "C-600101");
  await expect(panel).toContainText("季度报告 · 2028-03-31");
  await panel.getByRole("tab", { name: "现金流量表", exact: true }).click();
  const tableWrap = panel.getByTestId("company-statement-cash-flow");
  await expect(tableWrap).toBeVisible();
  expect(await tableWrap.evaluate((element) => element.scrollWidth >= element.clientWidth)).toBe(true);
  await panel.getByRole("button", { name: "推进模拟自然日" }).focus();
  await expect(panel.getByRole("button", { name: "推进模拟自然日" })).toBeFocused();
  expect(await page.locator("html").evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(true);
  await page.screenshot({ path: "../../.tmp/evidence/company-information/mobile-statement-scroll/mobile-375.png" });
});

test("新游戏默认 2030，拒绝无效日期并在有效日期重新创建真实 WASM 会话", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto("/?tradingE2E=1");
  await expectEngineReady(page);
  await page.getByRole("navigation", {name:"桌面主导航"}).getByRole("button", {name:"个股", exact:true}).click();
  await page.getByRole("button", {name:"公司资料 F10", exact:true}).click();
  await expectPublicReportReady(page);
  await page.getByRole("button", {name:"游戏与存档"}).click();
  const startDate = page.getByLabel("模拟起始日期").first();
  await expect(startDate).toHaveValue("2030-01-01");
  await startDate.focus();
  await expect(startDate).toBeFocused();
  await expect(page.locator("#section-company .company-panel")).toHaveAttribute("data-company-id", "C-600101");
  await startDate.fill("");
  await expect(startDate).toHaveValue("");
  await page.getByRole("button", { name: "创建新游戏", exact: true }).click();
  await expect(page.getByRole("alert").filter({ hasText: "模拟起始日期不是有效公历日" })).toBeVisible();
  await expect(page.locator("#section-company .company-panel")).toHaveAttribute("data-company-id", "C-600101");
  await startDate.evaluate((element) => {
    const input = element as { value: string; dispatchEvent(event: Event): boolean };
    input.value = "2030-02-30";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(new Event("change", { bubbles: true }));
  });
  await page.getByRole("button", { name: "创建新游戏", exact: true }).click();
  await expect(page.getByRole("alert").filter({ hasText: "模拟起始日期不是有效公历日" })).toBeVisible();
  await expect(page.locator("#section-company .company-panel")).toHaveAttribute("data-company-id", "C-600101");
  await startDate.fill("2031-01-01");
  await page.getByRole("button", { name: "创建新游戏", exact: true }).click();
  await expectEngineReady(page);
  await expect(page.getByRole("status").filter({ hasText: "已按 2031-01-01 创建新模拟会话" })).toBeVisible();
  await page.getByRole("navigation", {name:"桌面主导航"}).getByRole("button", {name:"个股", exact:true}).click();
  await page.getByRole("button", {name:"公司资料 F10", exact:true}).click();
  await expectPublicReportReady(page);
  await expect(companyPanel(page)).toContainText("2031-01-01");
  await expect(companyPanel(page)).toContainText("季度报告 · 2029-03-31");
  await page.screenshot({ path: "../../.tmp/evidence/company-information/new-game-disclosure/new-game-1280.png" });
});

test("生产 WASM 拒绝无效公开报告查询而不伪造报告内容", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.addInitScript({ content: `
    {
      const originalPostMessage = Worker.prototype.postMessage;
      Worker.prototype.postMessage = function(message, transfer) {
        if (message?.type === "publicReports" && message?.query?.company_id === "C-600101") {
          return originalPostMessage.call(this, {
            ...message,
            query: { ...message.query, page_size: 0 },
          }, transfer);
        }
        return originalPostMessage.call(this, message, transfer);
      };
    }
  ` });
  await page.goto("/?tradingE2E=1");
  await expectEngineReady(page);
  await page.getByRole("navigation", {name:"桌面主导航"}).getByRole("button", {name:"个股", exact:true}).click();
  await page.getByRole("button", {name:"公司资料 F10", exact:true}).click();
  const panel = companyPanel(page);
  await panel.scrollIntoViewIfNeeded();
  await expect(panel.getByRole("alert")).toContainText("公开报告查询失败：public report page size 0 outside 1..=100");
});

test("日终读档后公司资料立即显示存档自然日，横竖屏一致", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto("/?tradingE2E=1");
  await expectEngineReady(page);
  await page.evaluate(async () => {
    const controls = (globalThis as typeof globalThis & { __STOCK_GAME_E2E__?: { advanceToTick(tick: number): Promise<number> } }).__STOCK_GAME_E2E__;
    if (!controls) throw new Error("缺少真实 WASM 单步控制");
    await controls.advanceToTick(31);
  });
  await expect(page.getByRole("status").filter({ hasText: "日终存档已更新" })).toBeVisible();
  // 旧自然日的成功提示可能仍在；以目标日终档的 IndexedDB 提交为读取前提。
  await expect.poll(async () => {
    const raw = await readQuickArchive(page);
    if (raw === null) return null;
    expect(raw.startsWith("gzip:")).toBe(true);
    const slot = JSON.parse(gunzipSync(Buffer.from(raw.slice(5), "base64")).toString("utf8")) as {
      snapshot: { tick: number };
      civil_clock: { current_date: string };
    };
    return { tick: slot.snapshot.tick, civilDate: slot.civil_clock.current_date };
  }).toEqual({ tick: 30, civilDate: "2030-01-03" });
  await page.getByRole("button", { name: "游戏与存档", exact: true }).click();
  await page.getByRole("button", { name: "读取本地进度", exact: true }).click();
  await expect(page.getByRole("status").filter({ hasText: "已读档" })).toBeVisible();
  await page.getByRole("navigation", { name: "桌面主导航" }).getByRole("button", { name: "个股", exact: true }).click();
  await page.getByRole("button", { name: "公司资料 F10", exact: true }).click();
  await expectPublicReportReady(page);
  await expect(companyPanel(page).getByLabel("当前模拟日历")).toContainText("2030-01-03");
  await page.setViewportSize({ width: 375, height: 812 });
  await page.locator(".mobile-market-row").first().click();
  await page.getByRole("tab", { name: "财务", exact: true }).click();
  await expectPublicReportReady(page);
  await expect(companyPanel(page).getByLabel("当前模拟日历")).toContainText("2030-01-03");
});

test("公司阅读选择按公司保存，切股往返保持报告、报表及金额显示", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto("/?tradingE2E=1");
  await expectEngineReady(page);
  await page.getByRole("navigation", { name: "桌面主导航" }).getByRole("button", { name: "个股", exact: true }).click();
  await page.getByRole("button", { name: "公司资料 F10", exact: true }).click();
  await expectPublicReportReady(page);
  const panel = companyPanel(page);
  const firstReport = panel.getByRole("button", { name: /^半年度报告 · 2028-06-30 / });
  await firstReport.click();
  await panel.getByRole("tab", { name: "利润表", exact: true }).click();
  await panel.getByRole("button", { name: "查看精确值", exact: true }).click();
  const stocks = page.getByRole("navigation", { name: "个股列表" });
  await stocks.getByRole("button", { name: /芯片科技/ }).click();
  await expectPublicReportReady(page);
  await expect(panel.getByRole("tab", { name: "资产负债表", exact: true })).toHaveAttribute("aria-selected", "true");
  await expect(panel.getByRole("button", { name: "查看精确值", exact: true })).toBeVisible();
  const secondReport = panel.getByRole("button", { name: /^年度报告 · 2028-12-31 / });
  await secondReport.click();
  await panel.getByRole("tab", { name: "现金流量表", exact: true }).click();
  await stocks.getByRole("button", { name: /稳健实业/ }).click();
  await expect(firstReport).toHaveAttribute("aria-pressed", "true");
  await expect(panel.getByRole("tab", { name: "利润表", exact: true })).toHaveAttribute("aria-selected", "true");
  await expect(panel.getByRole("table", { name: "利润表", exact: true }).locator("caption")).toHaveText("金额（元，精确值）");
  await stocks.getByRole("button", { name: /芯片科技/ }).click();
  await expect(secondReport).toHaveAttribute("aria-pressed", "true");
  await expect(panel.getByRole("tab", { name: "现金流量表", exact: true })).toHaveAttribute("aria-selected", "true");
  await expect(panel.getByRole("button", { name: "查看精确值", exact: true })).toBeVisible();
});

test("公司阅读选择在横竖屏间共用，重新进入页面保持双向修改", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto("/?tradingE2E=1");
  await expectEngineReady(page);
  await page.getByRole("navigation", { name: "桌面主导航" }).getByRole("button", { name: "个股", exact: true }).click();
  await page.getByRole("button", { name: "公司资料 F10", exact: true }).click();
  await expectPublicReportReady(page);
  const panel = companyPanel(page);
  const semiannual = panel.getByRole("button", { name: /^半年度报告 · 2028-06-30 / });
  await semiannual.click();
  await panel.getByRole("tab", { name: "利润表", exact: true }).click();
  await panel.getByRole("button", { name: "查看精确值", exact: true }).click();
  await page.setViewportSize({ width: 375, height: 812 });
  await page.locator(".mobile-market-row").first().click();
  await page.getByRole("tab", { name: "财务", exact: true }).click();
  await expectPublicReportReady(page);
  await expect(semiannual).toHaveAttribute("aria-pressed", "true");
  await expect(panel.getByRole("tab", { name: "利润表", exact: true })).toHaveAttribute("aria-selected", "true");
  await expect(panel.getByRole("button", { name: "显示缩写金额", exact: true })).toBeVisible();
  const annual = panel.getByRole("button", { name: /^年度报告 · 2028-12-31 / });
  await annual.click();
  await panel.getByRole("tab", { name: "现金流量表", exact: true }).click();
  await panel.getByRole("button", { name: "显示缩写金额", exact: true }).click();
  await page.setViewportSize({ width: 1280, height: 900 });
  await expect(annual).toHaveAttribute("aria-pressed", "true");
  await expect(panel.getByRole("tab", { name: "现金流量表", exact: true })).toHaveAttribute("aria-selected", "true");
  await expect(panel.getByRole("button", { name: "查看精确值", exact: true })).toBeVisible();
});
