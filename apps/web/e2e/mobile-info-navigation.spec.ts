import { expect, test } from "@playwright/test";

for (const width of [320, 390]) {
  test(`${width}px 信息菜单只提供真实内容，键盘切换、周期切换和返回保留选择`, async ({ page }) => {
    test.setTimeout(10_000);
    await page.setViewportSize({ width, height: 844 });
    await page.goto("/?tradingE2E=1");
    await expect(page.locator(".app-root")).toBeVisible();
    await page.locator(".mobile-market-row").first().click();
    const detail = page.locator(".mobile-detail-page");
    const tabs = detail.getByRole("tablist", { name: "股票详情信息" });
    await expect(tabs.getByRole("tab")).toHaveText(["财务", "盘口", "资金"]);
    await expect(tabs.getByRole("tab", { name: "资金", exact: true })).toHaveAttribute("aria-selected", "true");
    await expect(detail.getByRole("region", { name: "当日累计成交" })).toBeVisible();
    await tabs.getByRole("tab", { name: "盘口", exact: true }).click();
    await expect(detail.getByRole("tabpanel", { name: "盘口", exact: true }).getByLabel("五档盘口，数量单位为手")).toBeVisible();
    await tabs.getByRole("tab", { name: "盘口", exact: true }).press("Home");
    await expect(tabs.getByRole("tab", { name: "财务", exact: true })).toBeFocused();
    await expect(tabs.getByRole("tab", { name: "盘口", exact: true })).toHaveAttribute("aria-selected", "true");
    await tabs.getByRole("tab", { name: "财务", exact: true }).press("End");
    await expect(tabs.getByRole("tab", { name: "资金", exact: true })).toBeFocused();
    await tabs.getByRole("tab", { name: "资金", exact: true }).press("ArrowRight");
    await expect(tabs.getByRole("tab", { name: "财务", exact: true })).toBeFocused();
    await tabs.getByRole("tab", { name: "财务", exact: true }).press("ArrowLeft");
    await expect(tabs.getByRole("tab", { name: "资金", exact: true })).toBeFocused();
    await tabs.getByRole("tab", { name: "资金", exact: true }).press("Home");
    await tabs.getByRole("tab", { name: "财务", exact: true }).press("Enter");
    await expect(detail.getByRole("list", { name: "公开报告列表" })).toBeVisible();
    await detail.getByRole("tablist", { name: "图表周期" }).getByRole("tab", { name: "日K", exact: true }).click();
    await expect(tabs.getByRole("tab", { name: "财务", exact: true })).toHaveAttribute("aria-selected", "true");
    await detail.getByRole("button", { name: "返回股票列表" }).click();
    await page.locator(".mobile-market-row").first().click();
    await expect(page.getByRole("tablist", { name: "股票详情信息" }).getByRole("tab", { name: "财务", exact: true })).toHaveAttribute("aria-selected", "true");
    expect(await page.locator("html").evaluate((element) => element.scrollWidth)).toBe(width);
  });
}
