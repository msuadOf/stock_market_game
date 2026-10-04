import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";
import { chromium } from "@playwright/test";

function luminance(color: string): number {
  const channels = parseColor(color).channels;
  const linear = channels.map((channel) => {
    const value = channel / 255;
    return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  });
  return linear[0] * 0.2126 + linear[1] * 0.7152 + linear[2] * 0.0722;
}

function parseColor(color: string): { channels: number[]; alpha: number } {
  const content = color.match(/rgba?\((.+)\)/)?.[1];
  assert.ok(content, `预期浏览器返回 RGB 颜色，实际为 ${color}`);
  const [channelText, slashAlpha] = content.split("/");
  const components = channelText.trim().split(/[\s,]+/).filter(Boolean);
  const legacyAlpha = components.length === 4 ? components.pop() : undefined;
  const channels = components.map((component) => component.endsWith("%") ? Number.parseFloat(component) * 2.55 : Number.parseFloat(component));
  const alphaValue = slashAlpha?.trim() ?? legacyAlpha ?? "1";
  const alpha = alphaValue.endsWith("%") ? Number.parseFloat(alphaValue) / 100 : Number.parseFloat(alphaValue);
  assert.equal(channels.length, 3, `预期颜色包含三个 RGB 通道，实际为 ${color}`);
  assert.ok(Number.isFinite(alpha) && alpha >= 0 && alpha <= 1, `预期颜色 alpha 在 0 至 1 之间，实际为 ${color}`);
  return { channels, alpha };
}

function contrastRatio(foreground: string, background: string): number {
  const foregroundColor = parseColor(foreground);
  const backgroundColor = parseColor(background);
  const compositedForeground = `rgb(${foregroundColor.channels.map((channel, index) => channel * foregroundColor.alpha + backgroundColor.channels[index] * (1 - foregroundColor.alpha)).join(", ")})`;
  const values = [luminance(compositedForeground), luminance(background)].sort((left, right) => right - left);
  return (values[0] + 0.05) / (values[1] + 0.05);
}

test("实际移动行情、持仓与导航表面上的小字号文字满足 WCAG AA", { timeout: 10000 }, async () => {
  const [globalCss, appCss, mobileCss] = await Promise.all([
    readFile(new URL("../index.css", import.meta.url), "utf8"),
    readFile(new URL("../App.css", import.meta.url), "utf8"),
    readFile(new URL("./MobileStockDetail.css", import.meta.url), "utf8"),
  ]);
  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage({ isMobile: true, viewport: { width: 390, height: 844 } });
    await page.setContent(`<!doctype html><html><head><meta name="viewport" content="width=device-width, initial-scale=1"></head><body><div id="root"><div class="app-root layout-mobile">
      <section class="mobile-market-dashboard"><div class="mobile-index-card up"><span>指数</span><strong>1010.00</strong><small>+1.00%</small></div><nav class="mobile-market-tabs"><button class="active">自选</button></nav></section>
      <div class="mobile-market-toolbar"><span>✎　　☷</span><b>▦ 多股同列</b><button type="button">涨幅　↓</button></div>
      <div class="mobile-market-list"><span class="mobile-market-price up"><strong>+1.00%</strong></span><span class="mobile-market-price down"><strong>-1.00%</strong></span></div>
      <main class="mobile-stock-detail"><header class="msd-header"><div class="msd-security-title"><strong>测试股份</strong><small>600101</small></div></header>
        <section class="msd-quote"><div class="msd-last rise"><strong>10.10</strong><span>+0.10　+1.00%</span></div><div class="msd-day-prices"><span>低 <b class="fall">9.90</b></span></div><div class="msd-stock-stats"><span>买一 <b class="rise">10.00</b></span><span>卖一 <b class="fall">10.20</b></span></div></section>
      </main>
      <section class="mobile-portfolio-summary"><div><span>持仓市值</span><b>1010元</b></div><div><span>浮动盈亏</span><b class="up">+10元</b></div></section>
      <nav class="mobile-tabbar mobile-main-tabbar" aria-label="主导航"><button class="tab-btn active"><span class="tab-icon tab-icon-market" aria-hidden="true"></span><span>行情</span></button><button class="tab-btn"><span>我的</span></button></nav>
    </div></div></body></html>`);
    await page.addStyleTag({ content: globalCss });
    await page.addStyleTag({ content: appCss });
    await page.addStyleTag({ content: mobileCss });

    const measure = (selector: string) => page.locator(selector).evaluate((element) => {
      const parse = (color: string) => {
        const content = color.match(/rgba?\((.+)\)/)?.[1];
        if (content === undefined) throw new Error(`无法解析浏览器颜色：${color}`);
        const [channelText, slashAlpha] = content.split("/");
        const components = channelText.trim().split(/[\s,]+/).filter(Boolean);
        const legacyAlpha = components.length === 4 ? components.pop() : undefined;
        const channels = components.map((component) => component.endsWith("%") ? Number.parseFloat(component) * 2.55 : Number.parseFloat(component));
        const alphaValue = slashAlpha?.trim() ?? legacyAlpha ?? "1";
        const alpha = alphaValue.endsWith("%") ? Number.parseFloat(alphaValue) / 100 : Number.parseFloat(alphaValue);
        return { channels, alpha };
      };
      const layers: Array<{ channels: number[]; alpha: number }> = [];
      let ancestor: Element | null = element;
      while (ancestor) {
        const background = parse(getComputedStyle(ancestor).backgroundColor);
        layers.push(background);
        if (background.alpha >= 1) break;
        ancestor = ancestor.parentElement;
      }
      let color = [255, 255, 255];
      for (const layer of layers.reverse()) {
        color = layer.channels.map((channel, index) => channel * layer.alpha + color[index] * (1 - layer.alpha));
      }
      return {
        foreground: getComputedStyle(element).color,
        background: `rgb(${color.map(Math.round).join(", ")})`,
        fontSize: getComputedStyle(element).fontSize,
      };
    });

    const selectors = [
      ".app-root",
      ".mobile-index-card.up small",
      ".mobile-market-price.up strong",
      ".mobile-market-price.down strong",
      ".mobile-market-toolbar",
      ".mobile-market-toolbar button",
      ".mobile-market-tabs button.active",
      ".msd-security-title strong",
      ".msd-header small",
      ".msd-last",
      ".msd-last strong",
      ".msd-day-prices b.fall",
      ".msd-stock-stats b.rise",
      ".msd-stock-stats b.fall",
      ".mobile-portfolio-summary b.up",
      ".mobile-tabbar .tab-btn.active",
      ".mobile-tabbar .tab-btn:not(.active)",
    ];
    for (const selector of selectors) {
      const style = await measure(selector);
      assert.ok(contrastRatio(style.foreground, style.background) >= 4.5, `${selector} 对比度不足：${style.foreground} / ${style.background} (${style.fontSize})`);
      if (selector === ".msd-header small") assert.ok(parseColor(style.foreground).alpha < 1, "标题副行应覆盖半透明前景的 alpha 合成路径");
    }
    const quoteSizes: number[] = [];
    for (const width of [320, 390, 430]) {
      await page.setViewportSize({ width, height: 844 });
      quoteSizes.push(await page.locator(".msd-last strong").evaluate((element) => Number.parseFloat(getComputedStyle(element).fontSize)));
    }
    assert.ok(quoteSizes.every((size) => size >= 24), `320–430px 视口下主报价字号应保持清晰：${quoteSizes.join(", ")}`);

    await page.locator("html").evaluate((element) => element.setAttribute("data-theme", "dark"));
    const viewport = await page.evaluate(() => ({ inner: [innerWidth, innerHeight], screen: [screen.width, screen.height], landscape: matchMedia("(orientation: landscape)").matches, themeToken: getComputedStyle(document.documentElement).getPropertyValue("--up-text"), surfaceToken: getComputedStyle(document.documentElement).getPropertyValue("--up-text-on-dark") }));
    for (const selector of selectors) {
      const style = await measure(selector);
      assert.ok(contrastRatio(style.foreground, style.background) >= 4.5, `暗色主题 ${selector} 对比度不足：${style.foreground} / ${style.background} (${style.fontSize}) ${JSON.stringify(viewport)}`);
    }
  } finally {
    await browser.close();
  }
});
