// 从实际 Linux Tauri WebView 驱动产品 UI。独立 setup/seed 尚未固定，因此这里只报告 smoke，不是三端配对。
import assert from "node:assert/strict";
import { connectWebKitInspector, waitFor } from "./webkit-inspector.mjs";
const [rawPort, mode] = process.argv.slice(2);
if (mode !== undefined && mode !== "--inspector-only") throw new Error("用法：desktop-ui-smoke.mjs <Inspector 端口> [--inspector-only]");
const app = await connectWebKitInspector(Number(rawPort));
try {
  if (mode === "--inspector-only") {
    const page = await waitFor("WebKit preflight 载入", async () => {
      const result = await app.evaluate("({ title: document.title, href: location.href })");
      return result.href !== "about:blank" && result.title.length > 0 ? result : undefined;
    }, 5_000);
    console.log(JSON.stringify(page));
  } else {
    const state = () => app.evaluate(`(() => ({
      tauri: "__TAURI_INTERNALS__" in window,
      href: location.href,
      title: document.title,
      ms: performance.now(),
      tick: Number(document.querySelector("[data-game-tick]")?.dataset.gameTick ?? -1),
      day: Number(document.querySelector("[data-game-day]")?.dataset.gameDay ?? -1),
      status: document.querySelector(".session-status")?.textContent ?? "",
      metrics: document.querySelector(".speed-actual")?.getAttribute("title") ?? "",
      speed: document.querySelector(".speed-select select")?.value ?? "",
      errors: [...document.querySelectorAll("[role=alert]")].map(node => node.textContent?.trim()).filter(Boolean),
    }))()`);
    const before = await waitFor("Desktop 前端初始化", async () => {
      const ready = await app.evaluate(`Boolean(document.querySelector('#startup-title') && document.querySelector("button[type='submit']"))`);
      return ready ? await state() : undefined;
    }, 20_000);
    console.log(JSON.stringify({ phase: "startup-choice", ...before }));
    assert.equal(before.tauri, true, "预期真实 Tauri WebView，不接受普通浏览器");
    assert.ok(!/^https?:\/\/(localhost|127\.0\.0\.1):5173\b/.test(before.href), "必须测嵌入的前端，不接受开发服务器");
    await app.evaluate(`(() => { document.querySelector("button[type=submit]").click(); return true; })()`);
    const ready = await waitFor("Desktop 产品会话首次推进", async () => {
      const next = await state();
      if (next.errors.length) throw new Error(next.errors.join("\n"));
      return next.tick >= 1 && next.speed === "1" ? next : undefined;
    }, 35_000);
    const fixed = await waitFor("Desktop 1× 前端和采样", async () => {
      const next = await state();
      return next.tick >= ready.tick + 2 && /最近 \d+ms 推进 \d+ tick/.test(next.metrics) ? next : undefined;
    }, 8_000);
    console.log(JSON.stringify({ phase: "one-x", ...fixed, uiTicksPerSecond: (fixed.tick - ready.tick) * 1_000 / (fixed.ms - ready.ms) }));
    const adjusted = await app.evaluate(`(() => {
      const select = document.querySelector(".speed-select select");
      if (!select || select.disabled || ![...select.options].some(option => option.value === "Infinity")) return false;
      select.value = "Infinity";
      select.dispatchEvent(new Event("change", { bubbles: true }));
      return true;
    })()`);
    assert.equal(adjusted, true, "Desktop 未能切换 MAX");
    const samples = [];
    for (let index = 0; index < 3; index++) {
      const prior = samples.at(-1) ?? fixed;
      const sample = await waitFor("Desktop MAX 产品测量窗", async () => {
        const next = await state();
        if (next.errors.length) throw new Error(next.errors.join("\n"));
        const parsed = /^最近 (\d+)ms 推进 (\d+) tick/.exec(next.metrics);
        if (next.speed !== "Infinity" || next.tick <= prior.tick || next.ms - prior.ms < 750 || parsed === null) return undefined;
        return { ...next, authorityTicksPerSecond: Number(parsed[2]) * 1_000 / Number(parsed[1]), uiNetTicksPerSecond: (next.tick - prior.tick) * 1_000 / (next.ms - prior.ms) };
      }, 8_000);
      assert.ok(Number.isFinite(sample.authorityTicksPerSecond) && sample.authorityTicksPerSecond > 0);
      samples.push(sample);
      console.log(JSON.stringify({ phase: "max", ...sample }));
    }
  }
} finally {
  app.close();
}
