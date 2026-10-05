import { expect, test } from "@playwright/test";

for (const width of [902, 320]) {
  test(`${width}px DEV 页面无诊断能力时不展示 NPC 诊断入口`, async ({ page }) => {
    test.setTimeout(10_000);
    await page.setViewportSize({ width, height: 844 });
    await page.goto("/?tradingE2E=1");
    await expect(page.locator(".app-root")).toBeVisible();
    const appModule = await page.request.get("/src/App.tsx");
    expect(appModule.ok()).toBe(true);
    expect(await appModule.text()).toMatch(/"DEV"\s*:\s*true/);
    await expect(page.getByRole("button", { name: "当前局 NPC 诊断", exact: true })).toHaveCount(0);
    if (width === 902) {
      await page.getByRole("navigation", { name: "桌面主导航" }).getByRole("button", { name: "个股", exact: true }).click();
      await expect(page.getByRole("button", { name: "返回行情", exact: true })).toBeVisible();
      await page.getByRole("button", { name: "游戏与存档", exact: true }).click();
    } else {
      await page.locator(".mobile-market-row").first().click();
      await expect(page.getByRole("tablist", { name: "股票详情信息" })).toBeVisible();
      await page.getByRole("navigation", { name: "主导航" }).getByRole("button", { name: "我的", exact: true }).click();
    }
    await expect(page.getByRole("button", { name: "日终存档说明", exact: true })).toBeVisible();
    await expect(page.getByRole("button", { name: "当前局 NPC 诊断", exact: true })).toHaveCount(0);
    await expect(page.locator(".app-error")).toHaveCount(0);
  });
}
