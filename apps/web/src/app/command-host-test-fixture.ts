import type { EngineHost } from "../host/engine-host.ts";
import { baseState } from "../host/protocol-test-fixtures.ts";
import { parseSaveSlot } from "../save/save-schema.ts";
import { currentSaveFixture } from "../save/current-save-fixture.ts";

/** 未声明的宿主操作一旦被命令误调用就显式失败，不能靠不完整类型断言通过测试。 */
export function commandHostFixture(overrides: Partial<EngineHost> = {}): EngineHost {
  const unsupported = (): never => { throw new Error("本定向 fixture 未授权此宿主操作"); };
  return {
    capabilities: { deliveryModes: [], targetUiHz: 62.5, sharedMemory: false, reconnect: false, publicCompanyReports: false, npcDecisionDiagnostics: false },
    start: unsupported, stop: unsupported, dispose: unsupported, setSpeed: unsupported, setFrameRate: unsupported,
    setPausePreferences: async () => unsupported(), readSpeedMetrics: async () => unsupported(), submitIntent: async () => unsupported(),
    snapshot: () => baseState().snapshot, tick: unsupported, day: unsupported, save: async () => unsupported(),
    refreshBaseline: async () => unsupported(), playerWorkingOrders: async () => unsupported(), queryStockHistory: async () => unsupported(), initialAllocation: async () => unsupported(), calculateIndicators: async () => unsupported(),
    load: async () => unsupported(), ...overrides,
  };
}

/** 命令边界 fixture 经过真实 schema 校验；不宣称已由 Rust 恢复验收。 */
export function commandDayEndArchiveFixture() {
  const raw = currentSaveFixture();
  return parseSaveSlot({ ...raw, civil_clock: { ...(raw.civil_clock as Record<string, unknown>), settled_through: "2029-12-31" } });
}
