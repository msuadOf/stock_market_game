import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { describe, it } from "node:test";

// These failure, optional-host, and zoom-interaction paths have no deterministic
// browser fixture yet. Other display and interactions use rendered-component and Playwright tests.
const app = readFileSync(new URL("../App.tsx", import.meta.url), "utf8");
const detail = readFileSync(new URL("./MobileStockDetail.tsx", import.meta.url), "utf8");
const localViews = readFileSync(new URL("../app/LocalRefreshViews.tsx", import.meta.url), "utf8");
const desktopChart = readFileSync(new URL("../components/PriceChart.tsx", import.meta.url), "utf8");

describe("remaining app visibility wiring guards", () => {
  it("shows a complete speed-metrics error beside the controls", () => {
    assert.match(app, /host\.readSpeedMetrics\(\)/);
    assert.match(app, /setSpeedMetricsError\(`实际倍速读取失败：\$\{/);
    assert.match(app, /className="speed-metrics-error" role="alert">\{speedMetricsError\}/);
  });

  it("offers Publisher switching only through host-declared capabilities", () => {
    assert.match(app, /host\.capabilities\.deliveryModes/);
    assert.match(app, /deliveryMode !== null && deliveryModes\.length > 0/);
    assert.match(app, /host\.setDeliveryMode\(mode\)/);
  });

  it("renders fatal protocol errors with details and a recovery action", () => {
    assert.match(app, /<div className="app-error" role="alert"/);
    assert.match(app, /行情引擎或协议无法继续/);
    assert.match(app, /window\.location\.reload\(\)/);
    assert.match(app, /failure\.code} @ \$\{failure\.where}: \$\{failure\.message/);
  });

  it("keeps K-line reset available after zooming beyond the default range", () => {
    assert.match(detail, /disabled=\{atDefaultZoom && window\.offsetFromEnd === 0\}/);
    assert.doesNotMatch(detail, /disabled=\{atLargestZoom && window\.offsetFromEnd === 0\}/);
  });

  it("keeps desktop trade volume in lots while showing account amounts in yuan", () => {
    assert.match(localViews, /formatSharesAsLots\(level\[1\]\)/);
    assert.match(localViews, /成交量（手）<\/th>/);
    assert.match(localViews, /formatSharesAsLots\(trade\.qty\)/);
    assert.match(desktopChart, /value: \(d\.volume \?\? 0\) \/ 100/);
    assert.match(localViews, /formatYuanAmount\(totalAssets \/ 100\)/);
    assert.match(localViews, /formatYuanAmount\(totalMarketValue \/ 100\)/);
    assert.match(localViews, /formatYuanAmount\(position\.marketValue \/ 100\)/);
  });
});
