import { expect, test, type Page } from "@playwright/test";

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
  await page.goto("/");
  await expectEngineReady(page);
  await expectPublicReportReady(page);

  const panel = companyPanel(page);
  await expect(panel.getByRole("listitem")).toHaveCount(7);
  await expect(panel).toContainText("季度报告 · 2028-03-31");
  await expect(panel).toContainText("公开编号 1 · 版本 1");
  await expect(panel).toContainText("2028-04-22 18:00 发布 · 版本 1");
  await expect(panel).toContainText("暂无上年同期：无上年历史");
  await expect(panel).toContainText("公开摘要未提供单体或合并范围");

  const statementTabs = ["资产负债表", "利润表", "现金流量表", "所有者权益变动表"] as const;
  for (const name of statementTabs) {
    await panel.getByRole("tab", { name, exact: true }).click();
    await expect(panel.getByRole("tabpanel", { name })).toBeVisible();
    await expect(panel.getByRole("table", { name })).toBeVisible();
  }
  await panel.getByRole("tab", { name: "利润表", exact: true }).click();
  await expect(panel.getByRole("table", { name: "利润表" })).toContainText("累计净利润");
  await expect(panel.getByRole("columnheader", { name: "金额（缩写）" })).toBeVisible();
  await panel.getByRole("button", { name: "查看精确值" }).click();
  await expect(panel.getByRole("columnheader", { name: "金额（元，精确值）" })).toBeVisible();
  await expect(panel.getByRole("table", { name: "利润表" })).toContainText("12928574075.43");
  await page.screenshot({ path: "../../.omo/evidence/company-information-npc-intentions/task-34-happy/desktop-1280.png" });
});

test("平板端以真实 WASM 切换公司和报告期间并保持规范公开内容", async ({ page }) => {
  await page.setViewportSize({ width: 768, height: 700 });
  await page.goto("/");
  await expectEngineReady(page);
  await expectPublicReportReady(page);

  const panel = companyPanel(page);
  await panel.getByLabel("选择公司").selectOption("C-002156");
  await expect(panel).toHaveAttribute("data-company-id", "C-002156");
  await expect(panel.getByRole("list", { name: "公开报告列表" })).toBeVisible();
  await expect(panel).toContainText("芯片科技股份有限公司");
  await expect(panel).toContainText("季度报告 · 2028-03-31");
  await panel.getByRole("listitem").filter({ hasText: "半年度报告 · 2028-06-30" }).click();
  await expect(panel).toContainText("公开编号 9 · 版本 1");
  await expect(panel).toContainText("半年度报告 · 期间 2028-06-30");
  await panel.getByRole("tab", { name: "资产负债表", exact: true }).focus();
  await page.keyboard.press("ArrowRight");
  await expect(panel.getByRole("tab", { name: "利润表", exact: true })).toBeFocused();
  expect(await page.locator("html").evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(true);
  await page.screenshot({ path: "../../.omo/evidence/company-information-npc-intentions/task-34-happy/tablet-768.png" });
});

test("移动端复用真实 WASM 公司内容并保持表格滚动在面板内", async ({ page }) => {
  await page.setViewportSize({ width: 375, height: 812 });
  await page.goto("/");
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
  await page.screenshot({ path: "../../.omo/evidence/company-information-npc-intentions/task-34-happy/mobile-375.png" });
});

test("新游戏默认 2030，拒绝无效日期并在有效日期重新创建真实 WASM 会话", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto("/");
  await expectEngineReady(page);
  await expectPublicReportReady(page);
  const startDate = page.getByLabel("模拟起始日期").first();
  await expect(startDate).toHaveValue("2030-01-01");
  await startDate.focus();
  await expect(startDate).toBeFocused();
  await expect(companyPanel(page)).toHaveAttribute("data-company-id", "C-600101");
  await startDate.fill("");
  await expect(startDate).toHaveValue("");
  await page.getByRole("button", { name: "新游戏", exact: true }).click();
  await expect(page.getByRole("alert").filter({ hasText: "模拟起始日期不是有效公历日" })).toBeVisible();
  await expect(companyPanel(page)).toHaveAttribute("data-company-id", "C-600101");
  await startDate.evaluate((element) => {
    const input = element as { value: string; dispatchEvent(event: Event): boolean };
    input.value = "2030-02-30";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(new Event("change", { bubbles: true }));
  });
  await page.getByRole("button", { name: "新游戏", exact: true }).click();
  await expect(page.getByRole("alert").filter({ hasText: "模拟起始日期不是有效公历日" })).toBeVisible();
  await expect(companyPanel(page)).toHaveAttribute("data-company-id", "C-600101");
  await startDate.fill("2031-01-01");
  await page.getByRole("button", { name: "新游戏", exact: true }).click();
  await expect(page.getByRole("status").filter({ hasText: "已按 2031-01-01 创建新模拟会话" })).toBeVisible();
  await expectEngineReady(page);
  await expectPublicReportReady(page);
  await expect(companyPanel(page)).toContainText("2031-01-01");
  await expect(companyPanel(page)).toContainText("季度报告 · 2029-03-31");
  await page.screenshot({ path: "../../.omo/evidence/company-information-npc-intentions/task-34-happy/new-game-1280.png" });
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
  await page.goto("/");
  await expectEngineReady(page);
  const panel = companyPanel(page);
  await panel.scrollIntoViewIfNeeded();
  await expect(panel.getByRole("alert")).toContainText("公开报告查询失败：public report page size 0 outside 1..=100");
});
