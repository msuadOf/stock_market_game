import assert from "node:assert/strict";
import test from "node:test";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import path from "node:path";
import vm from "node:vm";

const { companyPanelCompletion } = await import(process.env.N4_GUARD_BASELINE === "1"
  ? "./reference/wasm-company-guard-baseline.mjs" : "./wasm-company-guard.mjs");
const options = { timeout: 10_000 };
// 只提取真实 sourceManifest 的无副作用本体，避免短测导入 driver 时启动浏览器。
const driverSource = await readFile(new URL("./wasm-panic-rerun.mjs", import.meta.url), "utf8");
const sourceStart = driverSource.indexOf("async function sourceManifest() {");
const sourceEnd = driverSource.indexOf("async function driverManifest() {", sourceStart);
assert(sourceStart >= 0 && sourceEnd > sourceStart, "driver 必须保留可独立短测的 sourceManifest");
function manifestFixture() {
  const tracked = ["apps/web/src/App.tsx"];
  const untracked = ["apps/web/src/new-query.ts", "agents/company-system/wasm-company-guard.mjs"];
  const bytes = new Map([...tracked, ...untracked].map((filename) => [path.join("/fixture", filename), Buffer.from(filename)]));
  const collect = vm.runInNewContext(`(${driverSource.slice(sourceStart, sourceEnd).trim()})`, {
    root: "/fixture", path, createHash,
    execFileSync: (_command, args) => [...tracked, ...(args.includes("--others") ? untracked : [])].join("\0") + "\0",
    readFile: async (filename) => { if (!bytes.has(filename)) throw new Error(`fixture 缺少 ${filename}`); return bytes.get(filename); },
  });
  return { collect, untracked, bytes };
}

test("源码 manifest 必须包含必要 untracked 运行源码，不只读取 HEAD tracked", options, async () => {
  const fixture = manifestFixture();
  const files = await fixture.collect();
  assert(files.some((entry) => entry.path === "apps/web/src/new-query.ts"));
  assert(files.some((entry) => entry.path === "agents/company-system/wasm-company-guard.mjs"));
});

test("必要 untracked 内容修改或文件集合增加必须使源码指纹失效", options, async () => {
  const fixture = manifestFixture();
  const before = JSON.stringify(await fixture.collect());
  fixture.bytes.set("/fixture/apps/web/src/new-query.ts", Buffer.from("changed"));
  assert.notEqual(JSON.stringify(await fixture.collect()), before);
  fixture.untracked.push("apps/web/src/another-query.ts");
  fixture.bytes.set("/fixture/apps/web/src/another-query.ts", Buffer.from("new source"));
  assert.equal((await fixture.collect()).length, 4);
});
const baseline = { sequence: 10, generation: 2, workerId: 1 };
const required = [{ type: "companyCapabilities", successType: "companyCapabilities", companyId: "issuer-a", after: 10, minRequests: 1, widgets: ["capabilities"] }];
function sent(requestId = 21, sequence = 11, extra = {}) {
  return { direction: "sent", origin: "ui", workerId: 1, sequence, data: { type: "companyCapabilities", requestId, generation: 2, company: "issuer-a", ...extra } };
}
function reply(requestId = 21, sequence = 12, extra = {}) {
  return { direction: "received", workerId: 1, sequence, data: { type: "companyCapabilities", requestId, generation: 2, capabilities: { issued_shares: "100" }, ...extra } };
}
function sample(trace = [sent(), reply()]) {
  return { baseline: { ...baseline }, generation: 2, workerId: 1, required: structuredClone(required), trace,
    ui: { alerts: [], capabilityRegionVisible: true, widgets: { capabilities: { visible: true, loading: false, dataVisible: true } } } };
}

test("pending 即使 error 已清空且静态能力 region 可见，也不能绿", options, () => {
  assert.equal(companyPanelCompletion(sample([sent()])).status, "pending");
});

test("匹配成功返回但 UI loading 未结束，不能绿", options, () => {
  const input = sample();
  input.ui.widgets.capabilities.loading = true;
  assert.equal(companyPanelCompletion(input).status, "pending");
});

test("匹配成功返回但对应数据不可见，不能绿", options, () => {
  const input = sample();
  input.ui.widgets.capabilities.dataVisible = false;
  assert.equal(companyPanelCompletion(input).status, "pending");
});

test("仅静态 section 可见且没有实际 UI request，不能绿", options, () => {
  assert.equal(companyPanelCompletion(sample([])).status, "pending");
});

test("旧 requestId 的成功返回不能完成新请求", options, () => {
  assert.equal(companyPanelCompletion(sample([sent(), reply(20)])).status, "pending");
});

test("旧 generation 同 requestId 返回不能完成当前请求", options, () => {
  assert.equal(companyPanelCompletion(sample([sent(), reply(21, 12, { generation: 1 })])).status, "pending");
});

test("基线前的 request 即使晚到成功返回也不能充数", options, () => {
  assert.equal(companyPanelCompletion(sample([sent(21, 10), reply()])).status, "pending");
});

test("旧 Worker 的相同 generation/requestId 返回不能充数", options, () => {
  assert.equal(companyPanelCompletion(sample([sent(), { ...reply(), workerId: 0 }])).status, "pending");
});

test("driver 自发查询成功不能替代 UI 消费链", options, () => {
  assert.equal(companyPanelCompletion(sample([{ ...sent(), origin: "driver" }, reply()])).status, "pending");
});

test("请求前已出现的同 requestId 返回不能倒配", options, () => {
  assert.equal(companyPanelCompletion(sample([reply(21, 10), sent()])).status, "pending");
});

test("其他公司的成功查询不能替代当前公司", options, () => {
  assert.equal(companyPanelCompletion(sample([sent(21, 11, { company: "issuer-b" }), reply()])).status, "pending");
});

test("匹配 operationError 必须显式失败并保留完整原文", options, () => {
  const message = "真实 operationError：owner 查询失败，保留上下文";
  const result = companyPanelCompletion(sample([sent(), reply(21, 12, { type: "operationError", message })]));
  assert.equal(result.status, "failed");
  assert(result.failures.some((failure) => failure.message === message));
});

test("operationError 后同请求成功不能抹掉原错误", options, () => {
  const result = companyPanelCompletion(sample([sent(), reply(21, 12, { type: "operationError", message: "原错误" }), reply(21, 13)]));
  assert.equal(result.status, "failed");
  assert(result.failures.some((failure) => failure.message === "原错误"));
});

test("匹配请求收到错误类型不能作为成功", options, () => {
  assert.equal(companyPanelCompletion(sample([sent(), reply(21, 12, { type: "ownerRightsOfferings" })])).status, "failed");
});

test("Worker fatal failure 即使 UI 暂未显示，也显式失败", options, () => {
  const input = sample();
  input.trace.push({ direction: "received", workerId: 1, sequence: 13, data: { type: "failure", generation: 2, code: "WASM_WORKER_PROTOCOL", message: "unreachable" } });
  assert.equal(companyPanelCompletion(input).status, "failed");
});

test("UI alert 不允许被成功 Worker reply 遮蔽", options, () => {
  const input = sample();
  input.ui.alerts.push("消费者校验失败");
  assert.equal(companyPanelCompletion(input).status, "failed");
});

test("旧更正 alert 未在刷新开始清空，本次请求 pending 时应继续等待", options, () => {
  const input = sample([sent()]);
  input.baseline.priorAlerts = ["旧更正查询错误"];
  input.ui.alerts = [...input.baseline.priorAlerts];
  assert.equal(companyPanelCompletion(input).status, "pending");
  input.trace.push(reply());
  assert.equal(companyPanelCompletion(input).status, "pending");
  input.ui.alerts = [];
  assert.equal(companyPanelCompletion(input).status, "passed");
});

test("旧 alert 留存不能掩盖本次匹配 operationError", options, () => {
  const input = sample([sent(), reply(21, 12, { type: "operationError", message: "本次真实拒绝" })]);
  input.baseline.priorAlerts = ["旧错误"];
  input.ui.alerts = ["旧错误"];
  const result = companyPanelCompletion(input);
  assert.equal(result.status, "failed");
  assert(result.failures.some((failure) => failure.message === "本次真实拒绝"));
});

test("刷新期间新增 UI alert 不属于旧错误，必须显式失败", options, () => {
  const input = sample([sent()]);
  input.baseline.priorAlerts = ["旧错误"];
  input.ui.alerts = ["新消费者错误"];
  assert.equal(companyPanelCompletion(input).status, "failed");
});

test("generation 或 Worker 已切换时本次公司阶段失效", options, () => {
  const input = sample();
  input.generation = 3;
  assert.equal(companyPanelCompletion(input).status, "failed");
  input.generation = 2;
  input.workerId = 2;
  assert.equal(companyPanelCompletion(input).status, "failed");
});

test("完整匹配 reply、loading 结束及实际数据可见才绿", options, () => {
  const result = companyPanelCompletion(sample());
  assert.equal(result.status, "passed");
  assert.equal(result.matched.length, 1);
  assert.equal(result.matched[0].requestId, 21);
  assert.equal(result.matched[0].generation, 2);
});

test("两处能力面消费者不能用一个 request 冒充全部完成", options, () => {
  const input = sample();
  input.required[0].minRequests = 2;
  input.required[0].widgets.push("proposal");
  input.ui.widgets.proposal = { visible: true, loading: false, dataVisible: true };
  assert.equal(companyPanelCompletion(input).status, "pending");
  input.trace.push(sent(22, 13), reply(22, 14));
  assert.equal(companyPanelCompletion(input).status, "passed");
});

test("同类最新 request pending 不能被较早的成功返回遮蔽", options, () => {
  assert.equal(companyPanelCompletion(sample([sent(), reply(), sent(22, 13)])).status, "pending");
});

test("税状态成功但串行未清税额查询尚未发出，不能绿", options, () => {
  const input = sample();
  input.required.push({ type: "dividendTaxOutstanding", successType: "dividendTaxOutstanding", after: 10, minRequests: 1, widgets: ["tax"] });
  input.ui.widgets.tax = { visible: true, loading: false, dataVisible: true };
  assert.equal(companyPanelCompletion(input).status, "pending");
  input.trace.push(sent(22, 13, { type: "dividendTaxOutstanding" }), reply(22, 14, { type: "dividendTaxOutstanding", views: [] }));
  assert.equal(companyPanelCompletion(input).status, "passed");
});

test("当前 generation 的公开报告缓存只接受其真实 UI root 请求见证", options, () => {
  const input = sample();
  input.required.push({ type: "publicReports", successType: "publicReports", companyId: "issuer-a", rootPage: true, after: 2, minRequests: 1, widgets: ["reports"] });
  input.ui.widgets.reports = { visible: true, loading: false, dataVisible: true };
  const request = sent(19, 3, { type: "publicReports", query: { company_id: "issuer-a", cursor: null } });
  const response = reply(19, 4, { type: "publicReports", page: { reports: [{ id: "1" }] } });
  input.trace.unshift(request, response);
  assert.equal(companyPanelCompletion(input).status, "passed");
  request.data.query.cursor = "next-page";
  assert.equal(companyPanelCompletion(input).status, "pending");
});
