export const baselineDriverSha256 = "6183243bb49c1447fcf66228bacbb59e8ac1c4cbb237e2f8669dc0e82f408410";

// 保留原 companyPanel 的两条 DOM 条件，恢复验证时才能重放 pending 假绿。
export function companyPanelCompletion({ ui }) {
  return { status: ui.alerts.length === 0 && ui.capabilityRegionVisible ? "passed" : "pending", matched: [], failures: [], pending: [] };
}
