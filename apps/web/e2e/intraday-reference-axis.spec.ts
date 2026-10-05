import { expect, test } from "@playwright/test";

test("分时昨收0%参考轴在桌面与手机切换后始终位于价格绘图区正中", async ({ page }) => {
  test.setTimeout(10_000);
  await page.setViewportSize({ width: 902, height: 833 });
  await page.goto("/?tradingE2E=1");
  await expect(page.locator(".app-root")).toBeVisible();
  await page.getByRole("navigation", { name: "桌面主导航" }).getByRole("button", { name: "个股", exact: true }).click();
  const desktop = page.locator(".desktop-intraday");
  await expect(desktop.locator(".intraday-baseline")).toHaveAttribute("y1", "50");
  await expect(desktop.locator(".intraday-baseline")).toHaveAttribute("y2", "50");
  await expect(desktop.locator('.intraday-percent-label[style="top: 50%;"]')).toHaveText("0.00%");
  await page.setViewportSize({ width: 320, height: 844 });
  if (await page.locator(".mobile-market-row").first().isVisible()) await page.locator(".mobile-market-row").first().click();
  const plot = page.locator(".msd-price-plot");
  const middle = await plot.evaluate((element) => {
    const box = element.getBoundingClientRect();
    const line = element.ownerDocument.defaultView!.getComputedStyle(element, "::after");
    const label = element.querySelector(".msd-scale-mid")!.getBoundingClientRect();
    return { lineY: Number.parseFloat(line.top), halfHeight: box.height / 2, labelY: label.top + label.height / 2 - box.top };
  });
  expect(Math.abs(middle.lineY - middle.halfHeight)).toBeLessThan(1);
  expect(Math.abs(middle.labelY - middle.halfHeight)).toBeLessThan(1);
  await page.setViewportSize({ width: 902, height: 833 });
  await expect(desktop.locator(".intraday-baseline")).toHaveAttribute("y1", "50");
});
