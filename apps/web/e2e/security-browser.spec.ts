import { expect, test } from "@playwright/test";

test.setTimeout(10_000);

test("查询恢复原行序，Enter 与首行一致，主动列排序继续保留", async ({ page }) => {
  await page.goto("/?tradingE2E=1");
  const query = page.getByRole("searchbox", { name: "搜索股票" });
  const grid = page.getByRole("region", { name: "股票行情", exact: true });
  const names = () => grid.locator('.ag-row [col-id="name"]').evaluateAll(cells => cells.sort((a, b) => a.getBoundingClientRect().top - b.getBoundingClientRect().top).map(cell => cell.textContent));
  await query.fill("002156");
  await expect.poll(names).toEqual(["芯片科技"]);
  await query.press("Escape");
  await expect.poll(names).toEqual(["稳健实业", "芯片科技", "短线题材", "人气妖股", "ST低价股"]);
  await query.press("Enter");
  await expect(page.locator(".detail-code")).toHaveText("600101");
  await page.getByRole("button", { name: "返回行情", exact: true }).click();
  await grid.getByRole("columnheader", { name: "现价", exact: true }).click();
  await expect.poll(names).toEqual(["ST低价股", "人气妖股", "稳健实业", "芯片科技", "短线题材"]);
  await query.fill("002156");
  await query.press("Escape");
  await expect.poll(names).toEqual(["ST低价股", "人气妖股", "稳健实业", "芯片科技", "短线题材"]);
  await query.press("Enter");
  await expect(page.locator(".detail-code")).toHaveText("000812");
});

test("切到手机后搜索Enter选择手机首行，不受隐藏桌面列排序影响", async ({ page }) => {
  await page.goto("/?tradingE2E=1");
  const grid = page.getByRole("region", { name: "股票行情", exact: true });
  await grid.getByRole("columnheader", { name: "现价", exact: true }).click();
  await page.setViewportSize({ width: 320, height: 844 });
  await expect(page.locator(".mobile-market-row").first()).toContainText("600101");
  await page.getByRole("searchbox", { name: "搜索股票" }).press("Enter");
  await expect(page.locator(".msd-security-title")).toContainText("600101");
});

test("桌面搜索 Enter 进入个股，自选保存并筛选，移出不跳股", async ({ page }) => {
  await page.setViewportSize({ width: 902, height: 833 });
  await page.goto("/?tradingE2E=1");
  await page.getByRole("searchbox", { name: "搜索股票" }).fill("芯片");
  await expect(page.getByRole("region", { name: "股票行情", exact: true })).toContainText("002156");
  await expect(page.getByRole("region", { name: "股票行情", exact: true })).not.toContainText("600101");
  await page.getByRole("searchbox", { name: "搜索股票" }).press("Enter");
  await expect(page.locator(".detail-code")).toHaveText("002156");
  await page.getByRole("button", { name: "加入自选", exact: true }).click();
  await expect(page.getByRole("button", { name: "移出自选", exact: true })).toHaveAttribute("aria-pressed", "true");
  const list = page.getByRole("navigation", { name: "个股列表", exact: true });
  await list.getByRole("button", { name: "自选", exact: true }).click();
  await expect(list.locator(".terminal-stock-row")).toHaveCount(1);
  await list.getByRole("button", { name: "清空搜索", exact: true }).click();
  await page.getByRole("button", { name: "移出自选", exact: true }).click();
  await expect(list).toContainText("暂无自选股票");
  await expect(page.locator(".detail-code")).toHaveText("002156");
  await page.getByRole("button", { name: "加入自选", exact: true }).click();
  await page.reload();
  await page.getByRole("navigation", { name: "桌面主导航" }).getByRole("button", { name: "个股", exact: true }).click();
  await list.getByRole("button", { name: "自选", exact: true }).click();
  await expect(list.locator(".terminal-stock-row")).toHaveCount(1);
  await expect(list.locator(".terminal-stock-row")).toContainText("002156");
});

test("手机与桌面共用查询，自选返回保留，320px 无溢出", async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 844 });
  await page.goto("/?tradingE2E=1");
  await page.getByRole("searchbox", { name: "搜索股票" }).fill("002156");
  await page.getByRole("button", { name: /芯片科技 002156/ }).click();
  await page.getByRole("button", { name: "加入自选", exact: true }).click();
  await page.getByRole("button", { name: "返回股票列表", exact: true }).click();
  await page.getByRole("navigation", { name: "主导航" }).getByRole("button", { name: "自选", exact: true }).click();
  await expect(page.getByRole("searchbox", { name: "搜索股票" })).toHaveValue("002156");
  await expect(page.locator(".mobile-market-row")).toHaveCount(1);
  await page.getByRole("searchbox", { name: "搜索股票" }).fill("找不到");
  await expect(page.locator(".mobile-market-list")).toContainText("没有匹配股票");
  await page.getByRole("searchbox", { name: "搜索股票" }).press("Escape");
  await expect(page.locator(".mobile-market-row")).toHaveCount(1);
  expect(await page.locator("html").evaluate(el => el.scrollWidth > el.clientWidth)).toBe(false);
  await page.setViewportSize({ width: 902, height: 833 });
  await expect(page.getByRole("navigation", { name: "桌面主导航", exact: true })).toBeVisible();
  await expect(page.getByRole("navigation", { name: "股票范围", exact: true }).getByRole("button", { name: "自选", exact: true })).toHaveAttribute("aria-pressed", "true");
});

test("自选读写错误明确展示，失败不改变已保存名单", async ({ page }) => {
  await page.addInitScript(() => {
    const original = Storage.prototype.setItem;
    Storage.prototype.setItem = function(key, value) {
      if (key === "stock-game-watchlist") throw new Error("测试空间不足");
      original.call(this, key, value);
    };
  });
  await page.goto("/?tradingE2E=1");
  await page.getByRole("navigation", { name: "桌面主导航" }).getByRole("button", { name: "个股", exact: true }).click();
  await page.getByRole("button", { name: "加入自选", exact: true }).click();
  await expect(page.getByRole("button", { name: "加入自选", exact: true })).toHaveAttribute("aria-pressed", "false");
  await expect(page.getByRole("navigation", { name: "个股列表" })).toContainText("保存自选失败：测试空间不足");
});

test("损坏自选保留原内容，禁用自选写入但全部行情和搜索仍可用", async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem("stock-game-watchlist", "{"));
  await page.goto("/?tradingE2E=1");
  await page.getByRole("navigation", { name: "桌面主导航" }).getByRole("button", { name: "个股", exact: true }).click();
  const list = page.getByRole("navigation", { name: "个股列表" });
  await expect(list).toContainText("读取自选失败");
  await expect(list.locator(".terminal-stock-row")).toHaveCount(5);
  await expect(page.getByRole("button", { name: "加入自选", exact: true })).toBeDisabled();
  await list.getByRole("button", { name: "自选", exact: true }).click();
  await expect(list).toContainText("自选尚未读取");
  await expect(list).not.toContainText("暂无自选股票");
  await list.getByRole("button", { name: "重试读取自选" }).click();
  expect(await page.evaluate(() => localStorage.getItem("stock-game-watchlist"))).toBe("{");
});

test("读取权限恢复后重试明确成功，恢复自选开关", async ({ page }) => {
  await page.addInitScript(() => {
    const original = Storage.prototype.getItem;
    let fail = true;
    Storage.prototype.getItem = function(key) {
      if (key === "stock-game-watchlist" && fail) { fail = false; throw new Error("暂时不可读取"); }
      return original.call(this, key);
    };
  });
  await page.goto("/?tradingE2E=1");
  await page.getByRole("navigation", { name: "桌面主导航" }).getByRole("button", { name: "个股", exact: true }).click();
  const list = page.getByRole("navigation", { name: "个股列表" });
  await expect(page.getByRole("button", { name: "加入自选", exact: true })).toBeDisabled();
  await list.getByRole("button", { name: "重试读取自选" }).click();
  await expect(page.getByText("自选已重新读取。", { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "加入自选", exact: true })).toBeEnabled();
  await expect(list.getByRole("alert")).toHaveCount(0);
  await page.getByRole("button", { name: "加入自选", exact: true }).click();
  await expect(page.getByRole("button", { name: "移出自选", exact: true })).toHaveAttribute("aria-pressed", "true");
});

test("桌面左列表支持方向键与首尾定位，窄窗口按钮保持可见", async ({ page }) => {
  await page.setViewportSize({ width: 902, height: 833 });
  await page.goto("/?tradingE2E=1");
  await page.getByRole("navigation", { name: "桌面主导航" }).getByRole("button", { name: "个股", exact: true }).click();
  const list = page.getByRole("navigation", { name: "个股列表" });
  const rows = list.locator(".terminal-stock-row");
  await rows.first().press("ArrowDown");
  await expect(rows.nth(1)).toBeFocused();
  await expect(page.locator(".detail-code")).toHaveText("002156");
  await rows.nth(1).press("End");
  await expect(rows.last()).toBeFocused();
  await expect(page.locator(".detail-code")).toHaveText("000812");
  expect(await list.evaluate(el => el.scrollWidth > el.clientWidth)).toBe(false);
  const actions = page.locator(".terminal-quote-actions");
  const box = await actions.boundingBox();
  for (const button of await actions.getByRole("button").all()) {
    const bounds = await button.boundingBox();
    expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(box!.x + box!.width + 1);
  }
});


test("行情表真实表头使用中文排序提示，不产生 LocaleModule 缺失错误", async ({ page }) => {
  const localeErrors: string[] = [];
  page.on("console", message => {
    if (message.type() === "error" && message.text().includes("LocaleModule")) localeErrors.push(message.text());
  });
  await page.goto("/?tradingE2E=1");
  const grid = page.getByRole("region", { name: "股票行情", exact: true });
  const codeHeader = grid.getByRole("columnheader", { name: /^代码/ });
  await codeHeader.focus();
  await expect(grid.locator(".ag-aria-description-container")).toContainText("按 Enter 排序");
  await codeHeader.press("Enter");
  await expect(codeHeader).toHaveAttribute("aria-sort", "ascending");
  await page.getByRole("searchbox", { name: "搜索股票" }).fill("找不到");
  await expect(grid).toContainText("没有匹配股票");
  expect(localeErrors).toEqual([]);
});
