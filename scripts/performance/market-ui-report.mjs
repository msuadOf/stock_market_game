import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import process from "node:process";
import { analyzeChartProgress, buildHtmlReport, formatBrowserException } from "./market-ui-report-lib.mjs";

const projectRoot = resolve(dirname(new URL(import.meta.url).pathname.replace(/^\/(?=[A-Za-z]:)/, "")), "..", "..");
const defaultChromePaths = process.platform === "win32"
  ? [
      "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
      "C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe",
      "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe",
      "C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe",
    ]
  : ["/usr/bin/google-chrome", "/usr/bin/chromium", "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"];

function parseArgs(argv) {
  const config = {
    url: "http://127.0.0.1:5173/",
    durationMs: 5000,
    output: resolve(projectRoot, "artifacts", "market-ui-performance", new Date().toISOString().replaceAll(":", "-")),
    browser: process.env.MARKET_TEST_BROWSER || "",
  };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--url") config.url = argv[++index];
    else if (arg === "--duration-ms") config.durationMs = Number(argv[++index]);
    else if (arg === "--output") config.output = resolve(argv[++index]);
    else if (arg === "--browser") config.browser = resolve(argv[++index]);
    else if (arg === "--help") config.help = true;
    else throw new Error(`未知参数：${arg}`);
  }
  if (!Number.isFinite(config.durationMs) || config.durationMs < 1000 || config.durationMs > 60_000) {
    throw new Error("--duration-ms 必须是 1000 到 60000 之间的有限数字");
  }
  const parsedUrl = new URL(config.url);
  if (!['http:', 'https:'].includes(parsedUrl.protocol)) throw new Error("--url 只支持 http/https");
  return config;
}

function help() {
  return `行情 UI 最快档性能回归\n\n用法：pnpm test:market-performance -- [选项]\n\n  --url <url>           被测地址（默认 http://127.0.0.1:5173/）\n  --duration-ms <ms>    分时、日K各自观察时长（默认 5000）\n  --output <dir>        报告目录\n  --browser <path>      Chrome/Edge 可执行文件；也可设置 MARKET_TEST_BROWSER\n`;
}

function browserExecutable(explicit) {
  const candidates = explicit ? [explicit] : defaultChromePaths;
  const found = candidates.find((candidate) => existsSync(candidate));
  if (!found) throw new Error("未找到 Chrome/Edge。请用 --browser 或 MARKET_TEST_BROWSER 指定可执行文件。候选：" + candidates.join(", "));
  return found;
}

async function freePort() {
  return new Promise((resolvePort, reject) => {
    const server = createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      if (!address || typeof address === "string") {
        server.close();
        reject(new Error("无法分配 Chrome 调试端口"));
        return;
      }
      server.close((error) => error ? reject(error) : resolvePort(address.port));
    });
  });
}

async function waitFor(description, probe, timeoutMs = 20_000, intervalMs = 100) {
  const deadline = Date.now() + timeoutMs;
  let lastError;
  while (Date.now() < deadline) {
    try {
      const result = await probe();
      if (result) return result;
    } catch (error) {
      lastError = error;
    }
    await new Promise((resolveWait) => setTimeout(resolveWait, intervalMs));
  }
  throw new Error(`${description}超时${lastError ? `：${lastError instanceof Error ? lastError.message : String(lastError)}` : ""}`);
}

async function endpointReady(url) {
  try {
    const response = await fetch(url, { signal: AbortSignal.timeout(1500) });
    return response.ok;
  } catch {
    return false;
  }
}

function startViteIfNeeded(url) {
  const parsed = new URL(url);
  if (parsed.hostname !== "127.0.0.1" && parsed.hostname !== "localhost") return null;
  const viteEntry = resolve(projectRoot, "apps", "web", "node_modules", "vite", "bin", "vite.js");
  if (!existsSync(viteEntry)) {
    throw new Error(`Vite 未安装：${viteEntry}。请先运行 pnpm install。`);
  }
  const viteArgs = ["--host", parsed.hostname, "--port", parsed.port || "80", "--strictPort"];
  const child = spawn(process.execPath, [viteEntry, ...viteArgs], {
    cwd: resolve(projectRoot, "apps", "web"),
    windowsHide: true,
    stdio: ["ignore", "pipe", "pipe"],
  });
  const logChunks = [];
  const remember = (chunk) => {
    logChunks.push(String(chunk));
    if (logChunks.length > 40) logChunks.shift();
  };
  child.stdout.on("data", remember);
  child.stderr.on("data", remember);
  child.recentOutput = () => logChunks.join("").trim();
  return child;
}

class CdpClient {
  constructor(webSocketUrl) {
    this.socket = new WebSocket(webSocketUrl);
    this.nextId = 1;
    this.pending = new Map();
  }

  async connect() {
    await new Promise((resolveOpen, reject) => {
      this.socket.addEventListener("open", resolveOpen, { once: true });
      this.socket.addEventListener("error", () => reject(new Error("Chrome DevTools WebSocket 连接失败")), { once: true });
    });
    this.socket.addEventListener("message", async (event) => {
      const raw = typeof event.data === "string" ? event.data : await event.data.text();
      const message = JSON.parse(raw);
      if (!message.id) return;
      const pending = this.pending.get(message.id);
      if (!pending) return;
      this.pending.delete(message.id);
      if (message.error) pending.reject(new Error(`${pending.method} 失败：${message.error.message}`));
      else pending.resolve(message.result);
    });
  }

  send(method, params = {}) {
    const id = this.nextId++;
    return new Promise((resolveCommand, reject) => {
      this.pending.set(id, { resolve: resolveCommand, reject, method });
      this.socket.send(JSON.stringify({ id, method, params }));
    });
  }

  close() {
    this.socket.close();
  }
}

async function evaluate(client, expression) {
  const result = await client.send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
  if (result.exceptionDetails) {
    throw new Error(`浏览器脚本执行失败：${formatBrowserException(result.exceptionDetails)}`);
  }
  return result.result.value;
}

async function waitForSelector(client, selector, timeoutMs = 20_000) {
  return waitFor(`等待元素 ${selector}`, () => evaluate(client, `Boolean(document.querySelector(${JSON.stringify(selector)}))`), timeoutMs);
}

async function screenshot(client, outputDir, name) {
  const result = await client.send("Page.captureScreenshot", { format: "png", captureBeyondViewport: false, fromSurface: true });
  await writeFile(resolve(outputDir, name), Buffer.from(result.data, "base64"));
  return name;
}

async function readGame(client) {
  return evaluate(client, `(() => {
    const root = document.querySelector('.app-root');
    if (!root) throw new Error('缺少 .app-root');
    return { day: Number(root.dataset.gameDay), tick: Number(root.dataset.gameTick) };
  })()`);
}

async function readIntraday(client) {
  return waitFor("读取分时诊断", () => evaluate(client, `(() => {
    const node = document.querySelector('.msd-market-composite');
    if (!node) throw new Error('分时诊断节点不存在');
    return { count: Number(node.dataset.intradayCount), latestMinute: Number(node.dataset.intradayLatestMinute), signature: node.dataset.intradaySignature };
  })()`));
}

async function readKline(client) {
  return waitFor("读取K线诊断", () => evaluate(client, `(() => {
    const node = document.querySelector('.msd-kline');
    if (!node) throw new Error('K线诊断节点不存在');
    return { count: Number(node.dataset.klineCount), signature: node.dataset.klineSignature };
  })()`));
}

async function readMetrics(client) {
  const result = await client.send("Performance.getMetrics");
  return Object.fromEntries(result.metrics.map(({ name, value }) => [name, value]));
}

function performanceDelta(before, after, probe, durationMs) {
  const milliseconds = (name) => ((after[name] ?? 0) - (before[name] ?? 0)) * 1000;
  return {
    TaskDurationMs: milliseconds("TaskDuration"),
    ScriptDurationMs: milliseconds("ScriptDuration"),
    LayoutDurationMs: milliseconds("LayoutDuration"),
    RecalcStyleDurationMs: milliseconds("RecalcStyleDuration"),
    JSHeapUsedSizeBefore: before.JSHeapUsedSize ?? 0,
    JSHeapUsedSizeAfter: after.JSHeapUsedSize ?? 0,
    JSHeapUsedSizeDelta: (after.JSHeapUsedSize ?? 0) - (before.JSHeapUsedSize ?? 0),
    NodesDelta: (after.Nodes ?? 0) - (before.Nodes ?? 0),
    LongTaskCount: probe.longTasks.length,
    LongTaskDurationMs: probe.longTasks.reduce((sum, value) => sum + value, 0),
    LayoutShiftScore: probe.layoutShift,
    AnimationFrames: probe.frames,
    ObservedWallTimeMs: probe.elapsedMs ?? durationMs * 2,
    ApproxFps: probe.frames / ((probe.elapsedMs ?? durationMs * 2) / 1000),
  };
}

async function stopProcess(child) {
  if (!child || child.exitCode !== null) return;
  if (process.platform === "win32") {
    const killer = spawn("taskkill", ["/pid", String(child.pid), "/t", "/f"], { windowsHide: true, stdio: "ignore" });
    await new Promise((resolveExit) => killer.once("exit", resolveExit));
  } else {
    child.kill("SIGTERM");
  }
}

async function main() {
  const config = parseArgs(process.argv.slice(2));
  if (config.help) {
    process.stdout.write(help());
    return;
  }
  const outputDir = config.output;
  await mkdir(outputDir, { recursive: true });
  await rm(resolve(outputDir, "failure.txt"), { force: true });
  const screenshots = [];
  let server = null;
  let browser = null;
  let client = null;
  let browserProfile = null;
  try {
    if (!(await endpointReady(config.url))) server = startViteIfNeeded(config.url);
    try {
      await waitFor("Vite 页面启动", () => endpointReady(config.url), 30_000, 200);
    } catch (error) {
      const output = server?.recentOutput?.();
      throw new Error(`${error instanceof Error ? error.message : String(error)}${output ? `\nVite 输出：\n${output}` : ""}`);
    }

    const debugPort = await freePort();
    browserProfile = await mkdtemp(resolve(tmpdir(), "market-ui-perf-"));
    browser = spawn(browserExecutable(config.browser), [
      `--remote-debugging-port=${debugPort}`,
      `--user-data-dir=${browserProfile}`,
      "--headless=new",
      "--no-first-run",
      "--no-default-browser-check",
      "--disable-background-timer-throttling",
      "--disable-renderer-backgrounding",
      "about:blank",
    ], { windowsHide: true, stdio: "ignore" });

    await waitFor("Chrome 调试端口", async () => {
      const response = await fetch(`http://127.0.0.1:${debugPort}/json/version`);
      return response.ok;
    });
    const targetResponse = await fetch(`http://127.0.0.1:${debugPort}/json/new?${encodeURIComponent(config.url)}`, { method: "PUT" });
    if (!targetResponse.ok) throw new Error(`创建 Chrome 页面失败：HTTP ${targetResponse.status}`);
    const target = await targetResponse.json();
    if (!target.webSocketDebuggerUrl) throw new Error("Chrome 未返回 webSocketDebuggerUrl");
    client = new CdpClient(target.webSocketDebuggerUrl);
    await client.connect();
    await client.send("Runtime.enable");
    await client.send("Page.enable");
    await client.send("Performance.enable");
    await client.send("Emulation.setDeviceMetricsOverride", { width: 390, height: 1180, deviceScaleFactor: 1, mobile: true });
    await client.send("Page.navigate", { url: config.url });
    await waitForSelector(client, ".app-root", 30_000);
    const isolated = await evaluate(client, "crossOriginIsolated");
    if (!isolated) throw new Error("页面未启用 crossOriginIsolated，WASM 多线程性能测试无效；请检查 COOP/COEP 响应头");

    await evaluate(client, `(() => {
      window.__marketPerfProbe = { startedAt: performance.now(), frames: 0, longTasks: [], layoutShift: 0 };
      const frame = () => { window.__marketPerfProbe.frames += 1; requestAnimationFrame(frame); };
      requestAnimationFrame(frame);
      if ('PerformanceObserver' in window) {
        try { new PerformanceObserver((list) => list.getEntries().forEach((entry) => window.__marketPerfProbe.longTasks.push(entry.duration))).observe({ type: 'longtask', buffered: true }); } catch (error) { console.warn('longtask observer unavailable', error); }
        try { new PerformanceObserver((list) => list.getEntries().forEach((entry) => { if (!entry.hadRecentInput) window.__marketPerfProbe.layoutShift += entry.value; })).observe({ type: 'layout-shift', buffered: true }); } catch (error) { console.warn('layout-shift observer unavailable', error); }
      }
    })()`);

    await evaluate(client, `document.querySelector('.mobile-market-row')?.click()`);
    await waitForSelector(client, ".mobile-stock-detail");
    await evaluate(client, `(() => {
      const select = document.querySelector('select[aria-label="模拟速度"]');
      if (!select) throw new Error('找不到模拟速度选择器');
      select.value = 'Infinity';
      select.dispatchEvent(new Event('change', { bubbles: true }));
      const toggle = document.querySelector('.mobile-run-toggle--detail');
      if (toggle?.dataset.state === 'paused') toggle.click();
    })()`);
    await new Promise((resolveWait) => setTimeout(resolveWait, 250));

    await client.send("HeapProfiler.collectGarbage");
    const metricBefore = await readMetrics(client);
    const intradayBefore = await readIntraday(client);
    const gameBefore = await readGame(client);
    screenshots.push(await screenshot(client, outputDir, "01-intraday-before.png"));
    await new Promise((resolveWait) => setTimeout(resolveWait, config.durationMs));
    const intradayAfter = await readIntraday(client);
    screenshots.push(await screenshot(client, outputDir, "02-intraday-after.png"));

    await evaluate(client, `(() => {
      const tab = document.querySelector('#period-日K');
      if (!tab) throw new Error('找不到日K标签');
      tab.click();
    })()`);
    await waitForSelector(client, ".msd-kline");
    const klineBefore = await readKline(client);
    screenshots.push(await screenshot(client, outputDir, "03-kline-before.png"));
    await new Promise((resolveWait) => setTimeout(resolveWait, config.durationMs));
    const klineAfter = await readKline(client);
    const gameAfter = await readGame(client);
    screenshots.push(await screenshot(client, outputDir, "04-kline-after.png"));
    await client.send("HeapProfiler.collectGarbage");
    const metricAfter = await readMetrics(client);
    const probe = await evaluate(client, "({...window.__marketPerfProbe, elapsedMs: performance.now() - window.__marketPerfProbe.startedAt})");
    await evaluate(client, `(() => { const toggle = document.querySelector('.mobile-run-toggle--detail'); if (toggle?.dataset.state === 'running') toggle.click(); })()`);

    const before = { game: gameBefore, intraday: intradayBefore, kline: klineBefore };
    const after = { game: gameAfter, intraday: intradayAfter, kline: klineAfter };
    const result = analyzeChartProgress(before, after);
    const fatalError = await evaluate(client, "document.querySelector('.app-error')?.textContent ?? ''");
    result.checks.push({ id: "runtime", label: "运行时错误", passed: !fatalError, detail: fatalError || "未发现 fatal UI" });
    result.passed = result.checks.every((item) => item.passed);
    const report = {
      generatedAt: new Date().toISOString(),
      url: config.url,
      durationMs: config.durationMs,
      result,
      before,
      after,
      performance: performanceDelta(metricBefore, metricAfter, probe, config.durationMs),
      screenshots,
    };
    await writeFile(resolve(outputDir, "report.json"), JSON.stringify(report, null, 2));
    await writeFile(resolve(outputDir, "report.html"), buildHtmlReport(report));
    process.stdout.write(`${result.passed ? "PASS" : "FAIL"} ${resolve(outputDir, "report.html")}\n`);
    if (!result.passed) process.exitCode = 1;
  } catch (error) {
    const message = error instanceof Error ? `${error.name}: ${error.message}\n${error.stack ?? ""}` : String(error);
    await writeFile(resolve(outputDir, "failure.txt"), message);
    throw error;
  } finally {
    client?.close();
    await stopProcess(browser);
    await stopProcess(server);
    if (browserProfile) {
      await rm(browserProfile, { recursive: true, force: true, maxRetries: 8, retryDelay: 250 });
    }
  }
}

main().catch((error) => {
  process.stderr.write(`${error instanceof Error ? error.stack ?? error.message : String(error)}\n`);
  process.exitCode = 1;
});
