import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { createTauriHost } from "./tauri-host.ts";
import { DEFAULT_SETUP } from "../config/defaults.ts";
import { parseCompanyCapabilities } from "./company-contract-views.ts";

function companyCapabilitiesWire(): unknown {
  // engine 真实 wire 往返 fixture（与 engine 侧 capabilities_wire 测试同一来源）。
  return JSON.parse(
    readFileSync(new URL("../save/fixtures/company-capabilities-wire.json", import.meta.url), "utf8"),
  );
}

function installCompanyContractMocks(responses: {
  readonly capabilities?: { readonly generation: string; readonly value: unknown };
  readonly explanation?: { readonly generation: string; readonly value: unknown };
  readonly offerings?: { readonly generation: string; readonly value: unknown };
  readonly receipts?: { readonly generation: string; readonly value: unknown };
}, requests: { command: string; args: Record<string, unknown> }[]) {
  mockIPC((command, args) => {
    if (command === "create_session") return { sessionId: "company-session", setup: DEFAULT_SETUP, seed: "1", resumed: false };
    if (command === "host_capabilities") return { npcDecisionDiagnostics: false, indicatorCapabilities: { intradayAverage: true, macd: true, priceKdj: true, candleKdj: true }, personalTradeHistory: true };
    if (command === "engine_baseline") {
      return { snapshot: { seq: 0, tick: 0, day: 0, phase: "Continuous", markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} }, generation: "1", timeline_id: "company-timeline", civil_date: "2030-01-02" };
    }
    if (command === "company_capabilities" || command === "company_period_explanation" || command === "owner_rights_offerings" || command === "owner_flat_withholding_receipts") {
      requests.push({ command, args: args as Record<string, unknown> });
      if (command === "company_capabilities") return responses.capabilities ?? null;
      if (command === "company_period_explanation") return responses.explanation ?? null;
      if (command === "owner_rights_offerings") return responses.offerings ?? null;
      return responses.receipts ?? null;
    }
    return null;
  }, { shouldMockEvents: true });
}

test("Tauri 公司契约查询走专用命令并复用 Web 同源 parser", { timeout: 10000 }, async () => {
  const original = Object.getOwnPropertyDescriptor(globalThis, "window");
  Object.defineProperty(globalThis, "window", { configurable: true, value: { crypto: globalThis.crypto } });
  const requests: { command: string; args: Record<string, unknown> }[] = [];
  const wire = companyCapabilitiesWire();
  installCompanyContractMocks({
    capabilities: { generation: "1", value: wire },
    explanation: {
      generation: "1",
      value: {
        previous: { revenue: "1000000.00", fixed_expense: "900000.00", variable_expense: "0.00" },
        cycle: "Monthly",
        environment_change_bp: 0,
        demand_contribution_bp: 0,
        revenue_segments: [],
        fixed_expense_segments: [],
        variable_expense_segments: [],
        revenue_noise_bp: 0,
        fixed_expense_noise_bp: 0,
        variable_expense_noise_bp: 0,
        restart_revenue: null,
        restart_source: null,
      },
    },
    offerings: { generation: "1", value: [] },
    receipts: { generation: "1", value: [] },
  }, requests);
  try {
    const host = await createTauriHost(DEFAULT_SETUP, 1n);
    assert.equal(typeof host.companyCapabilities, "function", "Tauri 宿主必须暴露能力面查询");
    assert.equal(typeof host.companyPeriodExplanation, "function", "Tauri 宿主必须暴露期间解释查询");
    assert.equal(typeof host.ownerRightsOfferings, "function", "Tauri 宿主必须暴露本人配股权益查询");
    assert.equal(typeof host.ownerFlatWithholdingReceipts, "function", "Tauri 宿主必须暴露简税代扣回执查询");

    const capabilities = await host.companyCapabilities!("C-600101");
    assert.deepEqual(capabilities, parseCompanyCapabilities(wire), "返回值必须来自 Web 同源严格 parser");

    const explanation = await host.companyPeriodExplanation!("C-600101", "2029-12-31");
    assert.equal(explanation.cycle, "Monthly");
    assert.deepEqual(await host.ownerRightsOfferings!(), []);
    assert.deepEqual(await host.ownerFlatWithholdingReceipts!(), []);

    assert.deepEqual(requests, [
      { command: "company_capabilities", args: { sessionId: "company-session", generation: "1", company: "C-600101" } },
      { command: "company_period_explanation", args: { sessionId: "company-session", generation: "1", company: "C-600101", periodEnd: "2029-12-31" } },
      { command: "owner_rights_offerings", args: { sessionId: "company-session", generation: "1" } },
      { command: "owner_flat_withholding_receipts", args: { sessionId: "company-session", generation: "1" } },
    ], "命令与参数必须与 Desktop 注册的 Tauri 命令契约一致");
  } finally {
    clearMocks();
    if (original) Object.defineProperty(globalThis, "window", original);
  }
});

test("Tauri 公司契约查询拒绝坏公司身份与过期 generation", { timeout: 10000 }, async () => {
  const original = Object.getOwnPropertyDescriptor(globalThis, "window");
  Object.defineProperty(globalThis, "window", { configurable: true, value: { crypto: globalThis.crypto } });
  const requests: { command: string; args: Record<string, unknown> }[] = [];
  installCompanyContractMocks({
    capabilities: { generation: "2", value: companyCapabilitiesWire() },
  }, requests);
  try {
    const host = await createTauriHost(DEFAULT_SETUP, 1n);
    await assert.rejects(
      () => host.companyCapabilities!("  "),
      /公司身份/,
      "空公司身份必须本地拒绝且不发起 IPC",
    );
    await assert.rejects(
      () => host.companyCapabilities!("C-600101"),
      /过期/,
      "响应 generation 与查询 generation 不一致必须显式拒绝",
    );
    assert.equal(requests.length, 1, "坏身份不得发起 IPC；过期只影响已发起的一次查询");
  } finally {
    clearMocks();
    if (original) Object.defineProperty(globalThis, "window", original);
  }
});

test("Tauri 公司契约查询对畸形 wire 显式拒绝不默认兼容", { timeout: 10000 }, async () => {
  const original = Object.getOwnPropertyDescriptor(globalThis, "window");
  Object.defineProperty(globalThis, "window", { configurable: true, value: { crypto: globalThis.crypto } });
  const malformed = companyCapabilitiesWire() as Record<string, unknown>;
  delete malformed.issued_shares;
  installCompanyContractMocks({
    capabilities: { generation: "1", value: malformed },
    offerings: { generation: "1", value: [{ event_id: 123 }] },
  }, []);
  try {
    const host = await createTauriHost(DEFAULT_SETUP, 1n);
    await assert.rejects(
      () => host.companyCapabilities!("C-600101"),
      /issued_shares/,
      "缺字段的 wire 必须被同源 parser 显式拒绝",
    );
    await assert.rejects(
      () => host.ownerRightsOfferings!(),
      /owner_rights_offerings\[0\]/,
      "字段类型错配必须显式拒绝并指明位置",
    );
  } finally {
    clearMocks();
    if (original) Object.defineProperty(globalThis, "window", original);
  }
});

test("Desktop 未接线写命令保持显式「不支持」分类", { timeout: 10000 }, async () => {
  const original = Object.getOwnPropertyDescriptor(globalThis, "window");
  Object.defineProperty(globalThis, "window", { configurable: true, value: { crypto: globalThis.crypto } });
  installCompanyContractMocks({}, []);
  try {
    const host = await createTauriHost(DEFAULT_SETUP, 1n);
    assert.equal(host.subscribeRightsOffering, undefined, "认购提交命令仍为 Desktop 未支持（G 批写命令桥接另批）");
    assert.equal(host.setSimplePreferences, undefined, "偏好编辑仍为 Desktop 未支持");
    assert.equal(host.proposeCompanyAction, undefined, "玩家提案仍为 Desktop 未支持");
  } finally {
    clearMocks();
    if (original) Object.defineProperty(globalThis, "window", original);
  }
});
