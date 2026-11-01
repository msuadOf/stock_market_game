import assert from "node:assert/strict";
import test, { type TestContext } from "node:test";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { createTauriHost } from "./tauri-host.ts";
import { CompanyWriteRejectedError } from "./company-write.ts";
import { isReportCorrectionPrepareRejection } from "./protocol-failure.ts";
import { catalogWireFixture } from "./catalog-test-fixture.ts";
import { market, mirrorOrigin } from "./protocol-test-fixtures.ts";
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
  const snapshot = { seq: 0, tick: 0, day: 0, phase: "Continuous", mirror_origin: mirrorOrigin(0), markets: { "3": market() }, accounts: {}, daily_candles: { "3": [] }, active_daily_candles: {} };
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
  const fail = (details: Record<string, unknown> = {}) => emit("engine-failure", { session_id: "current-session", timeline_id: `timeline-${generation}`, code: "INVARIANT_VIOLATION", where: "fixture.host", message: "当前权威已失效", events: [], ...details });
  const publish = (update: unknown) => emit("engine-event", { session_id: "current-session", timeline_id: `timeline-${generation}`, update });
  return { host, calls, invoke, fail, publish };
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


const prepareRejection = {
  code: "REPORT_CORRECTION_PREPARE_REJECTED", where: "desktop.actor.prepare_civil_updates", message: "报表更正未满足提交条件（内部详情已脱敏）",
  recoverable: true, context: { operation: "endCivilDay", tick: 2, seq: 0, day: 1, generation: "1" },
  cause: { code: "REPORT_CORRECTION_REJECTED", message: "报表更正未满足提交条件（内部详情已脱敏）", cause: null },
};

test("Tauri 末拍先交付、可信F2使旧确认过期，新cancel可到actor并civil-only继续", { timeout: 10000 }, async t => {
  let finish!: (value: unknown) => void;
  const f = await fixture(t, (command, generation) => command === "propose_player_company_action"
    ? new Promise(resolve => { finish = resolve; }) : { generation, operation_id: correction.operation_id });
  const observed: string[] = [];
  await f.host.start(update => { observed.push(update.type); }, failure => { observed.push(failure.code); });
  const pending = f.invoke("proposePlayerCompanyAction", proposal);
  const stale = assert.rejects(pending, /过期|失效|中断/);
  await f.publish({ TickBatch: { frames: [{ tick: 2, seq_from: 0, seq_to: 0, facts: [], timeseries_payload: {} }], runtime_snapshot: null } });
  await f.fail(prepareRejection);
  finish({ generation: "1", receipt: proposalReceipt });
  await stale;
  assert.deepEqual(await f.invoke("cancelReportCorrection", correction.operation_id), { operation_id: correction.operation_id });
  const count = observed.length;
  await f.host.start(update => { observed.push(update.type); }, failure => { observed.push(failure.code); });
  await f.publish({ CivilUpdate: { tick: 2 } });
  assert.deepEqual(observed.slice(0, 3), ["baseline", "protocol", "REPORT_CORRECTION_PREPARE_REJECTED"]);
  assert.deepEqual(observed.slice(count), ["protocol"]);
});

for (const invalid of [
  { code: "CIVIL_DAY_SETTLEMENT_FAILED" }, { where: "desktop.actor.callback" }, { recoverable: false },
  { context: { ...prepareRejection.context, operation: "callback" } }, { context: { ...prepareRejection.context, generation: "2" } },
  { context: { ...prepareRejection.context, day: 4_294_967_296 } },
  { cause: { code: "REPORT_CORRECTION_REJECTED" } },
  { cause: { ...prepareRejection.cause, unexpected: true } },
  { cause: { ...prepareRejection.cause, message: "  " } },
  { cause: { ...prepareRejection.cause, cause: { code: "INNER" } } },
  { cause: { ...prepareRejection.cause, code: "REPORT_CORRECTION_FAILED" } },
]) test(`Tauri 非可信F2不能解除fatal写门 ${JSON.stringify(invalid)}`, { timeout: 10000 }, async t => {
  const f = await fixture(t, () => { throw new Error("非F2不能继续invoke"); });
  await f.fail({ ...prepareRejection, ...invalid });
  await assert.rejects(f.invoke("cancelReportCorrection", correction.operation_id), /失效|过期|fatal/);
  assert.equal(f.calls.length, 0);
});

test("Tauri 首fatal之后迟到的F2不能解除写门或替换首错", { timeout: 10000 }, async t => {
  const f = await fixture(t, () => { throw new Error("首fatal后不能invoke"); });
  await f.fail();
  await f.fail(prepareRejection);
  await assert.rejects(f.invoke("cancelReportCorrection", correction.operation_id), error => error instanceof Error && (error.cause as { code?: unknown })?.code === "INVARIANT_VIOLATION");
  assert.equal(f.calls.length, 0);
});

test("Tauri F2 matcher接纳完整嵌套FailureCause并拒绝循环cause", { timeout: 10000 }, () => {
  const nested = { ...prepareRejection, cause: { ...prepareRejection.cause, cause: { code: "INNER", message: "内部拒绝", cause: null } } };
  assert.equal(isReportCorrectionPrepareRejection(nested, "desktop", "1"), true);
  assert.equal(isReportCorrectionPrepareRejection({ ...prepareRejection, context: { ...prepareRejection.context, day: 4_294_967_295 } }, "desktop", "1"), true);
  const cyclicCause: { code: string; message: string; cause: unknown } = { code: "REPORT_CORRECTION_REJECTED", message: "拒绝", cause: null };
  cyclicCause.cause = cyclicCause;
  assert.equal(isReportCorrectionPrepareRejection({ ...prepareRejection, cause: cyclicCause }, "desktop", "1"), false);
});

test("Tauri 配股认购使用当前 generation 并验证受理回执", { timeout: 10000 }, async t => {
  const receipt = { event_id: "r1", requested_shares: "5", submitted_on: "2030-06-03" };
  const f = await fixture(t, (_command, generation) => ({ generation, receipt }));
  assert.deepEqual(await f.host.subscribeRightsOffering!("r1", "5"), receipt);
  assert.deepEqual(f.calls[0], { command: "subscribe_rights_offering", args: { sessionId: "current-session", generation: "1", eventId: "r1", requestedShares: "5" } });
});

test("Tauri 失效后不发送配股认购", { timeout: 10000 }, async t => {
  const f = await fixture(t, () => { throw new Error("不应发送认购"); });
  await f.fail();
  await assert.rejects(f.host.subscribeRightsOffering!("r1", "5"), /失效/);
  assert.equal(f.calls.length, 0);
});

for (const classification of ["InvalidInput", "BusinessCondition", "UnsupportedOperation", "SystemState"]) test(`Tauri 配股认购保留 ${classification} typed 拒绝`, { timeout: 10000 }, async t => {
  const failure = { kind: "RightsSubscription", data: { account: "0", event_id: "r1", classification, reason: "认购被拒绝" } };
  const f = await fixture(t, () => { throw failure; });
  await assert.rejects(f.host.subscribeRightsOffering!("r1", "5"), error => error instanceof CompanyWriteRejectedError && assert.deepEqual(error.failure, failure) === undefined);
});
for (const data of [{ account: "2", event_id: "r1" }, { account: "0", event_id: "other" }]) test(`Tauri 配股认购拒绝响应身份不符 ${JSON.stringify(data)}`, { timeout: 10000 }, async t => {
  const failure = { kind: "RightsSubscription", data: { ...data, classification: "BusinessCondition", reason: "拒绝" } };
  const f = await fixture(t, () => { throw failure; });
  await assert.rejects(f.host.subscribeRightsOffering!("r1", "5"), error => error instanceof AggregateError);
});

test("Tauri 配股认购拒绝畸形字段和错误 family", { timeout: 10000 }, async t => {
  const failures = [{ kind: "RightsSubscription", data: { account: "0", event_id: "r1", classification: "unknown", reason: "拒绝" } }, failure];
  const f = await fixture(t, () => { throw failures.shift(); });
  for (let i = 0; i < 2; i++) await assert.rejects(f.host.subscribeRightsOffering!("r1", "5"), error => error instanceof AggregateError);
});
