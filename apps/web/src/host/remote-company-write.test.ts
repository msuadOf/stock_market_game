import assert from "node:assert/strict";
import test, { type TestContext } from "node:test";
import { createRemoteHost } from "./remote-host.ts";
import { CompanyWriteRejectedError } from "./company-write.ts";
import { isReportCorrectionPrepareRejection } from "./protocol-failure.ts";
import { catalogWireFixture } from "./catalog-test-fixture.ts";
import { remoteTestContext } from "./remote-test-context.ts";
import { market, mirrorOrigin } from "./protocol-test-fixtures.ts";
import type { SessionSetup } from "../types/engine.ts";

// synthetic transport fixture：只检查实际 Web Host 接线，不冒真实 Rust 批准或更正完成。
const proposal = { CashDividend: { company: 3, gross_per_share: "5" } };
const correction = { operation_id: "真实入口接线检查", company: 3, supersedes: 7, entries: [{ source: 900000001, date: "2030-06-03", kind: "CashRevenue", cash_flow: "Operating", lines: [{ account: "1002", side: "Debit", amount: "10.00" }, { account: "6001", side: "Credit", amount: "10.00" }] }], reason: "transport fixture" };
const proposalReceipt = { account: "1", company: 3, kind: "CashDividend", event_id: "synthetic-receipt", approved_on: "2030-06-03" };
const operations = [
  { method: "proposePlayerCompanyAction", path: "/api/propose-player-company-action", field: "proposal", input: proposal, response: { receipt: proposalReceipt }, expected: proposalReceipt },
  { method: "enqueueReportCorrection", path: "/api/enqueue-report-correction", field: "request", input: correction, response: { operation_id: correction.operation_id }, expected: { operation_id: correction.operation_id } },
  { method: "cancelReportCorrection", path: "/api/cancel-report-correction", field: "operation_id", input: correction.operation_id, response: { operation_id: correction.operation_id }, expected: { operation_id: correction.operation_id } },
] as const;
const json = (value: unknown, status = 200) => new Response(JSON.stringify(value), { status, headers: { "content-type": "application/json" } });
const failure = { kind: "PlayerProposal", data: { account: "1", company: 3, kind: "CashDividend", day: "2030-06-03", classification: "BusinessCondition", reason: "真实公开领域拒绝 fixture" } };

async function fixture(t: TestContext, respond: (path: string) => Response | Promise<Response>, context: () => Response | Promise<Response> = () => json(remoteTestContext("current-session"))) {
  const calls: { path: string; body: unknown; token: string | null }[] = [];
  const contexts: { session: string | null; token: string | null }[] = [];
  const snapshot = { seq: 0, tick: 0, day: 0, phase: "Continuous", mirror_origin: mirrorOrigin(0), markets: { "3": market() }, accounts: {}, daily_candles: { "3": [] }, active_daily_candles: {} };
  let closed = 0;
  const updates: unknown[] = [];
  const failures: unknown[] = [];
  const socket = { readyState: 1, close() { closed++; }, send() {}, onmessage: null, onerror: null, onclose: null } as unknown as WebSocket;
  const host = await createRemoteHost({} as SessionSetup, 1n, {
    baseUrl: "https://company-write.example", webSocketFactory: () => socket,
    fetchFn: async (input, init) => {
      const url = new URL(String(input)), path = url.pathname;
      if (path === "/api/new") return json({ session_id: "current-session", session_token: "owner-token" });
      if (path === "/api/host-capabilities") return json({ npcDecisionDiagnostics: false });
      if (path === "/api/running" || (path === "/api/session" && init?.method === "DELETE")) return json({});
      if (path === "/api/market/context") {
        contexts.push({ session: url.searchParams.get("session_id"), token: new Headers(init?.headers).get("authorization") });
        return context();
      }
      calls.push({ path, body: JSON.parse(String(init?.body)), token: new Headers(init?.headers).get("authorization") });
      return respond(path);
    },
  });
  t.after(() => host.dispose());
  await host.start(update => { updates.push(update); }, failure => { failures.push(failure); });
  const publish = (generation = 1) => socket.onmessage!({ data: JSON.stringify({ Baseline: { ...catalogWireFixture(snapshot), timeline_generation: generation, snapshot,
    civil_date: "2030-06-03", public_revision: 0, public_report_ids: [] } }) } as MessageEvent);
  publish();
  const invoke = (method: string, input: unknown): Promise<unknown> => {
    const command = (host as unknown as Record<string, unknown>)[method];
    assert.equal(typeof command, "function", `实际 Remote EngineHost 必须提供 ${method}`);
    return (command as (value: unknown) => Promise<unknown>)(input);
  };
  const fail = (details: Record<string, unknown> = {}) => socket.onmessage!({ data: JSON.stringify({ HostFailure: { code: "BROKEN", where: "fixture.host", message: "当前连接权威失效", ...details } }) } as MessageEvent);
  const publishProtocol = (update: unknown) => socket.onmessage!({ data: JSON.stringify({ PublisherFrame: { timeline_generation: 1, update, civil_date: "2030-06-03", public_revision: 0 } }) } as MessageEvent);
  return { host, calls, contexts, publish, invoke, fail, publishProtocol, updates, failures, closed: () => closed };
}

for (const operation of operations) test(`Remote ${operation.method} 经实际 Host 转发可信身份与当前 generation`, { timeout: 10000 }, async t => {
  const f = await fixture(t, path => { assert.equal(path, operation.path); return json({ generation: "1", ...operation.response }); });
  assert.deepEqual(await f.invoke(operation.method, operation.input), operation.expected);
  assert.deepEqual(f.contexts, [{ session: "current-session", token: "Bearer owner-token" }]);
  assert.deepEqual(f.calls, [{ path: operation.path, body: { session_id: "current-session", generation: "1", [operation.field]: operation.input }, token: "Bearer owner-token" }]);
});

test("Remote Proposal typed四分类拒绝保完整公开failure与cause，不串化或停会话", { timeout: 10000 }, async t => {
  const f = await fixture(t, () => json({ code: "PLAYER_COMPANY_ACTION_REJECTED", failure }, 400));
  await assert.rejects(f.invoke("proposePlayerCompanyAction", proposal), error => error instanceof CompanyWriteRejectedError && assert.deepEqual(error.failure, failure) === undefined && error.cause === error.failure);
});

test("Remote 拒绝错owner、错响应代际与畸形typed failure，不能拿receipt自证本人", { timeout: 10000 }, async t => {
  const responses = [json({ generation: "1", receipt: { ...proposalReceipt, account: "0" } }), json({ generation: "2", receipt: proposalReceipt }), json({ code: "PLAYER_COMPANY_ACTION_REJECTED", failure: { ...failure, data: { ...failure.data, classification: "Other" } } }, 400)];
  const f = await fixture(t, () => { const value = responses.shift(); assert.ok(value); return value; });
  for (let index = 0; index < 3; index++) await assert.rejects(f.invoke("proposePlayerCompanyAction", proposal));
});

test("Remote真实HTTP入口保raw数字与重复字段拒绝，不先JSON.parse舍入", { timeout: 10000 }, async t => {
  const valid = JSON.stringify({ generation: "1", receipt: proposalReceipt });
  const typed = JSON.stringify({ code: "PLAYER_COMPANY_ACTION_REJECTED", failure });
  const responses = [
    new Response(valid.replace('"company":3', '"company":3.00000000000000001')),
    new Response(valid.replace('"company":3', '"company":3,"company":3')),
    new Response(typed.replace('"classification":"BusinessCondition"', '"classification":"BusinessCondition","classification":"SystemState"'), { status: 400 }),
  ];
  const f = await fixture(t, () => { const response = responses.shift(); assert.ok(response); return response; });
  for (let index = 0; index < 3; index++) await assert.rejects(f.invoke("proposePlayerCompanyAction", proposal));
});

test("Remote Proposal无member显式拒绝；Correction controller不伪造owner0", { timeout: 10000 }, async t => {
  const f = await fixture(t, () => json({ generation: "1", operation_id: correction.operation_id }), () => json({ ...remoteTestContext("current-session"), member: null, needs_rejoin: true, can_control: true }));
  await assert.rejects(f.invoke("proposePlayerCompanyAction", proposal), /member|本人|加入/);
  assert.equal(f.calls.length, 0);
  assert.deepEqual(await f.invoke("cancelReportCorrection", correction.operation_id), { operation_id: correction.operation_id });
});

test("Remote HostFailure之后三写均拒绝，不从旧baseline发送context或POST", { timeout: 10000 }, async t => {
  const f = await fixture(t, () => { throw new Error("失效连接不能写入"); });
  f.fail();
  for (const operation of operations) await assert.rejects(f.invoke(operation.method, operation.input), /baseline|连接|失效|过期/);
  assert.equal(f.contexts.length, 0);
  assert.equal(f.calls.length, 0);
});

test("Remote context等待时HostFailure中断，迟到context不得触发POST", { timeout: 10000 }, async t => {
  let finish!: (value: Response) => void;
  const f = await fixture(t, () => { throw new Error("失效连接不能写入"); }, () => new Promise(resolve => { finish = resolve; }));
  const rejected = assert.rejects(f.invoke("proposePlayerCompanyAction", proposal), /中断|过期|失效|连接/);
  f.fail();
  finish(json(remoteTestContext("current-session")));
  await rejected;
  assert.equal(f.calls.length, 0);
});

for (const change of ["session", "generation", "control"] as const) test(`Remote可信context的${change}不匹配时不发送命令`, { timeout: 10000 }, async t => {
  const context = { ...remoteTestContext("current-session"), ...(change === "session" ? { session_id: "other" } : change === "generation" ? { generation: "2" } : { can_control: false }) };
  const f = await fixture(t, () => { throw new Error("不应发送"); }, () => json(context));
  await assert.rejects(f.invoke("cancelReportCorrection", correction.operation_id));
  assert.equal(f.calls.length, 0);
});

for (const generation of [1, 2]) test(`Remote context等待期间baseline变为${generation}则旧context不能发送新写`, { timeout: 10000 }, async t => {
  let finish!: (value: Response) => void;
  const f = await fixture(t, () => { throw new Error("不应发送"); }, () => new Promise(resolve => { finish = resolve; }));
  const pending = assert.rejects(f.invoke("proposePlayerCompanyAction", proposal), /过期|中断|失效/);
  await Promise.resolve();
  f.publish(generation);
  finish(json(remoteTestContext("current-session")));
  await pending;
  assert.equal(f.calls.length, 0);
});

for (const response of ["success", "rejection"] as const) for (const change of ["refresh", "restore", "dispose", "failure"] as const) test(`Remote迟到${response}在${change}后显式过期，保原拒绝cause`, { timeout: 10000 }, async t => {
  let finish!: (value: Response) => void;
  const f = await fixture(t, () => new Promise(resolve => { finish = resolve; }));
  const pending = f.invoke("proposePlayerCompanyAction", proposal);
  const rejected = assert.rejects(pending, error => error instanceof Error && /过期|中断|失效|销毁/.test(error.message) && (response !== "rejection" || change !== "refresh" || error.cause instanceof CompanyWriteRejectedError));
  while (finish === undefined) await new Promise(resolve => setImmediate(resolve));
  if (change === "dispose") await f.host.dispose(); else if (change === "failure") f.fail(); else f.publish(change === "restore" ? 2 : 1);
  finish(response === "success" ? json({ generation: "1", receipt: proposalReceipt }) : json({ code: "PLAYER_COMPANY_ACTION_REJECTED", failure }, 400));
  await rejected;
});


const prepareRejection = {
  code: "REPORT_CORRECTION_PREPARE_REJECTED", where: "server.actor.prepare_civil_updates", message: "报表更正未满足提交条件（内部详情已脱敏）",
  recoverable: true, context: { operation: "endCivilDay", tick: 2, seq: 0, day: 1, generation: "1" },
  cause: { code: "REPORT_CORRECTION_REJECTED", message: "报表更正未满足提交条件（内部详情已脱敏）", cause: null },
};

test("Remote 真实Publisher消息先交末拍、可信F2中断旧确认并保同连接cancel与civil更新", { timeout: 10000 }, async t => {
  let finish!: (value: Response) => void;
  const f = await fixture(t, path => path === "/api/propose-player-company-action"
    ? new Promise(resolve => { finish = resolve; }) : json({ generation: "1", operation_id: correction.operation_id }));
  const pending = f.invoke("proposePlayerCompanyAction", proposal);
  const stale = assert.rejects(pending, /过期|中断|失效/);
  while (finish === undefined) await new Promise(resolve => setImmediate(resolve));
  f.publishProtocol({ TickBatch: { frames: [{ tick: 2, seq_from: 0, seq_to: 0, facts: [], timeseries_payload: {} }], runtime_snapshot: null } });
  f.fail(prepareRejection);
  finish(json({ generation: "1", receipt: proposalReceipt }));
  await stale;
  assert.equal(f.closed(), 0, "可信 F2 必须保留当前 Publisher 连接");
  assert.equal(f.updates.length, 2, "必须先交 baseline 与已接受末拍");
  assert.equal(f.failures.length, 1, "可恢复拒绝仍须显式通知用户");
  assert.deepEqual(await f.invoke("cancelReportCorrection", correction.operation_id), { operation_id: correction.operation_id });
  f.publishProtocol({ CivilUpdate: { tick: 2 } });
  assert.equal(f.updates.length, 3, "同连接必须继续交付 civil-only 更新");
});

for (const invalid of [
  { code: "CIVIL_DAY_SETTLEMENT_FAILED" }, { where: "server.actor.callback" }, { recoverable: false },
  { context: { ...prepareRejection.context, operation: "callback" } }, { context: { ...prepareRejection.context, generation: "2" } },
  { context: { ...prepareRejection.context, day: 4_294_967_296 } },
  { cause: { code: "REPORT_CORRECTION_REJECTED" } },
  { cause: { ...prepareRejection.cause, unexpected: true } },
  { cause: { ...prepareRejection.cause, message: "  " } },
  { cause: { ...prepareRejection.cause, cause: { code: "INNER" } } },
  { cause: { ...prepareRejection.cause, code: "REPORT_CORRECTION_FAILED" } },
]) test(`Remote 非可信F2仍关闭连接 ${JSON.stringify(invalid)}`, { timeout: 10000 }, async t => {
  const f = await fixture(t, () => { throw new Error("非F2不能继续POST"); });
  f.fail({ ...prepareRejection, ...invalid });
  assert.equal(f.closed(), 1);
  await assert.rejects(f.invoke("cancelReportCorrection", correction.operation_id), /baseline|连接|失效|过期/);
  assert.equal(f.calls.length, 0);
});

test("Remote 首fatal之后迟到的F2不能复活旧连接", { timeout: 10000 }, async t => {
  const f = await fixture(t, () => { throw new Error("首fatal后不能POST"); });
  f.fail();
  f.fail(prepareRejection);
  assert.equal(f.closed(), 1);
  await assert.rejects(f.invoke("cancelReportCorrection", correction.operation_id), /baseline|连接|失效|过期/);
  assert.equal(f.calls.length, 0);
});

test("Remote F2 matcher接纳完整嵌套FailureCause并拒绝循环cause", { timeout: 10000 }, () => {
  const nested = { ...prepareRejection, cause: { ...prepareRejection.cause, cause: { code: "INNER", message: "内部拒绝", cause: null } } };
  assert.equal(isReportCorrectionPrepareRejection(nested, "server", "1"), true);
  assert.equal(isReportCorrectionPrepareRejection({ ...prepareRejection, context: { ...prepareRejection.context, day: 4_294_967_295 } }, "server", "1"), true);
  const cyclicCause: { code: string; message: string; cause: unknown } = { code: "REPORT_CORRECTION_REJECTED", message: "拒绝", cause: null };
  cyclicCause.cause = cyclicCause;
  assert.equal(isReportCorrectionPrepareRejection({ ...prepareRejection, cause: cyclicCause }, "server", "1"), false);
});

test("Remote 配股认购从可信 member 注入身份并验证受理回执", { timeout: 10000 }, async t => {
  const receipt = { event_id: "r1", requested_shares: "5", submitted_on: "2030-06-03" };
  const f = await fixture(t, path => { assert.equal(path, "/api/subscribe-rights-offering"); return json({ generation: "1", receipt }); });
  assert.deepEqual(await f.host.subscribeRightsOffering!("r1", "5"), receipt);
  assert.deepEqual(f.calls[0], { path: "/api/subscribe-rights-offering", body: { session_id: "current-session", generation: "1", event_id: "r1", requested_shares: "5" }, token: "Bearer owner-token" });
});

test("Remote 配股认购无本人身份时不发送命令", { timeout: 10000 }, async t => {
  const f = await fixture(t, () => { throw new Error("不应发送认购"); }, () => json({ ...remoteTestContext("current-session"), member: null }));
  await assert.rejects(f.host.subscribeRightsOffering!("r1", "5"), /本人身份/);
  assert.equal(f.calls.length, 0);
});

for (const classification of ["InvalidInput", "BusinessCondition", "UnsupportedOperation", "SystemState"]) test(`Remote 配股认购保留 ${classification} typed 拒绝`, { timeout: 10000 }, async t => {
  const failure = { kind: "RightsSubscription", data: { account: "1", event_id: "r1", classification, reason: "认购被拒绝" } };
  const f = await fixture(t, () => json({ code: "RIGHTS_SUBSCRIPTION_REJECTED", failure }, 400));
  await assert.rejects(f.host.subscribeRightsOffering!("r1", "5"), error => error instanceof CompanyWriteRejectedError && assert.deepEqual(error.failure, failure) === undefined);
});
for (const data of [{ account: "2", event_id: "r1" }, { account: "1", event_id: "other" }]) test(`Remote 配股认购拒绝响应身份不符 ${JSON.stringify(data)}`, { timeout: 10000 }, async t => {
  const failure = { kind: "RightsSubscription", data: { ...data, classification: "BusinessCondition", reason: "拒绝" } };
  const f = await fixture(t, () => json({ code: "RIGHTS_SUBSCRIPTION_REJECTED", failure }, 400));
  await assert.rejects(f.host.subscribeRightsOffering!("r1", "5"), error => error instanceof AggregateError);
});

test("Remote 配股认购拒绝畸形字段和错误 family", { timeout: 10000 }, async t => {
  const failures = [{ kind: "RightsSubscription", data: { account: "1", event_id: "r1", classification: "unknown", reason: "拒绝" } }, failure];
  const f = await fixture(t, () => json({ code: "RIGHTS_SUBSCRIPTION_REJECTED", failure: failures.shift() }, 400));
  for (let i = 0; i < 2; i++) await assert.rejects(f.host.subscribeRightsOffering!("r1", "5"), error => error instanceof AggregateError);
});
