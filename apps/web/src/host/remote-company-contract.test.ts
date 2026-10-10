import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { DEFAULT_SETUP } from "../config/defaults.ts";
import { remoteTestContext } from "./remote-test-context.ts";
import { createRemoteHost } from "./remote-host.ts";
import { parseCompanyCapabilities } from "./company-contract-views.ts";

function companyCapabilitiesWire(): unknown {
  return JSON.parse(
    readFileSync(new URL("../save/fixtures/company-capabilities-wire.json", import.meta.url), "utf8"),
  );
}

function json(value: unknown): Response {
  return new Response(JSON.stringify(value), { headers: { "content-type": "application/json" } });
}

/** 远程（Server 宿主）公司共同契约查询的传输/解析契约（任务 178）。 */
async function remoteCompanyFixture(
  respond: (url: URL) => Response | undefined,
  token = "session-token",
) {
  const requests: { url: URL; init: RequestInit | undefined }[] = [];
  // 查询游标需要先有权威基线：socket 首个 send 即回 Baseline（与 reports 用例同构）。
  const socket = {
    readyState: 1,
    onmessage: null as ((event: MessageEvent) => void) | null,
    onerror: null,
    onclose: null,
    close() {},
    send() {},
  };
  const host = await createRemoteHost(DEFAULT_SETUP, 1n, {
    context: remoteTestContext("session-1"),
    baseUrl: "https://company.example",
    token,
    fetchFn: async (input, init) => {
      const url = new URL(input instanceof Request ? input.url : String(input));
      if (url.pathname === "/api/host-capabilities") return json({ npcDecisionDiagnostics: false, indicatorCapabilities: { intradayAverage: true, macd: true, priceKdj: true, candleKdj: true }, personalTradeHistory: true });
      requests.push({ url, init });
      const response = respond(url);
      if (response !== undefined) return response;
      return json(null);
    },
    // 工厂返回后由微任务推送 Baseline（宿主已挂载 onmessage），与既有远程用例同构。
    webSocketFactory: () => {
      queueMicrotask(() => socket.onmessage?.({ data: JSON.stringify({ Baseline: {
        timeline_generation: 1,
        snapshot: { seq: 0, tick: 0, day: 0, phase: "Continuous", markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} },
        civil_date: "2030-01-02", public_revision: 0, public_report_ids: [],
      } }) } as MessageEvent));
      return socket as unknown as WebSocket;
    },
  });
  await host.start(() => {});
  return { host, requests };
}

test("远程公司契约查询走专用路由并复用 Web 同源 parser", { timeout: 10000 }, async () => {
  const wire = companyCapabilitiesWire();
  const { host, requests } = await remoteCompanyFixture((url) => {
    if (url.pathname === "/api/companies/C-600101/capabilities") {
      return json({ generation: "1", capabilities: wire });
    }
    if (url.pathname === "/api/companies/C-600101/period-explanation") {
      return json({
        generation: "1",
        explanation: {
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
      });
    }
    if (url.pathname === "/api/owner/rights-offerings") return json({ generation: "1", offerings: [] });
    if (url.pathname === "/api/owner/flat-withholding-receipts") return json({ generation: "1", receipts: [] });
    return undefined;
  });
  assert.equal(typeof host.companyCapabilities, "function", "远程宿主必须暴露能力面查询");
  assert.equal(typeof host.companyPeriodExplanation, "function", "远程宿主必须暴露期间解释查询");
  assert.equal(typeof host.ownerRightsOfferings, "function", "远程宿主必须暴露本人配股权益查询");
  assert.equal(typeof host.ownerFlatWithholdingReceipts, "function", "远程宿主必须暴露简税代扣回执查询");

  assert.deepEqual(await host.companyCapabilities!("C-600101"), parseCompanyCapabilities(wire));
  const explanation = await host.companyPeriodExplanation!("C-600101", "2029-12-31");
  assert.equal(explanation.cycle, "Monthly");
  assert.deepEqual(await host.ownerRightsOfferings!(), []);
  assert.deepEqual(await host.ownerFlatWithholdingReceipts!(), []);

  assert.equal(requests.length, 4);
  assert.equal(requests[0]!.url.pathname, "/api/companies/C-600101/capabilities");
  assert.deepEqual([...requests[0]!.url.searchParams], [["session_id", "session-1"], ["generation", "1"]]);
  assert.equal(requests[0]!.init?.method, "GET");
  assert.equal(new Headers(requests[0]!.init?.headers).get("authorization"), "Bearer session-token");
  assert.deepEqual([...requests[1]!.url.searchParams], [["session_id", "session-1"], ["generation", "1"], ["period_end", "2029-12-31"]]);
});

test("远程公司契约查询拒绝过期 generation 与畸形 wire", { timeout: 10000 }, async () => {
  const malformed = companyCapabilitiesWire() as Record<string, unknown>;
  delete malformed.issued_shares;
  let call = 0;
  const { host } = await remoteCompanyFixture(() => {
    call += 1;
    if (call === 1) return json({ generation: "2", capabilities: companyCapabilitiesWire() });
    return json({ generation: "1", capabilities: malformed });
  });
  await assert.rejects(() => host.companyCapabilities!("C-600101"), /过期/, "响应 generation 与查询 generation 不一致必须显式拒绝");
  await assert.rejects(() => host.companyCapabilities!("C-600101"), /issued_shares/, "缺字段 wire 必须被同源 parser 显式拒绝");
});

test("远程公司契约查询坏身份本地拒绝不起网络请求", { timeout: 10000 }, async () => {
  const { host, requests } = await remoteCompanyFixture(() => undefined);
  await assert.rejects(() => host.companyCapabilities!(" "), /公司身份/);
  await assert.rejects(() => host.companyPeriodExplanation!("C-600101", "2029/12/31"), /periodEnd/);
  assert.equal(requests.length, 0, "本地校验失败不得发起网络请求");
});

test("远程宿主未接线面保持显式「不支持」分类（方法缺失即 UI 显式提示）", { timeout: 10000 }, async () => {
  const { host } = await remoteCompanyFixture(() => undefined);
  // 写命令与偏好/税账状态/拒绝回执面按批次登记仍为远程未支持：不得半接线。
  assert.equal(host.subscribeRightsOffering, undefined, "认购提交命令仍为远程未支持");
  assert.equal(host.setSimplePreferences, undefined, "偏好编辑仍为远程未支持");
  assert.equal(host.companySimplePreferences, undefined, "偏好查询仍为远程未支持");
  assert.equal(host.proposeCompanyAction, undefined, "玩家提案仍为远程未支持");
  assert.equal(host.dividendTaxStatus, undefined, "税账状态查询仍为远程未支持");
  assert.equal(host.rejectedRightsSubscriptions, undefined, "认购拒绝回执查询仍为远程未支持");
  assert.equal(host.companyPreferenceRejections, undefined, "偏好拒绝台账仍为远程未支持");
});
