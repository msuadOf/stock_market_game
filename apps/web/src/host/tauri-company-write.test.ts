import assert from "node:assert/strict";
import test, { type TestContext } from "node:test";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { createTauriHost } from "./tauri-host.ts";
import { CompanyWriteRejectedError } from "./company-write.ts";
import { catalogWireFixture } from "./catalog-test-fixture.ts";
import { market } from "./protocol-test-fixtures.ts";
import type { SessionSetup } from "../types/engine.ts";

// synthetic transport fixture：只检查实际 Web Host 接线，不冒真实 IPC actor 或 Rust 业务成功。
const proposal = { CashDividend: { company: 3, gross_per_share: "5" } };
const correction = { operation_id: "真实入口接线检查", company: 3, supersedes: 7, entries: [{ source: 900000001, date: "2030-06-03", kind: "CashRevenue", cash_flow: "Operating", lines: [{ account: "1002", side: "Debit", amount: "10.00" }, { account: "6001", side: "Credit", amount: "10.00" }] }], reason: "transport fixture" };
const proposalReceipt = { account: "0", company: 3, kind: "CashDividend", event_id: "synthetic-receipt", approved_on: "2030-06-03" };
const operations = [
  { method: "proposePlayerCompanyAction", command: "propose_player_company_action", field: "proposal", input: proposal, response: { receipt: proposalReceipt }, expected: proposalReceipt },
  { method: "enqueueReportCorrection", command: "enqueue_report_correction", field: "request", input: correction, response: { operation_id: correction.operation_id }, expected: { operation_id: correction.operation_id } },
  { method: "cancelReportCorrection", command: "cancel_report_correction", field: "operationId", input: correction.operation_id, response: { operation_id: correction.operation_id }, expected: { operation_id: correction.operation_id } },
] as const;
const failure = { kind: "PlayerProposal", data: { account: "0", company: 3, kind: "CashDividend", day: "2030-06-03", classification: "BusinessCondition", reason: "真实公开领域拒绝 fixture" } };

async function fixture(t: TestContext, respond: (command: string, generation: string) => unknown | Promise<unknown>) {
  const originalWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
  Object.defineProperty(globalThis, "window", { configurable: true, value: { crypto: globalThis.crypto } });
  const snapshot = { seq: 0, tick: 0, day: 0, phase: "Continuous", markets: { "3": market() }, accounts: {}, daily_candles: { "3": [] }, active_daily_candles: {} };
  const calls: { command: string; args: unknown }[] = [];
  let generation = "1";
  mockIPC((command, args) => {
    if (command === "create_session") return "current-session";
    if (command === "host_capabilities") return { npcDecisionDiagnostics: false };
    if (command === "engine_baseline" || command === "restore_session") {
      if (command === "restore_session") generation = "2";
      return { ...catalogWireFixture(snapshot), snapshot, generation, timeline_id: `timeline-${generation}`, civil_date: "2030-06-03" };
    }
    if (["stop_session", "pause_session", "resume_session"].includes(command)) return null;
    calls.push({ command, args });
    return respond(command, generation);
  }, { shouldMockEvents: true });
  let host: Awaited<ReturnType<typeof createTauriHost>> | undefined;
  t.after(async () => {
    await host?.dispose();
    clearMocks();
    if (originalWindow === undefined) delete (globalThis as { window?: unknown }).window;
    else Object.defineProperty(globalThis, "window", originalWindow);
  });
  host = await createTauriHost({} as SessionSetup, 1n);
  const invoke = (method: string, input: unknown): Promise<unknown> => {
    const command = (host as unknown as Record<string, unknown>)[method];
    assert.equal(typeof command, "function", `实际 Tauri EngineHost 必须提供 ${method}`);
    return (command as (value: unknown) => Promise<unknown>)(input);
  };
  const fail = () => emit("engine-failure", { session_id: "current-session", timeline_id: `timeline-${generation}`, code: "INVARIANT_VIOLATION", where: "fixture.host", message: "当前权威已失效", events: [] });
  return { host, calls, invoke, fail };
}

for (const operation of operations) test(`Tauri ${operation.method} 经实际 Host 转发当前 generation 与真实响应`, { timeout: 10000 }, async t => {
  const f = await fixture(t, command => { assert.equal(command, operation.command); return { generation: "1", ...operation.response }; });
  assert.deepEqual(await f.invoke(operation.method, operation.input), operation.expected);
  assert.deepEqual(f.calls, [{ command: operation.command, args: { sessionId: "current-session", generation: "1", [operation.field]: operation.input } }]);
});

test("Tauri typed四分类拒绝保公开failure/cause，普通拒绝不销毁当前Host", { timeout: 10000 }, async t => {
  const f = await fixture(t, () => Promise.reject(failure));
  await assert.rejects(f.invoke("proposePlayerCompanyAction", proposal), error => error instanceof CompanyWriteRejectedError && assert.deepEqual(error.failure, failure) === undefined && error.cause === error.failure);
  assert.equal(f.host.tick(), 0);
});

test("Tauri typed拒绝与成功同样拒错owner，畸形failure保协议错误cause", { timeout: 10000 }, async t => {
  const responses = [
    () => ({ generation: "1", receipt: { ...proposalReceipt, account: "1" } }),
    () => Promise.reject({ ...failure, data: { ...failure.data, account: "1" } }),
    () => Promise.reject({ ...failure, data: { ...failure.data, classification: "Other" } }),
    () => ({ generation: "2", receipt: proposalReceipt }),
  ];
  const f = await fixture(t, () => { const respond = responses.shift(); assert.ok(respond); return respond(); });
  for (let index = 0; index < 4; index++) await assert.rejects(f.invoke("proposePlayerCompanyAction", proposal));
});

test("Tauri engine-failure之后三写均拒绝，旧baseline不得继续invoke", { timeout: 10000 }, async t => {
  const f = await fixture(t, (command, generation) => command === "propose_player_company_action" ? { generation, receipt: proposalReceipt } : { generation, operation_id: correction.operation_id });
  await f.fail();
  for (const operation of operations) await assert.rejects(f.invoke(operation.method, operation.input), /失效|过期|fatal/);
  assert.equal(f.calls.length, 0);
});

test("Tauri同代refresh不解除fatal写门，正常restore的新authority才可恢复三写", { timeout: 10000 }, async t => {
  const f = await fixture(t, (_command, generation) => ({ generation, receipt: proposalReceipt }));
  await f.fail();
  await f.host.refreshBaseline();
  await assert.rejects(f.invoke("proposePlayerCompanyAction", proposal), /失效|过期|fatal/);
  await f.host.load({});
  assert.deepEqual(await f.invoke("proposePlayerCompanyAction", proposal), proposalReceipt);
  assert.equal(f.calls.length, 1);
});

for (const response of ["success", "rejection"] as const) for (const change of ["refresh", "restore", "dispose", "failure"] as const) test(`Tauri迟到${response}在${change}后显式过期且保原cause`, { timeout: 10000 }, async t => {
  let finish!: (value: unknown) => void;
  let reject!: (value: unknown) => void;
  const f = await fixture(t, () => new Promise((resolve, rejected) => { finish = resolve; reject = rejected; }));
  const pending = f.invoke("proposePlayerCompanyAction", proposal);
  const rejected = assert.rejects(pending, error => error instanceof Error && /过期|失效|销毁/.test(error.message) && (response !== "rejection" || error.cause instanceof CompanyWriteRejectedError));
  if (change === "dispose") await f.host.dispose(); else if (change === "restore") await f.host.load({}); else if (change === "failure") await f.fail(); else await f.host.refreshBaseline();
  if (response === "success") finish({ generation: "1", receipt: proposalReceipt }); else reject(failure);
  await rejected;
});

test("Tauri真正Step对象保原结构与cause，不能改成CompanyWriteRejectedError", { timeout: 10000 }, async t => {
  const step = { code: "INVARIANT_VIOLATION", message: "真实Step公开错误 fixture", where: "session.step", cause: { code: "INNER", message: "原公开链" }, context: { tick: 7 } };
  const f = await fixture(t, () => Promise.reject(step));
  await assert.rejects(f.invoke("proposePlayerCompanyAction", proposal), error => error instanceof Error && !(error instanceof CompanyWriteRejectedError) && error.cause === step);
});
