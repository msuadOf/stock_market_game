import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { describe, it } from "node:test";
import { MOBILE_LAYOUT } from "./mobile-layout-spec.ts";

describe("mobile reference layout", () => {
  it("shows unified host speed telemetry and its full failure in a visible alert", () => {
    const app = readFileSync(new URL("../App.tsx", import.meta.url), "utf8");
    assert.match(app, /className="speed-metrics-error" role="alert"/);
    assert.match(app, />\{speedMetricsError\}<\/div>/);
    assert.match(app, /const metrics = await host\.readSpeedMetrics\(\)/);
    assert.match(app, /speedMetricsMatchesUiState\(speedMetrics, speed, running\)/);
    assert.match(app, /\}, \[ready, speed, running, speedMetricsPollingGeneration\]\);/);
    assert.match(app, /speedMetricsRequestGateRef\.current\.isCurrent\(requestGeneration\)/);
    assert.doesNotMatch(app, /showsServerSpeedMetrics/);
  });

  it("matches the measured 390px reference geometry", () => {
    assert.equal(MOBILE_LAYOUT.viewportWidth, 390);
    assert.equal(MOBILE_LAYOUT.statusBar, 0);
    assert.deepEqual(MOBILE_LAYOUT.navigationBar, { min: 48, reference: 52, max: 54 });
    assert.equal(MOBILE_LAYOUT.watchRow, 52);
    assert.equal(MOBILE_LAYOUT.bottomNav, 68);
    assert.ok(Math.abs(MOBILE_LAYOUT.chartRatio - 2) < 0.001);
    assert.deepEqual(MOBILE_LAYOUT.watchlistTabs, ["自选股", "持仓股"]);
  });

  it("uses the red mobile header as the only page title", () => {
    const css = readFileSync(new URL("../App.css", import.meta.url), "utf8");
    assert.match(css, /\.layout-mobile \.pos-panel > \.panel-title,\s*\.layout-mobile \.user-panel > \.panel-title\s*\{\s*display:\s*none;/);
  });

  it("does not repeat quote data in a separate intraday summary row", () => {
    const component = readFileSync(new URL("./MobileStockDetail.tsx", import.meta.url), "utf8");
    const css = readFileSync(new URL("./MobileStockDetail.css", import.meta.url), "utf8");

    assert.doesNotMatch(component, /msd-after-hours/);
    assert.doesNotMatch(component, />盘中交易</);
    assert.doesNotMatch(css, /\.msd-after-hours/);
  });

  it("renders five-level quantities in lots instead of raw shares", () => {
    const component = readFileSync(new URL("./MobileStockDetail.tsx", import.meta.url), "utf8");

    assert.match(component, /aria-label="五档盘口，数量单位为手"/);
    assert.match(component, /formatTradeLots\(level\[1\]\)/);
    assert.doesNotMatch(component, /\{level \? level\[1\] : "--"\}/);
  });

  it("collapses the lunch break into one regular midpoint on a four-part trading axis", () => {
    const component = readFileSync(new URL("./MobileStockDetail.tsx", import.meta.url), "utf8");
    const css = readFileSync(new URL("./MobileStockDetail.css", import.meta.url), "utf8");

    assert.match(component, /x1="37" x2="37"/);
    assert.match(component, /className="msd-session-line" x1="58" x2="58"/);
    assert.match(component, /x1="79" x2="79"/);
    assert.match(component, />11:30\/13:00</);
    assert.doesNotMatch(component, /className="before-lunch"/);
    assert.doesNotMatch(component, /className="after-lunch"/);
    assert.doesNotMatch(component, /msd-session-mid|msd-volume-guide-mid/);
    assert.doesNotMatch(css, /\.msd-session-mid|\.msd-volume-guide-mid/);
  });

  it("renders side-relative depth fills behind every populated order-book quantity", () => {
    const component = readFileSync(new URL("./MobileStockDetail.tsx", import.meta.url), "utf8");
    const css = readFileSync(new URL("./MobileStockDetail.css", import.meta.url), "utf8");

    assert.match(component, /orderBookDepthPercent/);
    assert.match(component, /msd-book-depth sell/);
    assert.match(component, /msd-book-depth buy/);
    assert.match(component, /--depth/);
    assert.match(css, /\.msd-book-depth::before\s*\{/);
    assert.match(css, /width:\s*var\(--depth\);/);
    assert.match(css, /grid-template-columns:[^;]*minmax\(3\.2em,\s*auto\)/);
    assert.match(css, /\.msd-book-depth\s*\{[^}]*align-self:\s*stretch;[^}]*display:\s*flex;/);
    assert.match(css, /\.msd-book-depth::before\s*\{[^}]*inset-block:\s*14%;/);
    assert.doesNotMatch(css, /\.msd-book-depth::before\s*\{[^}]*top:\s*2px;[^}]*bottom:\s*2px;/);
    assert.match(css, /\.msd-book-depth\.sell::before\s*\{[^}]*var\(--msd-fall/);
    assert.match(css, /\.msd-book-depth\.buy::before\s*\{[^}]*var\(--msd-rise/);
  });

  it("renders every market-volume readout in lots while keeping engine data in shares", () => {
    const detail = readFileSync(new URL("./MobileStockDetail.tsx", import.meta.url), "utf8");
    const app = readFileSync(new URL("../App.tsx", import.meta.url), "utf8");
    const desktopChart = readFileSync(new URL("../components/PriceChart.tsx", import.meta.url), "utf8");

    assert.match(detail, /成交量（手）/);
    assert.match(detail, /量:\{formatTradeLots\(volumes\.at\(-1\) \?\? 0\)\}手/);
    assert.match(detail, /量:\{formatTradeLots\(allVolumePoints\.at\(-1\)\?\.volume \?\? 0\)\}手/);
    assert.match(detail, /成交量 <b>\{formatTradeLots\(props\.trades\.reduce/);
    assert.doesNotMatch(detail, /<span>成交股数<\/span>/);

    assert.match(app, /className="ob-qty">\{formatSharesAsLots\(lvl\[1\]\)\}<\/span>/);
    assert.match(app, /<th className="num">成交量（手）<\/th>/);
    assert.match(app, /<td className="num">\{formatSharesAsLots\(t\.qty\)\}<\/td>/);
    assert.match(desktopChart, /value: \(d\.volume \?\? 0\) \/ 100/);
  });

  it("uses the shared recursive formatter for account amounts and turnover", () => {
    const detail = readFileSync(new URL("./MobileStockDetail.tsx", import.meta.url), "utf8");
    const app = readFileSync(new URL("../App.tsx", import.meta.url), "utf8");

    assert.match(detail, /formatYuanAmount\(turnoverYuan\)/);
    assert.match(app, /formatYuanAmount\(totalAssets \/ 100\)/);
    assert.match(app, /formatYuanAmount\(totalMarketValue \/ 100\)/);
    assert.match(app, /formatYuanAmount\(p\.marketValue \/ 100\)/);
  });

  it("renders rise bars hollow red and fall bars solid green across daily candles and volume", () => {
    const css = readFileSync(new URL("./MobileStockDetail.css", import.meta.url), "utf8");
    const component = readFileSync(new URL("./MobileStockDetail.tsx", import.meta.url), "utf8");
    assert.match(css, /\.msd-candle-chart g\.rise rect\s*\{\s*fill:none;/);
    assert.match(css, /\.msd-candle-chart g\.fall rect\s*\{\s*fill:currentColor;/);
    assert.match(css, /\.msd-k-volume rect\.rise\s*\{[^}]*fill:none;/);
    assert.match(css, /\.msd-k-volume rect\.fall\s*\{[^}]*fill:currentColor;/);
    assert.match(component, /className="upper-wick"[^>]*y1=\{y\(wick\.upper\.start\)\}[^>]*y2=\{y\(wick\.upper\.end\)\}/);
    assert.match(component, /className="lower-wick"[^>]*y1=\{y\(wick\.lower\.start\)\}[^>]*y2=\{y\(wick\.lower\.end\)\}/);
    assert.doesNotMatch(component, /y1=\{y\(c\.high\)\}\s+y2=\{y\(c\.low\)\}/);
  });

  it("keeps K-line reset available at the extra zoomed-out levels", () => {
    const component = readFileSync(new URL("./MobileStockDetail.tsx", import.meta.url), "utf8");
    assert.match(component, /disabled=\{atDefaultZoom && window\.offsetFromEnd === 0\}/);
    assert.doesNotMatch(component, /disabled=\{atLargestZoom && window\.offsetFromEnd === 0\}/);
  });

  it("renders each intraday volume sample as a thin line on its authoritative time slot", () => {
    const css = readFileSync(new URL("./MobileStockDetail.css", import.meta.url), "utf8");
    const component = readFileSync(new URL("./MobileStockDetail.tsx", import.meta.url), "utf8");
    assert.match(component, /className=\{point\.buy \? "rise" : "fall"\}/);
    assert.match(component, /auctionPoints\.slice\(-CALL_AUCTION_ENTRY_MINUTES \* AUCTION_VOLUME_LINES_PER_MINUTE\)/);
    assert.match(component, /data-auction-volume-line-count=\{visibleAuctionPoints\.length\}/);
    assert.match(component, /visibleAuctionPoints\.length \/ AUCTION_VOLUME_LINES_PER_MINUTE/);
    assert.match(css, /\.msd-minute-bars i\s*\{[^}]*width:\.5px;/);
    assert.match(css, /\.msd-minute-bars i\.rise\s*\{[^}]*background:var\(--msd-rise\);[^}]*border:0;/);
    assert.doesNotMatch(css, /\.msd-minute-bars i\.auction/);
  });

  it("scales the compact quote header against its own width within safe bounds", () => {
    const css = readFileSync(new URL("./MobileStockDetail.css", import.meta.url), "utf8");
    assert.match(css, /--msd-header-height:\s*clamp\(48px,\s*13\.33cqw,\s*54px\);/);
    assert.match(css, /--msd-switch-offset:\s*clamp\(58px,\s*16\.9cqw,\s*72px\);/);
    assert.match(css, /\.msd-header \.msd-stock-switch\s*\{[^}]*font-size:\s*clamp\(11px,\s*3\.08cqw,\s*13px\);/);
    assert.match(css, /\.msd-header strong\s*\{\s*font-size:\s*clamp\(13\.5px,\s*3\.7cqw,\s*15px\);/);
    assert.match(css, /\.msd-header small\s*\{[^}]*font-size:\s*clamp\(9\.5px,\s*2\.65cqw,\s*11px\);/);
    assert.match(css, /\.msd-header \.msd-security-title\s*\{[^}]*left:\s*50%;[^}]*transform:\s*translateX\(-50%\);/);
    assert.match(css, /\.msd-header \.msd-previous\s*\{[^}]*left:\s*calc\(50%\s*-\s*var\(--msd-switch-offset\)\);/);
    assert.match(css, /\.msd-header \.msd-next\s*\{[^}]*left:\s*calc\(50%\s*\+\s*var\(--msd-switch-offset\)\);/);
    assert.match(css, /\.msd-header \.msd-back::before\s*\{[^}]*width:\s*var\(--msd-back-size\);[^}]*height:\s*var\(--msd-back-size\);[^}]*border-width:\s*0 0 2px 2px;[^}]*transform:\s*translateY\(var\(--msd-back-optical-y\)\) rotate\(45deg\);/);
  });

  it("keeps one authoritative game clock mounted across global and detail headers", () => {
    const component = readFileSync(new URL("./MobileStockDetail.tsx", import.meta.url), "utf8");
    const app = readFileSync(new URL("../App.tsx", import.meta.url), "utf8");
    const clock = readFileSync(new URL("./MobileGameClock.tsx", import.meta.url), "utf8");
    const css = readFileSync(new URL("./MobileGameClock.css", import.meta.url), "utf8");
    assert.match(component, /<MobileGameClock day=\{props\.gameDay\} tick=\{props\.gameTick\} variant="detail" \/>/);
    assert.match(app, /<MobileGameClock day=\{snapshot\.day\} tick=\{snapshot\.tick\} variant="global" \/>/);
    assert.match(clock, /className=\{`mobile-game-clock mobile-game-clock--\$\{variant\}`\} role="timer"/);
    assert.match(css, /\.mobile-game-clock\s*\{[^}]*font-variant-numeric:\s*tabular-nums;/);
  });

  it("shares a stateful pause and play control across global and detail headers", () => {
    const component = readFileSync(new URL("./MobileStockDetail.tsx", import.meta.url), "utf8");
    const app = readFileSync(new URL("../App.tsx", import.meta.url), "utf8");
    const toggle = readFileSync(new URL("./MobileRunToggle.tsx", import.meta.url), "utf8");
    assert.match(component, /<MobileRunToggle running=\{props\.running\} onToggle=\{props\.onPauseToggle\} variant="detail" \/>/);
    assert.match(app, /<MobileRunToggle running=\{running\} onToggle=\{handlePauseToggle\} variant="global" \/>/);
    assert.match(toggle, /aria-label=\{running \? "暂停模拟" : "继续模拟"\}/);
    assert.match(toggle, /mobile-run-toggle__icon--\$\{running \? "pause" : "play"\}/);
    assert.doesNotMatch(toggle, /🤖/);
  });

  it("exposes authoritative chart progress diagnostics for automated performance QA", () => {
    const detail = readFileSync(new URL("./MobileStockDetail.tsx", import.meta.url), "utf8");
    const app = readFileSync(new URL("../App.tsx", import.meta.url), "utf8");

    assert.match(app, /data-game-tick=\{snapshot\.tick\}/);
    assert.match(detail, /data-intraday-count=\{visiblePoints\.length\}/);
    assert.match(detail, /data-kline-count=\{allCandles\.length\}/);
  });

  it("lets the centered watchlist sparkline use the full fixed preview frame", () => {
    const component = readFileSync(new URL("../components/MarketGrid.tsx", import.meta.url), "utf8");
    assert.match(
      component,
      /<svg className=\{`mobile-market-trend \$\{trend\}`\} viewBox="0 0 64 48" preserveAspectRatio="none"/,
    );
  });
});
