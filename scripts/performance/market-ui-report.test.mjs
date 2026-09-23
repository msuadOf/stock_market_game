import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { analyzeChartProgress, buildHtmlReport, formatBrowserException } from "./market-ui-report-lib.mjs";

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
