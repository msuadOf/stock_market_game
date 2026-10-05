import { expect, test, type Page } from "@playwright/test";

async function openControlledGame(page: Page, width: number): Promise<void> {
  await page.setViewportSize({ width, height: 844 });
  await page.goto("/?tradingE2E=1");
  await expect(page.locator(".app-root")).toBeVisible({ timeout: 30_000 });
  await expect(page.locator(".app-error")).toHaveCount(0);
  await expect(page.locator(".app-root")).toHaveAttribute("data-game-tick", "0");
}

test("移动详情显示真实分时午休轴、倍率和共享暂停状态", async ({ page }) => {
  await openControlledGame(page, 390);
  await expect(page.locator(".mobile-brand-bar .mobile-speed-actual")).toHaveText("实测 0.00x");
  await page.locator(".mobile-market-row").first().click();

  const detail = page.locator(".mobile-detail-page");
  await expect(detail).toBeVisible();
  await expect(detail.getByRole("timer")).toContainText("第1日");
  await expect(detail.getByLabel("五档盘口，数量单位为手")).toBeVisible();
  const axis = detail.locator(".msd-time-axis");
  await expect(axis.getByText("11:30/13:00", { exact: true })).toHaveCount(1);
  await expect(axis.locator("span")).toHaveText(["09:15", "09:30", "10:30", "11:30/13:00", "14:00", "15:00"]);
  await expect(detail.locator(".mobile-speed-actual")).toHaveText("实测 0.00x");

  const detailToggle = detail.getByRole("button", { name: "继续模拟" });
  await expect(detailToggle).toHaveAttribute("data-state", "paused");
  await detailToggle.click();
  await expect(detail.getByRole("button", { name: "暂停模拟" })).toHaveAttribute("data-state", "running");
  await detail.getByRole("button", { name: "返回股票列表" }).click();
  const globalToggle = page.locator(".mobile-brand-bar").getByRole("button", { name: "暂停模拟" });
  await expect(globalToggle).toHaveAttribute("data-state", "running");
  await globalToggle.click();
  await page.locator(".mobile-market-row").first().click();
  await expect(detail.getByRole("button", { name: "继续模拟" })).toHaveAttribute("data-state", "paused");
});

test("移动我的页面与桌面控制区均可操作自然日暂停偏好", async ({ page }) => {
  test.setTimeout(10_000);
  await openControlledGame(page, 390);
  await page.getByRole("button", { name: "打开我的与存档" }).click();
  const mobileState = page.locator(".mobile-game-state");
  const afterClose = mobileState.getByRole("checkbox", { name: "收盘后暂停复盘" });
  const beforeOpen = mobileState.getByRole("checkbox", { name: "开盘前暂停查看资讯" });
  await afterClose.click();
  await expect(afterClose).toBeChecked();
  await beforeOpen.click();
  await expect(beforeOpen).toBeChecked();
  await expect.poll(() => page.evaluate(() => sessionStorage.getItem("stock-game-pause-preferences"))).toBe(
    '{"pause_after_close":true,"pause_before_open":true}',
  );

  await page.setViewportSize({ width: 1440, height: 900 });
  await page.getByRole("button", { name: "游戏与存档", exact: true }).click();
  const desktopControls = page.locator(".mobile-game-state");
  await expect(desktopControls.getByRole("checkbox", { name: "收盘后暂停复盘" })).toBeChecked();
  await expect(desktopControls.getByRole("checkbox", { name: "开盘前暂停查看资讯" })).toBeChecked();
  await desktopControls.getByRole("checkbox", { name: "开盘前暂停查看资讯" }).click();
  await expect(desktopControls.getByRole("checkbox", { name: "开盘前暂停查看资讯" })).not.toBeChecked();
  await expect.poll(() => page.evaluate(() => sessionStorage.getItem("stock-game-pause-preferences"))).toBe(
    '{"pause_after_close":true,"pause_before_open":false}',
  );
});
