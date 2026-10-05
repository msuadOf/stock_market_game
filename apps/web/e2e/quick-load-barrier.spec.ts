import { expect, test, type Page } from "@playwright/test";
import { gunzipSync } from "node:zlib";
import { readQuickArchive } from "./quick-archive.ts";

test.setTimeout(10_000);

async function preparePendingWrite(page: Page) {
  // 仅在验收页面延迟原生 gzip 前的流；不替换档案、引擎或 IndexedDB 输出。
  await page.addInitScript({ content: `(() => {
    const NativeCompression = CompressionStream;
    let armed = false, entered = false, release = null;
    const control = {
      arm() { armed = true; entered = false; },
      entered() { return entered; },
      release(fail) {
        if (release === null) throw new Error("没有正在等待的压缩写入");
        release(fail);
      }
    };
    globalThis.__STOCK_GAME_SAVE_WRITE_TEST__ = control;
    globalThis.CompressionStream = class {
      constructor(format) {
        const native = new NativeCompression(format);
        const gate = new TransformStream({ async transform(chunk, controller) {
          if (armed) {
            armed = false; entered = true;
            const fail = await new Promise(resolve => { release = resolve; });
            release = null;
            if (fail) throw new Error("验收压缩写入失败");
          }
          controller.enqueue(chunk);
        }});
        this.writable = gate.writable;
        this.readable = gate.readable.pipeThrough(native);
      }
    };
  })();` });
  await page.setViewportSize({ width: 902, height: 833 });
  await page.goto("/?tradingE2E=1");
  await expect(page.locator(".app-root")).toHaveAttribute("data-game-tick", "0");
  await page.evaluate(`globalThis.__STOCK_GAME_E2E__.advanceToTick(1)`);
  await expect.poll(async () => {
    const raw = await readQuickArchive(page);
    if (raw === null) return null;
    const slot = JSON.parse(gunzipSync(Buffer.from(raw.slice(5), "base64")).toString("utf8")) as { snapshot: { tick: number } };
    return slot.snapshot.tick;
  }).toBe(0);
  const previous = await readQuickArchive(page);
  await page.evaluate(`globalThis.__STOCK_GAME_SAVE_WRITE_TEST__.arm()`);
  await page.evaluate(`globalThis.__STOCK_GAME_E2E__.advanceToTick(31)`);
  await expect.poll(() => page.evaluate(`globalThis.__STOCK_GAME_SAVE_WRITE_TEST__.entered()`)).toBe(true);
  expect(await readQuickArchive(page)).toBe(previous);
  await page.getByRole("button", { name: "游戏与存档", exact: true }).click();
  await page.getByRole("button", { name: "读取本地进度", exact: true }).click();
  return previous;
}

test("日终压缩尚未完成时点击读档，等待提交后载入当日快速槽", async ({ page }, testInfo) => {
  await preparePendingWrite(page);
  await expect(page.locator(".notice")).toContainText("正在读取日终快速存档");
  await expect(page.locator(".app-root")).toHaveAttribute("data-game-tick", "31");
  await page.screenshot({ path: testInfo.outputPath("quick-load-waiting.png") });
  await page.evaluate(`globalThis.__STOCK_GAME_SAVE_WRITE_TEST__.release(false)`);
  await expect(page.locator(".notice")).toContainText("已读档");
  await expect(page.locator(".app-root")).toHaveAttribute("data-game-tick", "30");
  await page.getByRole("navigation", { name: "桌面主导航" }).getByRole("button", { name: "个股", exact: true }).click();
  await page.getByRole("button", { name: "公司资料 F10", exact: true }).click();
  await expect(page.getByLabel("当前模拟日历")).toContainText("2030-01-03");
});

test("读档等待的真实压缩写入失败时保留当前局与上一有效快速槽", async ({ page }) => {
  const previous = await preparePendingWrite(page);
  await expect(page.locator(".notice")).toContainText("正在读取日终快速存档");
  await page.evaluate(`globalThis.__STOCK_GAME_SAVE_WRITE_TEST__.release(true)`);
  await expect(page.locator(".notice")).toContainText("读档失败");
  await expect(page.locator(".notice")).toContainText("验收压缩写入失败");
  await expect(page.locator(".app-root")).toHaveAttribute("data-game-tick", "31");
  expect(await readQuickArchive(page)).toBe(previous);
});
