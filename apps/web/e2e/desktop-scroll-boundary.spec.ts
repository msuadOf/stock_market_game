import { expect, test } from "@playwright/test";

test.setTimeout(10_000);

for (const width of [902, 1020]) {
  test(`${width}px看盘交易保持文档高度，条件单只滚动内部面板且名称可访问`, async ({ page }) => {
    await page.setViewportSize({ width, height: 833 });
    await page.goto("/?tradingE2E=1");
    await page.getByRole("navigation", { name: "桌面主导航" }).getByRole("button", { name: "个股", exact: true }).click();
    await page.getByRole("tab", { name: "日K", exact: true }).click();
    await page.getByRole("button", { name: "买入此股票", exact: true }).click();
    await expect(page.getByRole("region", { name: "看盘交易栏" })).toBeVisible();
    const documentBounds = () => page.locator("html").evaluate(element => ({
      height: element.clientHeight,
      contentHeight: element.scrollHeight,
      scrollY: element.ownerDocument.defaultView!.scrollY,
    }));
    expect(await documentBounds()).toEqual({ height: 833, contentHeight: 833, scrollY: 0 });
    const order = page.locator("#section-order");
    const trigger = order.getByRole("textbox", { name: "条件单触发价（元）", exact: true });
    const quantity = order.getByRole("textbox", { name: "条件单数量（股）", exact: true });
    await trigger.fill("10.50");
    await quantity.fill("300");
    await expect(trigger).toHaveValue("10.50");
    await expect(quantity).toHaveValue("300");
    const internal = await order.evaluate(element => ({
      height: element.clientHeight, contentHeight: element.scrollHeight, scrollTop: element.scrollTop,
    }));
    expect(internal.contentHeight).toBeGreaterThan(internal.height);
    expect(internal.scrollTop).toBeGreaterThan(0);
    expect(await documentBounds()).toEqual({ height: 833, contentHeight: 833, scrollY: 0 });
    await page.getByRole("navigation", { name: "桌面主导航" }).getByRole("button", { name: "交易", exact: true }).click();
    await expect(trigger).toHaveValue("10.50");
    await expect(quantity).toHaveValue("300");
    expect(await documentBounds()).toEqual({ height: 833, contentHeight: 833, scrollY: 0 });
    await page.setViewportSize({ width: 320, height: 844 });
    await page.getByRole("navigation", { name: "主导航", exact: true }).getByRole("button", { name: "交易", exact: true }).click();
    const dialog = page.getByRole("dialog", { name: "交易面板", exact: true });
    await expect(dialog).toBeVisible();
    const mobileTrigger = dialog.getByRole("textbox", { name: "条件单触发价（元）", exact: true });
    const mobileQuantity = dialog.getByRole("textbox", { name: "条件单数量（股）", exact: true });
    await expect(mobileTrigger).toHaveValue("10.50");
    await expect(mobileQuantity).toHaveValue("300");
    await mobileTrigger.fill("10.75");
    await mobileQuantity.fill("400");
    await expect(mobileQuantity).toBeFocused();
    await expect(mobileTrigger).toHaveValue("10.75");
    await expect(mobileQuantity).toHaveValue("400");
    await dialog.getByRole("button", { name: "收起交易面板", exact: true }).click();
    await expect(dialog).toBeHidden();
    await expect(page.getByRole("searchbox", { name: "搜索股票", exact: true })).toBeVisible();
  });
}
