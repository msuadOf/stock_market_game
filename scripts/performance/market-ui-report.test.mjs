import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { analyzeChartProgress, buildHtmlReport, formatBrowserException } from "./market-ui-report-lib.mjs";

import * as marketReport from "./market-ui-report.mjs";

const before = {
  game: { day: 0, tick: 100 },
  intraday: { count: 2, latestMinute: 1, signature: "1:10.01" },
  kline: { count: 360, signature: "359:10.00" },
};

describe("market UI performance report", () => {
  it("keeps the browser exception description instead of reporting only Uncaught", () => {
    assert.equal(formatBrowserException({
      text: "Uncaught",
      exception: { description: "Error: 分时诊断节点不存在\n    at <anonymous>:3:22" },
    }), "Error: 分时诊断节点不存在\n    at <anonymous>:3:22");
  });

  it("passes when game time, intraday, and K-line diagnostics all advance", () => {
    const result = analyzeChartProgress(before, {
      game: { day: 1, tick: 15_000 },
      intraday: { count: 11, latestMinute: 10, signature: "10:10.23" },
      kline: { count: 360, signature: "360:10.20" },
    });

    assert.equal(result.passed, true);
    assert.deepEqual(result.checks.map((check) => check.passed), [true, true, true]);
  });

  it("fails loudly when either chart is visually stale despite a moving game clock", () => {
    const result = analyzeChartProgress(before, {
      ...before,
      game: { day: 0, tick: 200 },
    });

    assert.equal(result.passed, false);
    assert.match(result.checks.find((check) => check.id === "intraday")?.detail ?? "", /未推进/);
    assert.match(result.checks.find((check) => check.id === "kline")?.detail ?? "", /未推进/);
  });

  it("embeds metrics and screenshot links in the generated HTML", () => {
    const html = buildHtmlReport({
      generatedAt: "2026-01-01T00:00:00.000Z",
      url: "http://127.0.0.1:5173/",
      durationMs: 5000,
      result: analyzeChartProgress(before, { ...before, game: { day: 0, tick: 200 } }),
      before,
      after: { ...before, game: { day: 0, tick: 200 } },
      performance: { TaskDurationMs: 12.3, JSHeapUsedSizeDelta: 1024 },
      screenshots: ["01-intraday-before.png", "02-intraday-after.png"],
    });

    assert.match(html, /01-intraday-before\.png/);
    assert.match(html, /TaskDurationMs/);
    assert.match(html, /FAIL/);
  });
});


describe("MarketUiReportRun 资源会话", () => {
  function fixture(failAt) {
    const calls = [];
    const step = async (name, value) => { calls.push(name); if (name === failAt) throw new Error(name); return value; };
    const server = { recentOutput: () => "fixture Vite output" };
    const browser = { exitCode: null };
    const client = { connect: () => step("connect"), close: () => { calls.push("client.close"); if (failAt === "client.close") throw new Error(failAt); } };
    const dependencies = {
      endpointReady: async () => false,
      startViteIfNeeded: () => server,
      waitFor: (name) => step(name),
      freePort: () => step("port", 1234),
      mkdtemp: () => step("profile", "/fixture/profile"),
      browserExecutable: () => "fixture-browser",
      spawn: () => browser,
      fetch: () => step("target", { ok: true, json: async () => ({ webSocketDebuggerUrl: "ws://fixture" }) }),
      createClient: () => client,
      stopProcess: (child) => step(child === browser ? "browser.stop" : child === server ? "server.stop" : "null.stop"),
      rm: () => step("profile.rm"),
    };
    return { calls, dependencies };
  }

  it("正常会话按 client、browser、server、profile 顺序单次清理", async () => {
    assert.equal(typeof marketReport.MarketUiReportRun, "function");
    const { calls, dependencies } = fixture();
    const run = new marketReport.MarketUiReportRun({ url: "http://localhost/", browser: "" }, dependencies);
    await run.startServerIfNeeded();
    await run.launchBrowser();
    await run.connectPage();
    await run.close();
    assert.deepEqual(calls.slice(-4), ["client.close", "browser.stop", "server.stop", "profile.rm"]);
  });

  it("连接失败保留已取得 client，清理失败按原有边界停止后续步骤", async () => {
    assert.equal(typeof marketReport.MarketUiReportRun, "function");
    for (const failAt of ["connect", "client.close", "browser.stop", "server.stop", "profile.rm"]) {
      const { calls, dependencies } = fixture(failAt);
      const run = new marketReport.MarketUiReportRun({ url: "http://localhost/", browser: "" }, dependencies);
      await run.startServerIfNeeded();
      await run.launchBrowser();
      if (failAt === "connect") await assert.rejects(run.connectPage(), /connect/);
      else await run.connectPage();
      if (failAt === "connect") await run.close();
      else await assert.rejects(run.close(), new RegExp(failAt));
      const cleanup = ["client.close", "browser.stop", "server.stop", "profile.rm"];
      const count = failAt === "connect" ? 4 : cleanup.indexOf(failAt) + 1;
      assert.deepEqual(calls.slice(-count), cleanup.slice(0, count));
    }
  });

  it("部分初始化只清理已取得 profile 与原有空 child 分支", async () => {
    assert.equal(typeof marketReport.MarketUiReportRun, "function");
    const { calls, dependencies } = fixture("Chrome 调试端口");
    const run = new marketReport.MarketUiReportRun({ url: "http://localhost/", browser: "" }, dependencies);
    await assert.rejects(run.launchBrowser(), /Chrome/);
    await run.close();
    assert.deepEqual(calls.slice(-3), ["browser.stop", "null.stop", "profile.rm"]);
  });
});

describe("MarketUiReportRun 初始化边界", () => {
  it("远程 URL 不启动 Vite，已有退出 browser 保持 stopProcess 的返回分支", async () => {
    const calls = [];
    const remote = new marketReport.MarketUiReportRun({ url: "https://fixture.invalid/", browser: "" }, {
      endpointReady: async () => false,
      waitFor: async () => {},
      stopProcess: async (child) => { calls.push(child); },
    });
    await remote.startServerIfNeeded();
    await remote.close();
    assert.deepEqual(calls, [null, null]);

    const exited = new marketReport.MarketUiReportRun({ url: "http://localhost/", browser: "" }, {
      endpointReady: async () => true,
      startViteIfNeeded: () => { throw new Error("不应启动 Vite"); },
      waitFor: async () => {},
      freePort: async () => 1234,
      mkdtemp: async () => "/fixture/profile",
      browserExecutable: () => "fixture-browser",
      spawn: () => ({ exitCode: 0, kill: () => { throw new Error("不应终止已退出进程"); } }),
      rm: async (profile) => calls.push(profile),
    });
    await exited.startServerIfNeeded();
    await exited.launchBrowser();
    await exited.close();
    assert.equal(calls.at(-1), "/fixture/profile");
  });
});
