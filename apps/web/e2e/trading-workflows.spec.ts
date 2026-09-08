import { expect, test, type Page } from "@playwright/test";

async function expectEngineReady(page: Page): Promise<void> {
  await expect(page.locator(".app-root")).toBeVisible({ timeout: 30_000 });
  await expect(page.locator(".app-error")).toHaveCount(0);
}

test("桌面端可拒绝非整手买单，并完成本地存读档", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/");
  await expectEngineReady(page);
  const firstMarketRow = page
    .getByRole("grid")
    .getByRole("row")
    .filter({ hasText: "600101" })
    .first();
  await expect(firstMarketRow).toBeVisible();
  await expect(firstMarketRow.getByRole("gridcell").nth(2)).toHaveText(/^\d+\.\d{2}$/);

  await page.getByPlaceholder("委托价").fill("11.20");
  await page.getByPlaceholder("买入按手；零股一次卖完").fill("1");
  await page.getByRole("button", { name: "买入", exact: true }).click();
  await expect(page.getByRole("status")).toContainText("买入数量必须是 100 股的整数倍");

  await page.getByTitle("快存到 LocalStorage").click();
  await expect(page.getByRole("status")).toContainText("已存档");
  await page.getByTitle("从 LocalStorage 快读").click();
  await expect(page.getByRole("status")).toContainText("已读档");
});

test("移动端支持详情页键盘切换、交易底页与显式卖出拒绝", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/");
  await expectEngineReady(page);

  await page.locator(".mobile-market-row").first().click();
  const intradayTab = page.getByRole("tab", { name: "分时", exact: true });
  await intradayTab.focus();
  await intradayTab.press("ArrowRight");
  await expect(page.getByRole("tab", { name: "日K", exact: true })).toBeFocused();

  await page.getByRole("button", { name: "交易", exact: true }).click();
  const tradeDialog = page.getByRole("dialog", { name: "交易面板" });
  await expect(tradeDialog).toBeVisible();
  await tradeDialog.getByPlaceholder("买入按手；零股一次卖完").fill("100");
  await tradeDialog.getByRole("button", { name: "卖出", exact: true }).click();
  await expect(page.getByRole("status")).toContainText("可卖数量不足：当前可卖 0 股");

  await page.keyboard.press("Escape");
  await expect(tradeDialog).not.toBeVisible();
});
