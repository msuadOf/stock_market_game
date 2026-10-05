import { expect, test } from "@playwright/test";

test.setTimeout(10_000);

for (const width of [320, 390]) {
  test(`${width}px 暂停偏好每项独立触控行，点击行尾只切换对应选项`, async ({ page }) => {
    await page.setViewportSize({ width, height: 844 });
    await page.goto("/?tradingE2E=1");
    await page.getByRole("button", { name: "打开我的与存档", exact: true }).click();
    const settings = page.getByRole("region", { name: "游戏状态", exact: true });
    const afterClose = settings.getByRole("checkbox", { name: "收盘后暂停复盘", exact: true });
    const beforeOpen = settings.getByRole("checkbox", { name: "开盘前暂停查看资讯", exact: true });
    const afterRow = settings.locator("label").filter({ has: page.getByRole("checkbox", { name: "收盘后暂停复盘", exact: true }) });
    const beforeRow = settings.locator("label").filter({ has: page.getByRole("checkbox", { name: "开盘前暂停查看资讯", exact: true }) });
    const afterBox = await afterRow.boundingBox();
    const beforeBox = await beforeRow.boundingBox();
    if (afterBox === null || beforeBox === null) throw new Error("暂停选项触控行不可见");
    expect(afterBox.height).toBeGreaterThanOrEqual(44);
    expect(beforeBox.height).toBeGreaterThanOrEqual(44);
    expect(beforeBox.y).toBeGreaterThanOrEqual(afterBox.y + afterBox.height);
    expect(afterBox.width).toBeGreaterThanOrEqual(width - 30);
    expect(beforeBox.width).toBeGreaterThanOrEqual(width - 30);
    await afterRow.click({ position: { x: afterBox.width - 12, y: afterBox.height / 2 } });
    await expect(afterClose).toBeChecked();
    await expect(beforeOpen).not.toBeChecked();
    await expect(beforeOpen).toBeEnabled();
    await beforeRow.click({ position: { x: beforeBox.width - 12, y: beforeBox.height / 2 } });
    await expect(beforeOpen).toBeChecked();
    await expect(afterClose).toBeChecked();
    await expect.poll(() => page.evaluate('sessionStorage.getItem("stock-game-pause-preferences")')).toBe(
      '{"pause_after_close":true,"pause_before_open":true}',
    );
    const dimensions = await page.evaluate("({width:document.documentElement.clientWidth,scroll:document.documentElement.scrollWidth})") as { width: number; scroll: number };
    expect(dimensions.scroll).toBe(width);
    expect(dimensions.width).toBe(width);
  });
}
