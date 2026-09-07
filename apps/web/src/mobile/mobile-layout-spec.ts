/** Measured from the supplied 1200 × 2670 reference at a 390 CSS-pixel viewport. */
export const MOBILE_LAYOUT = Object.freeze({
  viewportWidth: 390,
  statusBar: 0,
  navigationBar: Object.freeze({ min: 48, reference: 52, max: 54 }),
  watchRow: 52,
  bottomNav: 68,
  chartRatio: 2,
  watchlistTabs: ["自选股", "持仓股"] as const,
});
