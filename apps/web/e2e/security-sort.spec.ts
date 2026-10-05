import { expect, test } from "@playwright/test";

test.setTimeout(10_000);

test("桌面行情排序后进入个股，左列表及方向键沿用同一顺序", async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 902, height: 833 });
  await page.goto("/?tradingE2E=1");
  const grid = page.getByRole("region", { name: "股票行情", exact: true });
  await grid.getByRole("columnheader", { name: "现价", exact: true }).click();
  await page.getByRole("searchbox", { name: "搜索股票" }).press("Enter");
  await expect(page.locator(".detail-code")).toHaveText("000812");
  const list = page.getByRole("navigation", { name: "个股列表", exact: true });
  await expect(list.locator(".terminal-stock-row").first()).toContainText("000812");
  await list.getByRole("button", { name: /ST低价股/ }).press("ArrowDown");
  await expect(page.locator(".detail-code")).toHaveText("600610");
  await page.screenshot({ path: testInfo.outputPath("security-sort-desktop.png") });
});

test("桌面涨跌幅与手机涨幅共用排序设置，双向切屏显示相同方向", async ({ page }, testInfo) => {
  await page.goto("/?tradingE2E=1");
  const grid = page.getByRole("region", { name: "股票行情", exact: true });
  const header = grid.getByRole("columnheader", { name: "涨跌幅", exact: true });
  await header.click();
  await expect(header).toHaveAttribute("aria-sort", "ascending");
  await page.setViewportSize({ width: 375, height: 812 });
  const sort = page.getByRole("button", { name: /^涨幅/ });
  await expect(sort).toHaveAttribute("aria-pressed", "true");
  await expect(sort).toContainText("↑");
  await sort.click();
  await expect(sort).toHaveAttribute("aria-pressed", "false");
  await sort.click();
  await expect(sort).toContainText("↓");
  await page.screenshot({ path: testInfo.outputPath("security-sort-mobile.png") });
  await page.setViewportSize({ width: 1280, height: 900 });
  await expect(header).toHaveAttribute("aria-sort", "descending");
});
