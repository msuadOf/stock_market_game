import assert from "node:assert/strict";
import test from "node:test";
import { createSaveCommands, type SaveCommandPorts } from "./useSaveCommands.ts";
import { DEFAULT_SETUP } from "../config/defaults.ts";
import { SessionReplacementGate, InitialSaveSource } from "../save/session-replacement.ts";
import { DayEndPersistence } from "../save/day-end-persistence.ts";
import { PlayerOrderRefreshGate } from "../components/player-orders.ts";
import { SpeedMetricsRequestGate } from "../host/speed.ts";
import type { SessionSetup } from "../types/engine.ts";
import { commandHostFixture } from "./command-host-test-fixture.ts";
import { createSeedDraft } from "../config/seed-draft.ts";
import { createCompanyInitialPreset } from "../config/company-initial-preset.ts";
import type { SimpleCompanyConfig } from "../types/generated/SimpleCompanyConfig";

function fixture() {
  let installed: SessionSetup | null = null;
  const notices: string[] = [];
  const calls: string[] = [];
  const ports: SaveCommandPorts = {
    browserLocal: true, hostRef: { current: null }, initialSaveSourceRef: { current: new InitialSaveSource() }, dayEndPersistenceRef: { current: new DayEndPersistence() }, autoOrderMgrRef: { current: null },
    sessionReplacementGateRef: { current: new SessionReplacementGate() }, saveSelectionGenerationRef: { current: 0 }, dayEndFileTargetRef: { current: null },
    playerOrderRefreshGateRef: { current: new PlayerOrderRefreshGate() }, speedMetricsLoadInProgressRef: { current: false }, speedMetricsRequestGateRef: { current: new SpeedMetricsRequestGate() }, fatalHostErrorRef: { current: () => {} },
    activeSetup: DEFAULT_SETUP, startDateDraft: "2030-01-02", priceCageEnabledDraft: true, floatAllocationDraft: DEFAULT_SETUP.float_allocation, reportFrequencyDraft: "Quarterly", companySystemDraft: JSON.stringify(DEFAULT_SETUP.company_system), dividendTaxModeDraft: DEFAULT_SETUP.dividend_tax_mode, seedDraft: "18446744073709551615",
    loadFromFile: async () => null, selectDayEndFileTarget: async () => null,
    getBrowserSaveRepository: () => ({ load: async () => null, select: async () => false, cancelPending: () => calls.push("cancel"), newSlot: () => calls.push("new-slot") }),
    resetMarketHistory: () => {}, configureMarketTiming: () => {}, refreshPlayerOrders: async () => {}, clearPlayerOrders: () => {},
    setNotice: notice => notices.push(notice), setError: () => {}, setReady: () => {},
    setSessionCreation: (setup, seed) => { assert.equal(seed, "18446744073709551615"); installed = setup; }, setSeedDraft: () => {},
    setActiveSetup: () => {}, setStartDateDraft: () => {}, setPriceCageEnabledDraft: () => {}, setFloatAllocationDraft: () => {}, setReportFrequencyDraft: () => {}, setCompanySystemDraft: () => {}, setDividendTaxModeDraft: () => {}, setInitialAllocation: () => {},
    setStartDateError: () => {}, setSpeedMetricsPollingGeneration: () => {}, setSpeedMetrics: () => {}, setSpeedMetricsError: () => {},
  };
  return { ports, notices, calls, installed: () => installed };
}

test("非法公司配置在替换门禁前拒绝，不创建空资金或清理当前市场", { timeout: 10000 }, async () => {
  for (const draft of ["", "{", JSON.stringify({ mode: "Simulation" }), JSON.stringify({ mode: "Other" })]) {
    const current = fixture(); current.ports.companySystemDraft = draft;
    await createSaveCommands(current.ports).newGame();
    assert.equal(current.installed(), null);
    assert.deepEqual(current.calls, []);
    assert.equal(current.ports.saveSelectionGenerationRef.current, 0);
    assert.match(current.notices.join("\n"), /公司基本面设置无效/);
  }
});

test("新局明确携带可编辑公司配置，不写回当前市场或默认模板", { timeout: 10000 }, async () => {
  const current = fixture();
  const original = JSON.stringify(DEFAULT_SETUP.company_system);
  const config = structuredClone(DEFAULT_SETUP.company_system);
  if (config.mode !== "Simple") throw new Error("测试要求 Simple");
  config.config.environment.noise.monthly_bp = 25;
  current.ports.companySystemDraft = JSON.stringify(config);
  await createSaveCommands(current.ports).newGame();
  assert.deepEqual(current.installed()?.company_system, config);
  assert.equal(JSON.stringify(DEFAULT_SETUP.company_system), original);
  assert.equal(current.installed()?.start_date, "2030-01-02");
  assert.deepEqual(current.calls, ["cancel", "new-slot"]);
});

test("价格反推仅形成 seed 预览中的开局参数，实际新局原样消费且不二次反推", { timeout: 10000 }, async () => {
  const current = fixture();
  const seed = 18446744073709551615n;
  const preview = createSeedDraft(DEFAULT_SETUP, seed);
  current.ports.companySystemDraft = preview.companySystem;
  current.ports.seedDraft = preview.seed;
  await createSaveCommands(current.ports).newGame();
  const installed = current.installed();
  assert.notEqual(installed, null);
  assert.deepEqual(installed?.company_system, JSON.parse(preview.companySystem));
  if (installed?.company_system.mode !== "Simple") throw new Error("测试要求 Simple 新局");
  for (const stock of DEFAULT_SETUP.stocks) {
    const expected = createCompanyInitialPreset(stock, seed, 1);
    const actual: SimpleCompanyConfig | undefined = installed.company_system.config.companies.find((company: SimpleCompanyConfig) => company.company === `C-${stock.code}`);
    assert.equal(actual?.generation.initial_revenue, expected.initial_revenue);
    assert.equal(actual?.generation.initial_fixed_expense, expected.initial_fixed_expense);
    assert.deepEqual(actual?.finance.opening_lines, [{ account: "simple_receivable", side: "Debit", amount: expected.initial_equity }, { account: "4001", side: "Credit", amount: expected.initial_equity }]);
  }
});

test("远程重置使用同一公司配置而不另建浏览器槽，保持市场控制与账户独立", { timeout: 10000 }, async () => {
  const current = fixture();
  let submitted: SessionSetup | null = null;
  const host = commandHostFixture({
    capabilities: { persistence: "remote", deliveryModes: ["push"], targetUiHz: 60, sharedMemory: false, reconnect: true, publicCompanyReports: true, npcDecisionDiagnostics: false, indicatorCapabilities: { intradayAverage: true, macd: false, priceKdj: false, candleKdj: false }, personalTradeHistory: true },
    resetMarket: async (setup, seed) => { assert.equal(seed, 18446744073709551615n); submitted = setup; },
  });
  current.ports.hostRef.current = host;
  await createSaveCommands(current.ports).newGame();
  assert.notEqual(submitted, null);
  assert.deepEqual((submitted as SessionSetup | null)?.company_system, DEFAULT_SETUP.company_system);
  assert.equal(current.installed(), null);
  assert.deepEqual(current.calls, []);
  assert.match(current.notices.join("\n"), /控制授权不从存档恢复/);
});

test("无效 seed 在新局替换前显错，不创建半份 setup／seed", { timeout: 10000 }, async () => {
  const current = fixture();
  current.ports.seedDraft = "18446744073709551616";
  await createSaveCommands(current.ports).newGame();
  assert.equal(current.installed(), null);
  assert.deepEqual(current.calls, []);
  assert.match(current.notices.join("\n"), /新局 seed 无效/);
});
