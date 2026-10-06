import type { EngineHost } from "../host/engine-host.ts";
import { baseState } from "../host/protocol-test-fixtures.ts";
import { parseSaveSlot } from "../save/save-schema.ts";
import { currentSaveFixture } from "../save/current-save-fixture.ts";

/** 未声明的宿主操作一旦被命令误调用就显式失败，不能靠不完整类型断言通过测试。 */
export function commandHostFixture(overrides: Partial<EngineHost> = {}): EngineHost {
  const unsupported = (): never => { throw new Error("本定向 fixture 未授权此宿主操作"); };
  return {
    capabilities: { persistence: "browser", deliveryModes: [], targetUiHz: 62.5, sharedMemory: false, reconnect: false, publicCompanyReports: false, npcDecisionDiagnostics: false, indicatorCapabilities: { intradayAverage: false, macd: false, priceKdj: false, candleKdj: false }, personalTradeHistory: false },
    start: unsupported, stop: unsupported, dispose: unsupported, setSpeed: unsupported, setFrameRate: unsupported,
    setPausePreferences: async () => unsupported(), readSpeedMetrics: async () => unsupported(), submitIntent: async () => unsupported(),
    snapshot: () => baseState().snapshot, tick: unsupported, day: unsupported, save: async () => unsupported(),
    refreshBaseline: async () => unsupported(), playerWorkingOrders: async () => unsupported(), queryStockHistory: async () => unsupported(), initialAllocation: async () => unsupported(), calculateIndicators: async () => unsupported(), calculateIntradayAverage: async () => unsupported(), calculateIntradayAverageCurve: async () => unsupported(), queryPersonalTradeConfirmations: async () => unsupported(),
    submitReportCorrection: async () => unsupported(), cancelReportCorrection: async () => unsupported(), queryReportCorrections: async () => unsupported(),
    queryPersonalTradeHistory: async () => unsupported(),
    queryMarketHistory: async () => unsupported(),
    queryCurrentMinuteHistory: async () => unsupported(),
    load: async () => unsupported(), ...overrides,
  };
}

/** Engine 生成并完成休市日结的命令边界档案。 */
export function commandDayEndArchiveFixture() {
  return parseSaveSlot(currentSaveFixture());
}
