import { expect, test } from "@playwright/test";
import { SAVE_DATABASE } from "../src/save/save-storage-keys.ts";

test.setTimeout(10_000);

for (const viewport of [{ width: 902, height: 833 }, { width: 320, height: 844 }]) {
  test(`${viewport.width}px 游戏管理说明日终存档，点击说明不写档`, async ({ page }) => {
    await page.setViewportSize(viewport);
    await page.goto("/?tradingE2E=1");
    await expect(page.locator(".app-root")).toHaveAttribute("data-game-tick", "0");
    await page.getByRole("button", { name: viewport.width < viewport.height ? "打开我的与存档" : "游戏与存档", exact: true }).click();
    const section = page.locator(".mobile-user-section");
    const policy = section.getByRole("button", { name: "日终存档说明", exact: true });
    const file = section.getByRole("button", { name: "设置日终存档文件", exact: true });
    await expect(policy).toBeVisible();
    await expect(file).toBeVisible();
    await expect(policy).toHaveAccessibleDescription(/日内不保存/);
    await expect(file).toHaveAccessibleDescription(/后续日终更新/);
    await expect(section).toContainText("首次日终前没有可读档案");
    await expect(section.getByRole("button", { name: "保存当前进度", exact: true })).toHaveCount(0);
    await expect(section.getByRole("button", { name: "另存为文件", exact: true })).toHaveCount(0);
    // 首次日终前应连持久数据库都未创建；不要以 open() 检查而先造出空库。
    const databases = () => page.evaluate<string[]>("indexedDB.databases().then(databases => databases.map(database => database.name))");
    expect(await databases()).not.toContain(SAVE_DATABASE);
    await policy.click();
    await expect(page.locator(".notice")).toContainText("日内不写档");
    expect(await databases()).not.toContain(SAVE_DATABASE);
    const dimensions = await page.evaluate("({width:document.documentElement.clientWidth,scroll:document.documentElement.scrollWidth})") as { width: number; scroll: number };
    expect(dimensions.scroll).toBe(dimensions.width);
  });
}
