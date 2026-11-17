#!/usr/bin/env node
// 在进程树监督器内运行：真实 Native HTTP/WS 与真实 Browser Worker 的对照。
// Server 使用 host-parity 单步控制；浏览器只用 E2E JS 单步，加载正式共享内存 WASM。
import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { availableParallelism, tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { verifySharedMemoryArtifact } from "./check-web-release-wasm.mjs";
import { instrumentWindow } from "./performance/libos-performance-window.mjs";
import { executeScenario, fixtureFacts, makeSetup, SCENARIO } from "./fixtures/libos-product-scenario.mjs";
import { observeAuthorityPort } from "./fixtures/session-authority-happy-scenario.mjs";

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const measurementModule = path.join(root, "scripts/performance/libos-performance-window.mjs");
const fixture = path.join(root, "scripts/fixtures/libos-product-scenario.mjs");
const timeoutMs = 10_000;
const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const hash = (input) => createHash("sha256").update(input).digest("hex");
const fixedAcceptedFacts = Object.freeze({
  state: "unsupported",
  reason: "生产 HTTP/WS 和 Worker 只提供已发布 EventFact，未提供完整 World 受理收据的捕获与按原序重放入口",
});

function canonical(value) {
  if (Array.isArray(value)) return value.map(canonical);
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort().map((key) => [key, canonical(value[key])]));
  }
  return value;
}
const canonicalJson = (value) => JSON.stringify(canonical(value));

async function waitFor(get, description) {
  const deadline = performance.now() + timeoutMs;
  while (performance.now() < deadline) {
    const value = get();
    if (value !== undefined) return value;
    await delay(10);
  }
  throw new Error(`等待超时：${description}`);
}

function readArgs(argv) {
  assert.ok(argv.length >= 2 && argv.length % 2 === 0,
    "用法：--output <新的绝对 JSON 路径> [--mode parity|native|browser] [--contract strict-diagnostic|free-restart|fixed-accepted-facts] [--server-bin <绝对路径>] [--browser <绝对路径>]");
  const args = new Map();
  for (let i = 0; i < argv.length; i += 2) {
    assert.ok(["--mode", "--contract", "--server-bin", "--output", "--browser"].includes(argv[i]) && !args.has(argv[i]),
      "未知或重复参数：" + argv[i]);
    args.set(argv[i], argv[i + 1]);
  }
  const mode = args.get("--mode") ?? "parity";
  const contract = args.get("--contract") ?? "strict-diagnostic";
  assert.ok(["parity", "native", "browser"].includes(mode), "未知验收模式");
  assert.ok(["strict-diagnostic", "free-restart", "fixed-accepted-facts"].includes(contract), "未知验收契约");
  const output = args.get("--output"), serverBin = args.get("--server-bin"), browserExecutable = args.get("--browser");
  assert.ok(output !== undefined && path.isAbsolute(output) &&
    (contract === "fixed-accepted-facts" || mode === "browser" || (serverBin !== undefined && path.isAbsolute(serverBin))) &&
    (browserExecutable === undefined || path.isAbsolute(browserExecutable)),
    "制品与报告路径应为绝对路径");
  assert.equal(process.platform, "linux", "Native 线程观测器当前仅支持 Linux /proc");
  assert.ok(availableParallelism() >= 2, "验收至少需要两个 CPU");
  return { mode, contract, serverBin, browserExecutable, output };
}

function freeRestartContract({ checkpoint, freeRestart: { uninterrupted, restarted } }) {
  const observe = ({ closedDays, monday }) => ({ closedDays, monday });
  const expected = observe(uninterrupted);
  assert.deepEqual(observe(restarted), expected, "自由恢复没有通过同一个市场日历和集合竞价协议");
  return {
    completedTradingTick: checkpoint.snapshot.tick,
    settledThrough: checkpoint.civil_clock.settled_through,
    closedDays: expected.closedDays,
    monday: expected.monday,
  };
}

const now = () => Number(process.hrtime.bigint());
let linuxClockTick;
async function nativeResources(serverPid) {
  linuxClockTick ??= Number(execFileSync("getconf", ["CLK_TCK"], { encoding: "utf8" }).trim());
  assert.ok(Number.isSafeInteger(linuxClockTick) && linuxClockTick > 0, "Linux CLK_TCK 无效");
  const [stat, status] = await Promise.all([
    readFile("/proc/" + serverPid + "/stat", "utf8"),
    readFile("/proc/" + serverPid + "/status", "utf8"),
  ]);
  // comm 可含括号；状态后的数字依 Linux stat 字段编号读取。
  const fields = stat.slice(stat.lastIndexOf(")") + 2).trim().split(/\s+/u);
  const rssKib = Number(status.match(/^VmRSS:\s+(\d+) kB$/mu)?.[1]);
  const cpuNs = Math.round((Number(fields[11]) + Number(fields[12])) * 1e9 / linuxClockTick);
  assert.ok(Number.isSafeInteger(cpuNs) && cpuNs >= 0 && Number.isSafeInteger(rssKib) && rssKib > 0,
    "Native CPU/RSS 观测数据缺失");
  return { cpuNs, memory: 1024 * rssKib };
}

async function native(base, serverPid, setup, { startupAt, warmupTicks, expectedGroups }) {
  let socket;
  let token;
  let session;
  let generation;
  let stockCatalog, metrics, rssTimer, rssSample, scenarioError;
  async function request(route, body, method = body === undefined ? "GET" : "POST") {
    const headers = { accept: "application/json" };
    if (body !== undefined) headers["content-type"] = "application/json";
    if (token !== undefined) headers.authorization = `Bearer ${token}`;
    const response = await fetch(new URL(route, base), {
      method, headers, body: body === undefined ? undefined : JSON.stringify(body),
      signal: AbortSignal.timeout(timeoutMs),
    });
    const payload = await response.text();
    assert.ok(response.ok, `Native ${method} ${route} HTTP ${response.status}：${payload}`);
    return payload === "" ? undefined : JSON.parse(payload);
  }
  async function openSocket() {
    const url = new URL("/ws", base);
    url.protocol = "ws:";
    url.searchParams.set("session_id", session);
    url.searchParams.set("delivery", "push");
    const connection = new WebSocket(url, ["stock-game", `stock-game.auth.${Buffer.from(token).toString("hex")}`]);
    socket = connection;
    const queue = [];
    let failure;
    connection.addEventListener("message", ({ data }) => {
      try { queue.push(JSON.parse(data)); }
      catch (error) { failure = new Error("Native WS 消息不是 JSON", { cause: error }); }
    });
    connection.addEventListener("error", () => { failure = new Error("Native WS 连接失败"); });
    connection.addEventListener("close", () => { failure = new Error("Native WS 在预期帧之前关闭"); });
    const take = (variant) => waitFor(() => {
      const frame = queue.shift();
      if (frame !== undefined) {
        assert.ok(Object.hasOwn(frame, variant), `Native WS 预期 ${variant}：${JSON.stringify(frame)}`);
        return frame[variant];
      }
      if (failure !== undefined) throw failure;
      return undefined;
    }, `Native WS ${variant}`);
    const baseline = await take("Baseline");
    assert.equal(String(baseline.timeline_generation), generation);
    stockCatalog = baseline.stock_catalog;
    assert.deepEqual(stockCatalog.map((entry) => entry.code).sort(), [...SCENARIO.codes].sort());
    return { take, baseline };
  }
  const send = (route, payload) => request(route, { session_id: session, generation, ...payload });
  const observeTopology = async () => {
    const taskPath = `/proc/${serverPid}/task`;
    const names = [];
    for (const id of await readdir(taskPath)) {
      try {
        names.push((await readFile(path.join(taskPath, id, "comm"), "utf8")).trim());
      } catch (error) {
        // 退出中的非业务线程可能在读取目录后消失；下面依然要求全部目标 worker。
        if (error.code !== "ENOENT") throw error;
      }
    }
    const groups = names.filter((name) => /^rt-group-\d+$/.test(name));
    const desks = names.filter((name) => /^rt-desk-\d+$/.test(name));
    assert.deepEqual(groups.sort(), Array.from({ length: expectedGroups }, (_, i) => "rt-group-" + i), "原生生产入口实际 NPC Group 拓扑不同");
    assert.deepEqual(desks.sort(), ["rt-desk-0", "rt-desk-1"], "未观察到真实的原生双 Desk worker");
    return { groups, desks };
  };
  try {
    ({ token } = await request("/api/auth/guest", {}));
    assert.equal(typeof token, "string", "Native 游客身份未返回 token");
    const created = await request("/api/new", { setup, seed: SCENARIO.seed });
    session = created.session_id;
    generation = created.generation;
    assert.equal(created.can_control, true, "原生 Server 必须绑定真实本地测试账户");
    assert.equal(created.member?.account_id, "0", "两端必须使用同一个引擎玩家账户");
    let wire = await openSocket();
    let date = wire.baseline.civil_date;
    async function expectUpdate(expected) {
      const frame = await wire.take("PublisherFrame");
      assert.equal(String(frame.timeline_generation), generation);
      const witness = fixtureFacts(frame.update), reply = fixtureFacts(expected);
      assert.equal(witness.to, reply.to, "Native HTTP 已提交游标尚未被 WS 发布");
      assert.equal(witness.tick, reply.tick);
      assert.equal(witness.kind, reply.kind);
      // WS 可以压缩 PriceTick，账务、成交、撤单及日界事实不得因此丢失。
      const authoritativeFacts = ({ facts }) => facts.filter(({ event }) => !Object.hasOwn(event, "PriceTick"));
      assert.deepEqual(authoritativeFacts(witness), authoritativeFacts(reply), "Native WS 发布事实与 HTTP commit 回执不一致");
      date = frame.civil_date;
      return expected;
    }
    const host = {
      async state() {
        const snapshot = await request(`/api/snapshot?session_id=${encodeURIComponent(session)}`);
        return {
          ...snapshot, civilDate: date,
          markets: Object.fromEntries(stockCatalog.map(({ id, code }) => [code, snapshot.markets[id]])),
        };
      },
      submit(intent) {
        const [kind] = Object.keys(intent);
        const value = intent[kind];
        const stock = stockCatalog.find(({ code }) => code === value.code);
        assert.ok(stock, `Native StockCatalog 不存在 ${value.code}`);
        return send("/api/intent", { intent: { [kind]: { ...value, code: stock.id } } });
      },
      async step() { return expectUpdate(await send("/api/host-parity/step", {})); },
      async civil() { return expectUpdate({ CivilUpdate: await send("/api/host-parity/advance-civil-day", {}) }); },
      save({ seq, settledDate }) {
        return send("/api/save", { candidate: { seq, settledDate } });
      },
      async restore(slot) {
        await observeTopology();
        socket.close();
        await request("/api/load", { session_id: session, slot });
        const next = await request(`/api/market/context?session_id=${encodeURIComponent(session)}`);
        assert.equal(BigInt(next.generation), BigInt(generation) + 1n, "Native 读档没有接管新世代");
        generation = next.generation;
        wire = await openSocket();
        date = wire.baseline.civil_date;
      },
    };
    let measurement;
    if (warmupTicks !== undefined) {
      await host.state();
      const startupNs = now() - startupAt;
      let start, maxRss = 0, sampleFailure;
      const sample = async () => {
        const record = await nativeResources(serverPid);
        maxRss = Math.max(maxRss, record.memory);
        return record;
      };
      measurement = instrumentWindow(host, {
        warmupTicks, measuredTicks: SCENARIO.ticksPerDay - warmupTicks, now,
        async start() {
          start = await sample();
          rssTimer = setInterval(() => {
            if (rssSample === undefined) rssSample = sample().catch((error) => { sampleFailure = error; })
              .finally(() => { rssSample = undefined; });
          }, 10);
        },
        async end() {
          clearInterval(rssTimer);
          await rssSample;
          if (sampleFailure !== undefined) throw sampleFailure;
          const final = await sample();
          return { cpuNs: final.cpuNs - start.cpuNs, memoryPeak: maxRss };
        },
      });
      metrics = { startupNs };
    }
    const observedAuthority = observeAuthorityPort(measurement?.host ?? host);
    const results = await executeScenario(observedAuthority.host, expectedGroups);
    if (measurement !== undefined) Object.assign(metrics, measurement.results());
    const { groups, desks } = await observeTopology();
    return { ...results, authorityLifecycle: observedAuthority.evidence(), groups: groups.length, desks: desks.length, path: "server-http-ws", metrics };
  } catch (error) {
    scenarioError = error;
    throw error;
  } finally {
    clearInterval(rssTimer);
    await rssSample;
    const shutdownAt = now();
    socket?.close();
    if (session !== undefined && generation !== undefined) {
      try {
        await request(`/api/session?session_id=${encodeURIComponent(session)}&generation=${encodeURIComponent(generation)}`, undefined, "DELETE");
        if (metrics !== undefined) metrics.shutdownNs = now() - shutdownAt;
      } catch (cleanupError) {
        if (scenarioError !== undefined) throw new AggregateError([scenarioError, cleanupError], "Native 对照和会话回收均失败");
        throw cleanupError;
      }
    }
  }
}

async function browserWorker(page, setup, { warmupTicks, expectedGroups, browser }) {
  let cdp;
  try {
    if (warmupTicks !== undefined) {
      cdp = await browser.newBrowserCDPSession();
      let start;
      const sample = async () => {
        const { processInfo } = await cdp.send("SystemInfo.getProcessInfo");
        const processes = new Map(processInfo.map(({ id, type, cpuTime }) => [id, { type, cpuTime }]));
        assert.ok(processes.size === processInfo.length && processes.size > 1 &&
          processInfo.some(({ type }) => type === "renderer") &&
          processInfo.every(({ cpuTime }) => Number.isFinite(cpuTime) && cpuTime >= 0), "Chromium 进程 CPU 观测不完整");
        return processes;
      };
      await page.exposeFunction("__libosMeasurementStart", async () => { start = await sample(); });
      await page.exposeFunction("__libosMeasurementEnd", async () => {
        const end = await sample();
        assert.deepEqual([...start.keys()].sort(), [...end.keys()].sort(), "测量窗内 Chromium 进程有进出，无法完整归因 CPU");
        let cpuSeconds = 0;
        for (const [id, { type, cpuTime }] of end) {
          const previous = start.get(id);
          assert.ok(previous.type === type && cpuTime >= previous.cpuTime, "Chromium 进程 CPU 计数不可归因");
          cpuSeconds += cpuTime - previous.cpuTime;
        }
        return { cpuNs: Math.round(cpuSeconds * 1e9) };
      });
    }
    return await page.evaluate(async ({ setup, seed, fixtureModule, measurementModule, warmupTicks, expectedGroups }) => {
      if (!crossOriginIsolated || location.search !== "") throw new Error("真实 Worker 必须启用隔离且不携带 runtime override");
      const { demand, executeScenario, fixtureFacts, SCENARIO } = await import(fixtureModule);
      const { observeAuthorityPort } = await import(fixtureModule.replace("libos-product-scenario.mjs", "session-authority-happy-scenario.mjs"));
      const { createWorkerHost } = await import("/src/host/worker-host.ts");
      const { normalizeSerdeValue } = await import("/src/host/protocol/guards.ts");
      let failure;
      let baseline;
      const queue = [];
      const spawnNames = new Set();
      let finalRelease;
      const now = () => Math.round(performance.now() * 1e6);
      const { instrumentWindow } = warmupTicks === undefined ? {} : await import(measurementModule);
      const startupAt = now();
      const host = await createWorkerHost(setup, BigInt(seed), {
        ...(expectedGroups === 2 ? { e2eRuntimeGroups: 2 } : {}),
        enableE2EStepping: true,
        lifecycleObserver(observation) {
          if (observation.type === "childSpawn") spawnNames.add(observation.runtimeName);
          if (observation.type === "finalRelease") finalRelease = observation;
        },
        onSpawnFailure(value) { failure = value; },
      });
      const observeTopology = () => {
        const groups = [...spawnNames].filter((name) => /^rt-group-\d+$/.test(name)).sort();
        const desks = [...spawnNames].filter((name) => /^rt-desk-\d+$/.test(name)).sort();
        demand(groups.length === expectedGroups && groups.every((name, i) => name === "rt-group-" + i) &&
          desks.join(",") === "rt-desk-0,rt-desk-1", "Browser 实际 Group/Desk 拓扑不符：" + [...spawnNames]);
        return { groups, desks };
      };
      let result, metrics, groups, desks, scenarioError, authorityLifecycle;
      try {
        await host.start((update) => {
          if (update.type === "baseline") baseline = update;
          else queue.push(normalizeSerdeValue(update.update, "product-parity.WASM"));
        }, (value) => { failure = value; });
        await host.stop();
        const take = async () => {
          const end = performance.now() + 10_000;
          while (performance.now() < end) {
            if (failure !== undefined) throw new Error(`Browser Worker 故障：${JSON.stringify(failure)}`);
            if (queue.length > 0) return queue.shift();
            await new Promise((resolve) => setTimeout(resolve, 10));
          }
          throw new Error("Browser Worker 未发布已单步提交的协议更新");
        };
        const scenarioHost = {
          async state() {
            await host.refreshBaseline();
            demand(baseline, "Browser Worker 未安装权威快照");
            return { ...baseline.snapshot, civilDate: baseline.civilDate };
          },
          submit: (intent) => host.submitIntent(intent),
          async step() {
            const tick = await host.stepOnceForE2E();
            const update = await take();
            demand(fixtureFacts(update).tick === tick, "Browser Worker 单步与发布游标不一致");
            return update;
          },
          async civil() { await host.endCivilDay(); return take(); },
          async save(candidate) { return normalizeSerdeValue(await host.save(candidate), "product-parity.save"); },
          async restore(slot) {
            observeTopology();
            spawnNames.clear();
            const previous = baseline?.generation;
            await host.load(slot);
            demand(baseline && baseline.generation !== previous, "Browser 读档没有切换世代");
          },
        };
        let measurement;
        if (warmupTicks !== undefined) {
          await scenarioHost.state();
          metrics = { startupNs: now() - startupAt };
          measurement = instrumentWindow(scenarioHost, {
            warmupTicks, measuredTicks: SCENARIO.ticksPerDay - warmupTicks, now,
            start: window.__libosMeasurementStart, end: window.__libosMeasurementEnd,
          });
        }
        const observedAuthority = observeAuthorityPort(measurement?.host ?? scenarioHost);
        result = await executeScenario(observedAuthority.host, expectedGroups);
        authorityLifecycle = observedAuthority.evidence();
        if (measurement !== undefined) Object.assign(metrics, measurement.results());
        ({ groups, desks } = observeTopology());
      } catch (error) {
        scenarioError = error;
        throw error;
      } finally {
        const shutdownAt = now();
        try {
          await host.dispose();
          if (metrics !== undefined) metrics.shutdownNs = now() - shutdownAt;
        } catch (cleanupError) {
          if (scenarioError !== undefined) throw new Error("Browser 对照和 Worker 回收均失败", {
            cause: new AggregateError([scenarioError, cleanupError]),
          });
          throw cleanupError;
        }
      }
      if (metrics !== undefined) {
        demand(finalRelease?.kind === "released" && Number.isSafeInteger(finalRelease.finalBytes) &&
          finalRelease.finalBytes > 0 && finalRelease.finalBytes % 65536 === 0,
          "Browser 没有 shared WASM 线性内存最终释放数据");
        metrics.memoryPeak = finalRelease.finalBytes / 65536;
      }
      return { ...result, authorityLifecycle, groups: groups.length, desks: desks.length, path: "browser-worker-shared-wasm", metrics };
    }, {
      setup, seed: SCENARIO.seed, fixtureModule: "/@fs" + fixture,
      measurementModule: "/@fs" + measurementModule, warmupTicks, expectedGroups,
    });
  } finally {
    await cdp?.detach();
  }
}

export async function runLibosProductExperiment({
  sourceRoot = root, serverBin, mode = "parity", browserExecutable, warmupTicks,
  expectedGroups = 1, contract = "strict-diagnostic",
}) {
  assert.ok(["parity", "native", "browser"].includes(mode), "未识别的 LibOS 产品验收类型");
  assert.ok(["strict-diagnostic", "free-restart", "fixed-accepted-facts"].includes(contract), "未识别的 LibOS 验收契约");
  if (contract === "fixed-accepted-facts") {
    throw new Error("固定已受理事实重放尚未获得证明：\n" + fixedAcceptedFacts.reason);
  }
  assert.ok(expectedGroups === 1 || expectedGroups === 2, "只支持默认或双 Group 产品验收");
  const launchNative = mode !== "browser";
  const launchBrowser = mode !== "native";
  const web = path.join(sourceRoot, "apps/web");
  const artifact = {};
  if (launchNative) artifact.nativeSha256 = hash(await readFile(serverBin));
  if (launchBrowser) {
    // 历史制品采用本版本锁定的 ABI/schema 检验，不用新版本的导出表要求旧制品。
    const { readWasmPackageMetadata, captureWasmBuildSourceIdentity, wasmBuildSourceSha256 } =
      await import(pathToFileURL(path.join(sourceRoot, "scripts/wasm-package-capability.mjs")).href);
    const wasmPackage = path.join(web, "wasm-pkg");
    const [metadata, source, wasmBytes, glueBytes] = await Promise.all([
      readWasmPackageMetadata(wasmPackage),
      captureWasmBuildSourceIdentity(sourceRoot),
      readFile(path.join(wasmPackage, "web_wasm_bg.wasm")),
      readFile(path.join(wasmPackage, "web_wasm.js")),
    ]);
    artifact.wasmSha256 = hash(wasmBytes);
    artifact.glueSha256 = hash(glueBytes);
    artifact.wasmSourceSha256 = wasmBuildSourceSha256(source);
    assert.equal(artifact.wasmSha256, metadata.provenance.wasmSha256, "WASM 制品不匹配 build provenance");
    assert.equal(artifact.glueSha256, metadata.provenance.glueSha256, "WASM glue 不匹配 build provenance");
    assert.equal(artifact.wasmSourceSha256, metadata.provenance.sourceSha256, "WASM 制品不是当前源码构建");
    await verifySharedMemoryArtifact(wasmPackage);
  }
  const { createServer } = await import(pathToFileURL(path.join(web, "node_modules/vite/dist/node/index.js")).href);
  const work = await mkdtemp(path.join(tmpdir(), "libos-product-parity-"));
  let vite, browser, child, childExit, childError, report;
  try {
    vite = await createServer({ root: web, mode: "e2e", appType: "custom", server: {
      host: "127.0.0.1", port: 0, fs: { allow: [sourceRoot, root] },
      ...(launchBrowser ? {} : { middlewareMode: true, hmr: false, watch: null }),
    }, ...(launchBrowser ? {} : { optimizeDeps: { noDiscovery: true, include: [] } }) });
    vite.middlewares.use((req, res, next) => {
      if (req.url !== "/libos-product-parity") return next();
      res.setHeader("Content-Type", "text/html");
      res.end("<!doctype html><title>LibOS 产品对照</title>");
    });
    let page;
    if (launchBrowser) {
      await vite.listen();
      const { chromium } = await import(pathToFileURL(path.join(web, "node_modules/@playwright/test/index.mjs")).href);
      browser = await chromium.launch({ headless: true, executablePath: browserExecutable });
      page = await browser.newPage();
      const browserUrl = vite.resolvedUrls?.local[0];
      assert.ok(browserUrl, "Vite 未分配回环地址");
      await page.goto(new URL("/libos-product-parity", browserUrl).href);
    }
    const defaults = launchBrowser
      ? await page.evaluate(async () => (await import("/src/config/defaults.ts")).DEFAULT_SETUP)
      : (await vite.ssrLoadModule("/src/config/defaults.ts")).DEFAULT_SETUP;
    const setup = makeSetup(defaults);
    let address, startupAt;
    if (launchNative) {
      const database = path.join(work, "market.sqlite");
      const env = { ...process.env, RUST_LOG: "info", NO_COLOR: "1" };
      delete env.ENGINE_STEP_RUNTIME;
      delete env.STOCK_GAME_PARITY_GROUPS;
      if (expectedGroups === 2) env.STOCK_GAME_PARITY_GROUPS = "2";
      startupAt = now();
      child = spawn(serverBin, ["--services", "server", "--new-market", "--bind", "127.0.0.1:0", "--database", database], {
        cwd: work, env, stdio: ["ignore", "pipe", "pipe"],
      });
      childExit = new Promise((resolve) => { child.once("close", resolve); });
      child.once("error", (error) => { childError = error; });
      let log = "";
      for (const stream of [child.stdout, child.stderr]) {
        stream.setEncoding("utf8");
        stream.on("data", (chunk) => { log = (log + chunk).slice(-16_384); });
      }
      address = await waitFor(() => {
        if (childError !== undefined) throw childError;
        if (child.exitCode !== null || child.signalCode !== null) throw new Error("Native Server 提前退出：" + log);
        return log.replace(/\x1b\[[0-9;]*m/g, "").match(/HTTP\/WS service starting[^\n]*?(127\.0\.0\.1:[1-9]\d*)/)?.[1];
      }, "临时 Native Server 启动");
    }
    const options = { startupAt, warmupTicks, expectedGroups, browser };
    const outcomes = await Promise.allSettled([
      ...(launchNative ? [native("http://" + address, child.pid, setup, options)] : []),
      ...(launchBrowser ? [browserWorker(page, setup, options)] : []),
    ]);
    const failures = outcomes.filter((result) => result.status === "rejected").map((result) => result.reason);
    if (failures.length > 0) throw new AggregateError(failures, "LibOS 产品验收未完成");
    const witnesses = outcomes.map((result) => result.value);
    const fixtureSha256 = hash(canonicalJson({ scenario: SCENARIO, setup, sourceSha256: hash(await readFile(fixture)) }));
    for (const witness of witnesses) {
      assert.equal(canonicalJson(witness.checkpoint), canonicalJson(witness.restoredCheckpoint),
        witness.path + " 已提交日终档恢复导出后不一致");
      freeRestartContract(witness);
      if (contract === "strict-diagnostic") {
        // 保留旧的字节诊断，不把自由调度命中误称作固定受理事实重放。
        assert.equal(canonicalJson(witness.continued), canonicalJson(witness.replayed),
          witness.path + " 自由调度读档后的首拍事实不完全相同；固定受理事实仍需独立证明");
      }
    }
    if (mode !== "parity") {
      report = {
        schema: "libos-product-single-host/v1",
        artifact, fixtureSha256, contract, fixedAcceptedFacts, witness: witnesses[0],
        hostVersion: browser?.version() ?? process.version,
      };
    } else {
      const [nativeWitness, wasmWitness] = witnesses;
      assert.deepEqual(freeRestartContract(nativeWitness), freeRestartContract(wasmWitness),
        "Native / WASM 自由恢复的日历与提交协议不同");
      if (contract === "strict-diagnostic") {
        for (const key of ["trace", "inputs", "checkpoint"]) {
          assert.equal(canonicalJson(nativeWitness[key]), canonicalJson(wasmWitness[key]), "Native / WASM 权威 " + key + " 不一致");
        }
      }
      const summarize = ({ path, groups, desks, lastTick, lastSeq, playerTradeCount, npcTradeCount, npcGroupActivity }) =>
        ({ path, groups, desks, lastTick, lastSeq, playerTradeCount, npcTradeCount, npcGroupActivity });
      const authority = ({ trace, inputs, checkpoint, freeRestart, authorityLifecycle }) => ({ trace, inputs, checkpoint, freeRestart, authorityLifecycle });
      const traceDigests = (witness) => ({
        authority: hash(canonicalJson(authority(witness))),
        uninterrupted: hash(canonicalJson(witness.freeRestart.uninterrupted.frames)),
        restarted: hash(canonicalJson(witness.freeRestart.restarted.frames)),
      });
      report = contract === "strict-diagnostic" ? {
        schema: "libos-product-parity/v1",
        fixtureSha256, contract, fixedAcceptedFacts,
        authorityTraceSha256: hash(canonicalJson({ trace: nativeWitness.trace, inputs: nativeWitness.inputs, checkpoint: nativeWitness.checkpoint })),
        native: { ...summarize(nativeWitness), authorityLifecycle: nativeWitness.authorityLifecycle }, wasm: { ...summarize(wasmWitness), authorityLifecycle: wasmWitness.authorityLifecycle }, artifact,
      } : {
        schema: "libos-product-free-restart/v1",
        fixtureSha256, contract, fixedAcceptedFacts, artifact,
        native: { ...summarize(nativeWitness), digests: traceDigests(nativeWitness), evidence: authority(nativeWitness) },
        wasm: { ...summarize(wasmWitness), digests: traceDigests(wasmWitness), evidence: authority(wasmWitness) },
      };
    }
  } finally {
    try {
      if (browser) await browser.close();
    } finally {
      try { if (vite) await vite.close(); }
      finally {
        if (child) {
          child.kill("SIGTERM");
          try { await Promise.race([childExit, delay(1000)]); }
          finally {
            if (child.exitCode === null && child.signalCode === null) child.kill("SIGKILL");
            await childExit;
          }
        }
        await rm(work, { recursive: true, force: true });
      }
    }
  }
  return report;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  const { output, ...options } = readArgs(process.argv.slice(2));
  const report = await runLibosProductExperiment({ ...options, expectedGroups: 2 });
  // 所有复核与资源回收完成后才生成证据；已有文件拒绝覆盖。
  await writeFile(output, JSON.stringify(report, null, 2) + "\n", { flag: "wx" });
}
