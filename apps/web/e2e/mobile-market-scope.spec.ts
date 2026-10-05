import { expect, test } from "@playwright/test";

test.setTimeout(10_000);

test("手机行情只展示真实游戏证券和可用操作，不保留参考软件的辅助工具", async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 320, height: 844 });
  await page.goto("/?tradingE2E=1");
  const list = page.getByLabel("股票行情列表", { exact: true });
  await expect(list.getByRole("button")).toHaveCount(5);
  await expect(page.getByRole("button", { name: /资金|资讯|资产|分析/ })).toHaveCount(0);
  await expect(page.getByRole("region", { name: "市场指数与快捷入口" })).toHaveCount(0);
  await expect(page.getByText("多股同列", { exact: false })).toHaveCount(0);
  const toolbar = page.getByLabel("行情列表工具栏", { exact: true });
  await expect(toolbar).toContainText("名称 / 代码");
  const sort = toolbar.getByRole("button", { name: /^涨幅/ });
  await sort.click();
  await expect(sort).toContainText("↓");
  await page.getByRole("searchbox", { name: "搜索股票" }).fill("600101");
  await expect(list.getByRole("button")).toHaveCount(1);
  await list.getByRole("button", { name: /稳健实业 600101/ }).click();
  await expect(page.getByRole("tablist", { name: "图表周期" })).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath("mobile-market-detail-320.png") });
});
