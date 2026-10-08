import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { once } from "node:events";
import { appendFileSync } from "node:fs";
import { mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { availableParallelism } from "node:os";
import path from "node:path";
import { performance } from "node:perf_hooks";
import { fileURLToPath } from "node:url";
import { companyPanelCompletion } from "./wasm-company-guard.mjs";

// 默认规模旅程只能作为长验收，由 run-long-validation.mjs 300000ms 共享外部 deadline 监督。
if (process.env.STOCK_GAME_DEADLINE_GROUP !== "owned") throw new Error("请通过 run-long-validation.mjs 300000 -- 启动 N4 driver");
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const web = path.join(root, "apps/web");
const args = process.argv.slice(2);
function argument(name, defaultValue) {
  const index = args.indexOf(name);
  if (index === -1) return defaultValue;
  if (args[index + 1] === undefined || args[index + 1].startsWith("--")) throw new Error(`${name} 缺少值`);
  return args[index + 1];
}
const label = argument("--label", `run-${Date.now()}`);
if (!/^[a-z0-9-]+$/.test(label)) throw new Error("--label 只允许小写字母、数字和连字符");
const threads = Number(argument("--threads", "2"));
if (!Number.isSafeInteger(threads) || threads < 1 || threads > availableParallelism()) throw new Error("--threads 超出本机可用 CPU 预算");
const flow = argument("--flow", "default");
if (!["default", "day-end", "trading-day"].includes(flow)) throw new Error("--flow 必须为 default、day-end 或 trading-day");
const port = Number(argument("--port", "4183"));
if (!Number.isInteger(port) || port < 1024 || port > 65535) throw new Error("--port 无效");
const evidenceRoot = path.join(root, ".tmp/company-system/wasm-panic-rerun");
const evidence = path.join(evidenceRoot, label);
await mkdir(evidenceRoot, { recursive: true });
await mkdir(evidence); // 已存在的 label 显式失败，禁止覆写历史证据。
const require = createRequire(path.join(web, "package.json"));
const { chromium, expect } = require("@playwright/test");
async function packageManifest(directory, prefix = "") {
  const entries = await readdir(path.join(directory, prefix), { withFileTypes: true });
  const nested = await Promise.all(entries.map(async (entry) => {
    const name = path.join(prefix, entry.name);
    if (entry.isDirectory()) return packageManifest(directory, name);
    if (!entry.isFile()) throw new Error(`WASM package 含非普通文件：${name}`);
    const bytes = await readFile(path.join(directory, name));
    return [{ path: name, bytes: bytes.length, sha256: createHash("sha256").update(bytes).digest("hex") }];
  }));
  return nested.flat().sort((left, right) => left.path.localeCompare(right.path));
}
async function sourceManifest() {
  const files = execFileSync("git", ["ls-files", "--cached", "--others", "--exclude-standard", "-z", "--", ".cargo", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "pnpm-lock.yaml", "package.json", "apps/web", "apps/web-wasm", "packages/engine", "scripts/frontend-build.mjs", "scripts/frontend-build-plan.mjs", "agents/company-system/wasm-panic-rerun.mjs", "agents/company-system/wasm-company-guard.mjs", "agents/company-system/wasm-company-guard.test.mjs", "agents/company-system/reference/wasm-company-guard-baseline.mjs"], { cwd: root, encoding: "utf8" }).split("\0").filter(Boolean).sort();
  return Promise.all(files.map(async (filename) => ({ path: filename, sha256: createHash("sha256").update(await readFile(path.join(root, filename))).digest("hex") })));
}
async function driverManifest() {
  return Promise.all(["wasm-panic-rerun.mjs", "wasm-company-guard.mjs"].map(async (filename) => ({ path: filename, sha256: createHash("sha256").update(await readFile(path.join(root, "agents/company-system", filename))).digest("hex") })));
}
const sourceFiles = await sourceManifest();
const driverFiles = await driverManifest();
const wasmPackageManifest = await packageManifest(path.join(web, "wasm-pkg"));
const wasm = await readFile(path.join(web, "wasm-pkg/web_wasm_bg.wasm"));
const started = performance.now();
const result = {
  label, flow, threads, seed: "42", cpuBudget: availableParallelism(),
  sourceHead: execFileSync("git", ["rev-parse", "HEAD"], { cwd: root, encoding: "utf8" }).trim(),
  driverSha256: createHash("sha256").update(await readFile(fileURLToPath(import.meta.url))).digest("hex"),
  driverFiles, sourceFiles,
  sourceContentSha256: createHash("sha256").update(JSON.stringify(sourceFiles)).digest("hex"),
  wasmProducerEvidence: "sourceHead 只标识当树；WASM producer 必须另以本树成套构建日志核对，历史 ff26a4e4 包不计当前构建",
  acceptanceScope: flow === "default" ? "默认规模新局、首日终、首交易 tick、F10、非空文件 restore、fixture 规模继续步进" : flow === "day-end" ? "仅默认规模首日终，不覆盖市场 tick 或完整旅程" : "默认规模改为交易日起点的步进对照，不覆盖完整旅程",
  cargoConfigSha256: createHash("sha256").update(await readFile(path.join(root, ".cargo/config.toml"))).digest("hex"),
  wasmSha256: createHash("sha256").update(wasm).digest("hex"),
  wasmPackageManifest,
  wasmPackageSha256: createHash("sha256").update(JSON.stringify(wasmPackageManifest)).digest("hex"),
  stages: [], status: "running",
};
// 同 label 重新运行时明确重建日志，不能混入前一轮证据。
await writeFile(path.join(evidence, "console.jsonl"), "");
await writeFile(path.join(evidence, "result.json"), JSON.stringify(result, null, 2));
const consoles = [];
const errors = [];
const debuggerEvents = [];
let browser;
let page;
let failure;
const serverLog = [];
const server = spawn(process.execPath, [path.join(web, "node_modules/vite/bin/vite.js"), "--mode", "e2e", "--host", "127.0.0.1", "--port", String(port), "--strictPort"], { cwd: web, env: process.env, stdio: ["ignore", "pipe", "pipe"] });
let serverClosed = false;
const serverClose = once(server, "close").then(([code, signal]) => { serverClosed = true; return { code, signal }; });
server.stdout.on("data", (chunk) => serverLog.push(chunk.toString()));
server.stderr.on("data", (chunk) => serverLog.push(chunk.toString()));

function mark(stage, details = {}) {
  const value = { stage, elapsedMs: Math.round(performance.now() - started), ...details };
  result.stages.push(value);
  process.stdout.write(`${JSON.stringify(value)}\n`);
}

// Worker 控制只暂停计时器和发送已有的 E2E stepOnce，默认 setup、seed 与
// NPC 数量均保留。消息摘要不复写请求，failure/operationError 原文完整保留。
function installObserver({ threads }) {
  // 两种产物使用同一个显式初始化 seed；不要求自由并发调度产生相同整局字节。
  const randomValues = Crypto.prototype.getRandomValues;
  Crypto.prototype.getRandomValues = function (array) {
    if (array instanceof Uint32Array && array.length === 2) { array.set([0, 42]); return array; }
    return randomValues.call(this, array);
  };
  Object.defineProperty(navigator, "hardwareConcurrency", { configurable: true, value: threads });
  const NativeWorker = window.Worker;
  const state = { worker: null, workerId: 0, generation: 0, generationSequence: 0, sequence: 0, trace: [], driverRequestIds: new Set(), tick: 0, requestId: 1_000_000_000, readyThreads: null, setup: null, memory: null };
  window.__N4__ = state;
  setInterval(() => {
    if (state.memory !== null) console.debug(`[N4_MEMORY] ${JSON.stringify({ atMs: performance.now(), tick: state.tick, bytes: state.memory.buffer.byteLength })}`);
  }, 5000);
  function summarize(data) {
    const value = { ...data };
    if (value.setup) value.setup = { npcs: value.setup.npcs, stocks: value.setup.stocks.length, ticks_per_day: value.setup.ticks_per_day, start_date: value.setup.start_date, company_system: value.setup.company_system.mode };
    if (value.snapshot) value.snapshot = { tick: value.snapshot.tick, day: value.snapshot.day };
    if (value.slot) value.slot = { tick: value.slot.snapshot.tick, current_date: value.slot.civil_clock.current_date, settled_through: value.slot.civil_clock.settled_through };
    if (value.memory) value.memory = "WebAssembly.Memory";
    if (value.module) value.module = "WebAssembly.Module";
    if (value.update) value.update = summarizeUpdate(value.update);
    if (value.updates) value.updates = value.updates.map(summarizeUpdate);
    return value;
  }
  function summarizeUpdate(update) {
    const kind = Object.keys(update)[0];
    const payload = update[kind];
    return kind === "TickBatch"
      ? { kind, tick: payload.frames.at(-1)?.tick, seq_from: payload.frames[0]?.seq, seq_to: payload.frames.at(-1)?.seq }
      : { kind, tick: payload.tick, seq_from: payload.seq_from, seq_to: payload.seq_to, settled_date: payload.boundary?.settled_date, next_date: payload.boundary?.next_date };
  }
  function record(direction, data, workerId, origin) {
    const event = { direction, workerId, origin, sequence: ++state.sequence, atMs: performance.now(), data: summarize(data) };
    state.trace.push(event);
    console.debug(`[N4_WORKER] ${JSON.stringify(event, (_key, value) => typeof value === "bigint" ? value.toString() : value)}`);
  }
  window.Worker = class extends NativeWorker {
    constructor(url, options) {
      super(url, options);
      if (!String(url).includes("/wasm-worker.ts")) return;
      const workerId = ++state.workerId;
      state.worker = this;
      state.generationSequence = state.sequence;
      state.generation = 0;
      state.tick = 0;
      state.readyThreads = null;
      state.setup = null;
      state.memory = null;
      const post = this.postMessage.bind(this);
      this.postMessage = (data, ...rest) => {
        if (data.type === "init") data = { ...data, threads };
        record("sent", data, workerId, state.driverRequestIds.has(data.requestId) ? "driver" : "ui");
        if (data.type === "create") state.setup = data.setup;
        post(data, ...rest);
        // 保留默认 start 链，仅停止自动 timer 抢跑，随后由 UI 暂停同步展示状态。
        if (data.type === "start") {
          const stop = { type: "stop", generation: data.generation, requestId: ++state.requestId };
          state.driverRequestIds.add(stop.requestId);
          record("sent", stop, workerId, "driver");
          post(stop);
        }
      };
      this.addEventListener("message", ({ data }) => {
        record("received", data, workerId);
        if (workerId !== state.workerId) return;
        if (data.type === "ingress") state.memory = data.memory;
        if (data.type === "ready") state.readyThreads = data.threads;
        if ((data.type === "created" || data.type === "baseline") && data.generation > state.generation) {
          state.generation = data.generation;
          state.generationSequence = state.sequence;
        }
        if (data.type === "restored" && data.nextGeneration > state.generation) {
          state.generation = data.nextGeneration;
          state.generationSequence = state.sequence;
        }
        if (data.type !== "restored" && data.generation !== undefined && data.generation !== state.generation) return;
        if (data.snapshot) state.tick = data.snapshot.tick;
        if (data.type === "stepped") state.tick = data.tick;
      });
    }
  };
  state.request = (type, success, payload = {}, timeoutMs = 10_000) => new Promise((resolve, reject) => {
    if (state.worker === null || state.generation < 1) { reject(new Error("N4 Worker 尚未就绪")); return; }
    const worker = state.worker;
    const requestId = ++state.requestId;
    const requestedGeneration = state.generation;
    state.driverRequestIds.add(requestId);
    const cleanup = () => { clearTimeout(timer); worker.removeEventListener("message", listener); };
    const listener = ({ data }) => {
      if (data.type === "failure") { cleanup(); reject(new Error(JSON.stringify(data))); return; }
      if (data.requestId !== requestId || data.generation !== requestedGeneration) return;
      if (data.type === "operationError") { cleanup(); reject(new Error(JSON.stringify(data))); return; }
      if (data.type !== success) { cleanup(); reject(new Error(`N4 ${type} 响应类型错误：${data.type}`)); return; }
      cleanup(); resolve(data);
    };
    const timer = setTimeout(() => { cleanup(); reject(new Error(`N4 ${type} 超时 ${timeoutMs}ms`)); }, timeoutMs);
    worker.addEventListener("message", listener);
    worker.postMessage({ type, ...payload, requestId, generation: state.generation });
  });
}

async function attachWasmDebugger() {
  const cdp = await browser.newBrowserCDPSession();
  const { targetInfos } = await cdp.send("Target.getTargets");
  const target = targetInfos.find((target) => target.type === "worker" && target.url.includes("/wasm-worker.ts"));
  if (!target) throw new Error("找不到主 WASM Worker 的 CDP target");
  const { sessionId } = await cdp.send("Target.attachToTarget", { targetId: target.targetId, flatten: false });
  let id = 0;
  const pending = new Map();
  function send(method, params = {}) {
    return new Promise((resolve, reject) => {
      const next = ++id;
      pending.set(next, { resolve, reject });
      cdp.send("Target.sendMessageToTarget", { sessionId, message: JSON.stringify({ id: next, method, params }) }).catch(reject);
    });
  }
  cdp.on("Target.receivedMessageFromTarget", ({ sessionId: incomingSession, message }) => {
    if (incomingSession !== sessionId) return;
    const event = JSON.parse(message);
    if (event.id !== undefined) {
      const request = pending.get(event.id);
      if (!request) return;
      pending.delete(event.id);
      if (event.error) request.reject(new Error(JSON.stringify(event.error)));
      else request.resolve(event.result);
    } else if (event.method === "Debugger.paused") {
      debuggerEvents.push(event.params);
      void send("Debugger.resume").catch((error) => errors.push(`CDP resume 失败：${error.stack}`));
    }
  });
  await send("Debugger.enable");
  await send("Debugger.setPauseOnExceptions", { state: "all" });
  return async () => { await send("Debugger.setPauseOnExceptions", { state: "none" }); await cdp.detach(); };
}

async function step(count, timeoutMs = 10_000) {
  const steps = [];
  for (let index = 0; index < count; index += 1) {
    const before = performance.now();
    const response = await page.evaluate(async ({ timeoutMs }) => {
      const state = window.__N4__;
      const sequence = state.sequence;
      const previousTick = state.tick;
      const reply = await state.request("stepOnce", "stepped", {}, timeoutMs);
      const updates = state.trace.filter((event) => event.direction === "received" && event.sequence > sequence && event.workerId === state.workerId
        && event.data.generation === reply.generation && (event.data.type === "protocol" || event.data.type === "protocolBatch"))
        .flatMap((event) => event.data.type === "protocol" ? [event.data.update] : event.data.updates);
      return { ...reply, previousTick, updates };
    }, { timeoutMs });
    steps.push(response);
    mark("step", { index, ...response, durationMs: Math.round(performance.now() - before) });
  }
  return steps;
}

function companyDom() {
  const panel = document.querySelector(".company-panel[data-company-id]");
  const visible = (element) => element !== null && element.getClientRects().length > 0 && getComputedStyle(element).visibility !== "hidden";
  const text = (element) => element === null ? "" : element.innerText;
  const select = (selector) => panel === null ? null : panel.querySelector(selector);
  const widget = (element, dataVisible) => ({ visible: visible(element), dataVisible,
    loading: element !== null && (/正在查询|正在读取|正在请求|正在加载/.test(text(element)) || Array.from(element.querySelectorAll("button")).some((button) => /^(刷新|重新读取)/.test(button.textContent) && button.disabled)) });
  const tableWith = (element, title) => element !== null && Array.from(element.querySelectorAll("table")).some((table) => text(table.querySelector("thead")).includes(title) && table.querySelectorAll("tbody tr").length > 0);
  const reports = select(".company-report");
  const tax = select(".company-dividend-tax");
  const rightsRejections = select(".company-rights-rejections");
  const preferenceRejections = select(".company-preference-rejections");
  const contract = select(".company-contract");
  const facts = select(".company-contract-facts");
  const proposal = select(".player-proposal-readiness");
  const preferences = select(".in-game-preferences");
  const corrections = select(".company-correction-control");
  return {
    companyId: panel?.getAttribute("data-company-id"), alerts: panel === null ? [] : Array.from(panel.querySelectorAll('[role="alert"]')).map((element) => element.textContent),
    capabilityRegionVisible: visible(contract), correctionsEnabled: corrections !== null,
    widgets: {
      reports: widget(panel, visible(reports) && reports.querySelectorAll('[role="listitem"]').length > 0 && /公开编号 \d+/.test(text(reports))),
      tax: widget(tax, visible(tax) && /本局税务模式：/.test(text(tax)) && (/没有未划收的股息税额/.test(text(tax)) || tableWith(tax, "未划收税额"))),
      rightsRejections: widget(rightsRejections, /本人没有配股认购拒绝回执/.test(text(rightsRejections)) || tableWith(rightsRejections, "拒绝原因")),
      preferenceRejections: widget(preferenceRejections, /该公司没有偏好提案拒绝记录/.test(text(preferenceRejections)) || tableWith(preferenceRejections, "拒绝原因")),
      capabilities: widget(contract, visible(facts) && /现行总股本/.test(text(facts)) && facts.querySelector("strong.num")?.textContent.length > 0),
      proposal: widget(proposal, /前置条件满足|当前不满足：/.test(text(proposal))),
      rights: widget(contract, /本人当前没有未完成的配股方案/.test(text(contract)) || tableWith(contract, "本人权利/额度")),
      flatReceipts: widget(contract, /本人暂无简税代扣回执/.test(text(contract)) || tableWith(contract, "付款批次")),
      preferences: widget(preferences, preferences !== null && preferences.querySelectorAll("fieldset.company-preference-group").length === 2),
      corrections: widget(corrections, visible(corrections) && !/尚未取得权威更正状态/.test(text(corrections)) && /已完成（日终实际公开结果）/.test(text(corrections))),
    },
  };
}

async function companyPanel() {
  const opening = await page.evaluate(() => ({ sequence: window.__N4__.sequence, generation: window.__N4__.generation, workerId: window.__N4__.workerId, generationSequence: window.__N4__.generationSequence }));
  await page.getByRole("navigation", { name: "桌面主导航" }).getByRole("button", { name: "个股", exact: true }).click();
  await page.getByRole("button", { name: "公司资料 F10", exact: true }).click();
  const panel = page.locator(".company-panel[data-company-id]");
  await expect(panel).toBeVisible({ timeout: 10_000 });
  const beforeRefresh = await page.evaluate(() => window.__N4CompanyDom__());
  const attempt = { opening, beforeRefresh, status: "pending" };
  if (result.companyAttempts === undefined) result.companyAttempts = [];
  result.companyAttempts.push(attempt);
  mark("company-panel-before-refresh", { opening, alerts: beforeRefresh.alerts });
  if (beforeRefresh.alerts.length > 0) await page.screenshot({ path: path.join(evidence, `company-before-refresh-${result.companyAttempts.length}.png`) });
  const corrections = panel.locator(".company-correction-control");
  if (beforeRefresh.correctionsEnabled && !await corrections.evaluate((element) => element.open)) await corrections.locator("summary").click();
  for (const button of await panel.getByRole("button", { name: /^(刷新|重新读取当前偏好)/ }).all()) await expect(button).toBeEnabled({ timeout: 10_000 });
  const baseline = await page.evaluate(() => ({ sequence: window.__N4__.sequence, generation: window.__N4__.generation, workerId: window.__N4__.workerId, priorAlerts: window.__N4CompanyDom__().alerts }));
  assert.equal(baseline.generation, opening.generation, "打开公司面板时 generation 不得改变");
  assert.equal(baseline.workerId, opening.workerId, "打开公司面板时 Worker 不得改变");
  const query = (type, widgets, companyScoped = false, successType = type, minRequests = 1) => ({ type, successType, widgets, minRequests, after: baseline.sequence, ...(companyScoped ? { companyId: beforeRefresh.companyId } : {}) });
  const required = [
    { ...query("publicReports", ["reports"], true), after: opening.generationSequence, rootPage: true, source: "当前 generation 的真实 UI root 查询缓存" },
    query("dividendTaxStatus", ["tax"]), query("dividendTaxOutstanding", ["tax"]),
    query("rightsRejections", ["rightsRejections"]), query("preferenceRejections", ["preferenceRejections"], true),
    query("companyCapabilities", ["capabilities", "proposal"], true, "companyCapabilities", 2),
    query("ownerRightsOfferings", ["rights"]), query("flatWithholdingReceipts", ["flatReceipts"]),
    query("companySimplePreferences", ["preferences"], true),
  ];
  if (beforeRefresh.correctionsEnabled) required.push(query("queryReportCorrections", ["corrections"], false, "reportCorrections"));
  attempt.baseline = baseline;
  attempt.required = required;
  attempt.optional = { queryReportCorrections: beforeRefresh.correctionsEnabled ? "市场控制能力已启用，列为必需" : "当前 UI 无市场控制能力，未查询", publicReportAvailability: "未填写期间，不触发", periodExplanation: "未填写期间，不触发", publicReportById: "当前报告列表已携带数据，不触发", writeOperations: "本次只读 F10，不提交提案、认购、偏好修改或财报更正" };
  // 面板常驻且会预先查询，所以每次都由真实刷新按钮建立 owner 查询的新 request 基线。
  for (const button of await panel.getByRole("button", { name: /^(刷新|重新读取当前偏好)/ }).all()) await button.click();
  try {
    await page.waitForFunction(({ baseline, required }) => {
      const state = window.__N4__;
      const ui = window.__N4CompanyDom__();
      const completion = window.__N4CompanyComplete__({ baseline, required, generation: state.generation, workerId: state.workerId, trace: state.trace, ui });
      window.__N4CompanyLast__ = { ui, completion };
      return completion.status !== "pending";
    }, { baseline, required }, { timeout: 10_000 });
  } catch (error) {
    attempt.observed = await page.evaluate(() => window.__N4CompanyLast__);
    attempt.status = "failed";
    throw new Error(`F10 本次 UI 查询未完成：${JSON.stringify(attempt.observed)}`, { cause: error });
  }
  attempt.observed = await page.evaluate(() => window.__N4CompanyLast__);
  attempt.status = attempt.observed.completion.status;
  assert.equal(attempt.status, "passed", `F10 查询失败：${JSON.stringify(attempt.observed)}`);
  await expect(panel.getByRole("list", { name: "公开报告列表" })).toBeVisible({ timeout: 10_000 });
  mark("company-panel", { status: "passed", baseline, matched: attempt.observed.completion.matched, priorAlerts: beforeRefresh.alerts, refreshedThroughUi: true });
}

try {
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => { cleanup(); reject(new Error("Vite 启动超过 10000ms")); }, 10_000);
    const ready = (chunk) => { if (/http:\/\/127\.0\.0\.1:/.test(chunk.toString())) { cleanup(); resolve(); } };
    const exited = (code) => { cleanup(); reject(new Error(`Vite 提前退出 ${code}`)); };
    const cleanup = () => { clearTimeout(timer); server.stdout.off("data", ready); server.off("exit", exited); };
    server.stdout.on("data", ready);
    server.once("exit", exited);
    server.once("error", reject);
  });
  browser = await chromium.launch({ headless: true });
  page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  page.setDefaultTimeout(10_000);
  page.on("console", (message) => {
    const value = { type: message.type(), text: message.text(), location: message.location() };
    consoles.push(value);
    // 逐条落盘，进程外 deadline 截断时仍保留 Worker 消息与错误证据。
    appendFileSync(path.join(evidence, "console.jsonl"), `${JSON.stringify(value)}\n`);
  });
  page.on("pageerror", (error) => errors.push(error.stack));
  await page.addInitScript(installObserver, { threads });
  await page.addInitScript({ content: `window.__N4CompanyComplete__ = ${companyPanelCompletion.toString()}; window.__N4CompanyDom__ = ${companyDom.toString()};` });
  // mode=e2e 仅自动选择本地宿主；不传 tradingE2E=1，因此仍创建真正 DEFAULT_SETUP。
  await page.goto(`http://127.0.0.1:${port}/`);
  await page.waitForFunction(() => document.querySelector(".app-root, .app-error") !== null, undefined, { timeout: 120_000 });
  if (await page.locator(".app-error").count() > 0) throw new Error(`初始化显式失败：${await page.locator(".app-error").innerText()}`);
  await expect(page.locator(".app-root")).toBeVisible({ timeout: 10_000 });
  await page.getByRole("button", { name: "暂停", exact: true }).click();
  const initial = await page.evaluate(() => ({ tick: window.__N4__.tick, generation: window.__N4__.generation, threads: window.__N4__.readyThreads, npcs: window.__N4__.setup.npcs, stocks: window.__N4__.setup.stocks.length, companySystem: window.__N4__.setup.company_system.mode }));
  assert.equal(initial.npcs.retail_count, 20_000, "默认新局不得被零 NPC fixture 替代");
  assert.equal(initial.npcs.inst_count, 5);
  assert.equal(initial.npcs.hot_count, 2);
  assert.equal(initial.threads, threads);
  mark("default-created", initial);
  if (flow === "trading-day") {
    // 独立对照：通过真实新游戏 UI 明确改日期，不偷换原始 DEFAULT_SETUP。
    await page.getByRole("button", { name: "游戏与存档", exact: true }).click();
    await page.getByLabel(/^模拟起始日期/).fill("2030-01-02");
    await page.getByRole("button", { name: "创建新游戏", exact: true }).click();
    await page.waitForFunction(() => window.__N4__.setup?.start_date === "2030-01-02" && window.__N4__.generation === 1, undefined, { timeout: 60_000 });
    await expect(page.locator(".app-root")).toBeVisible({ timeout: 60_000 });
    await page.getByRole("button", { name: "暂停", exact: true }).click();
    mark("trading-day-created", await page.evaluate(() => ({ startDate: window.__N4__.setup.start_date, tick: window.__N4__.tick, threads: window.__N4__.readyThreads, npcs: window.__N4__.setup.npcs })));
  }
  const detachDebugger = await attachWasmDebugger();
  // 默认 2030-01-01 是模拟休市日，必须把日结与后续真实交易 tick 分开验收。
  if (flow === "trading-day") await step(6, 240_000);
  else {
    const [dayEnd] = await step(1, 240_000);
    assert.equal(dayEnd.previousTick, 0);
    assert.equal(dayEnd.tick, 0, "休市日结不得伪造市场 tick");
    assert(dayEnd.updates.some((update) => update.kind === "CivilUpdate" && update.settled_date === "2030-01-01" && update.next_date === "2030-01-02"), "首日终必须交付真实 CivilUpdate");
    mark("default-first-day-end", dayEnd);
    if (flow === "default") {
      const [firstTick] = await step(1, 240_000);
      assert.equal(firstTick.tick, 1, "必须完成默认规模首个市场 tick，不能只验日终");
      assert(firstTick.updates.some((update) => update.kind === "TickBatch" && update.tick === 1));
      mark("default-first-trading-tick", firstTick);
      await step(4);
    }
  }
  if (flow === "default") {
    await companyPanel();
    const filename = path.join(web, "src/save/fixtures/current-schema-save.json");
    const text = await readFile(filename, "utf8");
    const saved = JSON.parse(text);
    assert(saved.snapshot.tick > 0 && saved.setup.stocks.length > 0 && saved.civil_clock.settled_through !== null, "必须导入真实非空日终档");
    await page.evaluate((text) => {
      const file = new File([text], "current-schema-save.json", { type: "application/json" });
      window.showOpenFilePicker = async () => [{ kind: "file", name: file.name, getFile: async () => file }];
    }, text);
    await page.getByRole("button", { name: "游戏与存档", exact: true }).click();
    await page.getByRole("button", { name: "从文件读取", exact: true }).click();
    await expect(page.getByRole("status").filter({ hasText: "已从文件读档" })).toBeVisible({ timeout: 10_000 });
    const restored = await page.evaluate(() => ({ tick: window.__N4__.tick, generation: window.__N4__.generation }));
    assert.equal(restored.tick, saved.snapshot.tick);
    assert(restored.generation > initial.generation);
    const fixtureNpcCount = saved.setup.npcs.retail_count + saved.setup.npcs.inst_count + saved.setup.npcs.hot_count;
    assert(fixtureNpcCount > 0, "restore fixture 不得是零 NPC 空档");
    mark("file-restored", { ...restored, fixture: filename, fixtureSha256: createHash("sha256").update(text).digest("hex"), fixtureBytes: Buffer.byteLength(text), fixtureNpcCount, scope: "导入后为 fixture 规模，不冒充默认 20007 NPC 性能证据" });
    const continued = await step(6);
    assert(continued.every((response) => response.generation === restored.generation), "继续步进须属于 restore 后 generation");
    assert(continued.at(-1).tick > saved.snapshot.tick, "restore 后必须实际推进市场 tick");
    mark("post-restore-continued", { generation: restored.generation, tick: continued.at(-1).tick, fixtureNpcCount });
    await companyPanel();
  }
  await detachDebugger();
  await expect(page.locator(".app-error")).toHaveCount(0);
  const consoleErrors = consoles.filter((message) => message.type === "error");
  const workerFailures = consoles.filter((message) => message.text.startsWith("[N4_WORKER]") && /"type":"failure"/.test(message.text));
  const operationErrors = consoles.filter((message) => message.text.startsWith("[N4_WORKER]") && /"type":"operationError"/.test(message.text));
  assert.deepEqual(errors, [], "浏览器未捕获异常必须为零");
  assert.deepEqual(consoleErrors, [], "console error 必须为零");
  assert.deepEqual(workerFailures, [], "Worker fatal failure 必须为零");
  const wasmTraps = debuggerEvents.filter((event) => /RuntimeError: (unreachable|memory access out of bounds|table index is out of bounds)/.test(event.data?.description));
  assert.deepEqual(wasmTraps, [], "CDP 捕获的 WASM trap 必须为零，日终迟延不等价 trap");
  result.operationErrors = operationErrors;
  result.debuggerExceptionCount = debuggerEvents.length;
  result.status = "passed";
  mark("passed", { consoleErrors: 0, workerFatalFailures: 0, pageErrors: 0, operationErrors: operationErrors.length });
} catch (error) {
  failure = error;
  result.status = "failed";
  result.error = error.stack;
  process.stderr.write(`${error.stack}\n`);
} finally {
  if (page !== undefined && !page.isClosed()) {
    try { await page.screenshot({ path: path.join(evidence, "final.png") }); }
    catch (error) { errors.push(`截图失败：${error.stack}`); }
    try { await writeFile(path.join(evidence, "page.txt"), await page.locator("body").innerText({ timeout: 3000 })); }
    catch (error) { errors.push(`页面文本采集失败：${error.stack}`); }
  }
  if (browser !== undefined) await browser.close();
  if (!serverClosed) server.kill("SIGTERM");
  const closed = await serverClose;
  result.serverExit = closed;
  const finalDriverSha256 = createHash("sha256").update(await readFile(fileURLToPath(import.meta.url))).digest("hex");
  const finalPackageSha256 = createHash("sha256").update(JSON.stringify(await packageManifest(path.join(web, "wasm-pkg")))).digest("hex");
  result.driverStable = finalDriverSha256 === result.driverSha256 && JSON.stringify(await driverManifest()) === JSON.stringify(result.driverFiles);
  result.sourceStable = createHash("sha256").update(JSON.stringify(await sourceManifest())).digest("hex") === result.sourceContentSha256
    && execFileSync("git", ["rev-parse", "HEAD"], { cwd: root, encoding: "utf8" }).trim() === result.sourceHead;
  result.wasmPackageStable = finalPackageSha256 === result.wasmPackageSha256;
  result.operationErrors = consoles.filter((message) => message.text.startsWith("[N4_WORKER]") && /"type":"operationError"/.test(message.text));
  if (!result.driverStable || !result.sourceStable || !result.wasmPackageStable || errors.length > 0) {
    result.status = "failed";
    const detail = `证据失败：driverStable=${result.driverStable}、sourceStable=${result.sourceStable}、wasmPackageStable=${result.wasmPackageStable}、errors=${JSON.stringify(errors)}`;
    result.evidenceError = detail;
    failure = new Error(detail);
    process.stderr.write(`${detail}\n`);
  }
  result.totalMs = Math.round(performance.now() - started);
  if (errors.length > 0) {
    result.status = "failed";
    result.evidenceErrors = errors;
    if (failure === undefined) failure = new Error("浏览器异常或证据收集失败，详见 page-errors.json");
  }
  await Promise.all([
    writeFile(path.join(evidence, "console.json"), JSON.stringify(consoles, null, 2)),
    writeFile(path.join(evidence, "page-errors.json"), JSON.stringify(errors, null, 2)),
    writeFile(path.join(evidence, "debugger-exceptions.json"), JSON.stringify(debuggerEvents, null, 2)),
    writeFile(path.join(evidence, "vite.log"), serverLog.join("")),
    writeFile(path.join(evidence, "result.json"), JSON.stringify(result, null, 2)),
  ]);
  process.stdout.write(`${JSON.stringify({ ...result, evidence })}\n`);
}
if (failure !== undefined) process.exitCode = 1;
