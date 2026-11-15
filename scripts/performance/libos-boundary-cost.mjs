#!/usr/bin/env node
// 对同一制品中的两条路径做交错配对，不混用 Native 与 Browser 绝对速度。
import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { readFile, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { arch, platform, release } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { parseCpuSet } from "./libos-collect-performance.mjs";

const run = promisify(execFile);
const sha = (bytes) => createHash("sha256").update(bytes).digest("hex");
const commonCases = ["clock-now", "buffered-recv", "idle-deadline"];

export function summarizeBoundarySamples(pairs, resolutionNs) {
  assert.ok(Array.isArray(pairs) && pairs.length === 31, "需要 31 对固定样本");
  assert.ok(Number.isFinite(resolutionNs) && resolutionNs > 0, "缺少正的实测分辨率");
  const { operations, segments } = pairs[0];
  assert.ok(Number.isSafeInteger(operations) && operations > 0, "操作数无效");
  assert.ok(Number.isSafeInteger(segments) && segments > 0 && segments <= operations, "计时段数无效");
  const checked = pairs.map(({ order, operations: count, segments: segmentCount, directNs, facadeNs }, index) => {
    assert.equal(order, index % 2 === 0 ? "direct-facade" : "facade-direct", "两侧需要交错配对顺序");
    assert.equal(count, operations, "固定操作数不一致");
    assert.equal(segmentCount, segments, "固定计时段数不一致");
    assert.ok(Number.isFinite(directNs) && Number.isFinite(facadeNs) &&
      Math.min(directNs, facadeNs) >= 100 * resolutionNs * segments, "样本距离计时量化不足 100 倍");
    return { direct: directNs / count, facade: facadeNs / count, delta: (facadeNs - directNs) / count };
  });
  const sorted = (key) => checked.map((row) => row[key]).sort((a, b) => a - b);
  const deltas = sorted("delta");
  // 对总体配对差中位数的无分布假设双边符号区间：选使双端概率 <= 5% 的最大阶数。
  // 相邻批次若相关，该覆盖率只是在独立配对模型下的名义覆盖率。
  let term = 2 ** -pairs.length;
  let tail = term, next = 1;
  for (; next < pairs.length; next++) {
    term *= (pairs.length - next + 1) / next;
    if (2 * (tail + term) > 0.05) break;
    tail += term;
  }
  const low = deltas[next - 1], high = deltas[deltas.length - next];
  // 两条均是两个计时读数的差，采用两倍观测到的量化间隔作为比较裕量。
  // 该裕量不是浏览器隐私噪声或调度误差的保证。
  const timerGuard = 2 * resolutionNs * segments / operations;
  const mid = Math.floor(pairs.length / 2);
  const highRank = Math.ceil(pairs.length * 0.95) - 1;
  return {
    pairs: pairs.length, operationsPerArm: operations, segmentsPerArm: segments, observedResolutionNs: resolutionNs,
    medianDirectNsPerOperation: sorted("direct")[mid],
    medianFacadeNsPerOperation: sorted("facade")[mid],
    medianDeltaNsPerOperation: deltas[mid],
    p95DirectBatchMeanNsPerOperation: sorted("direct")[highRank],
    p95FacadeBatchMeanNsPerOperation: sorted("facade")[highRank],
    distributionFreeMedianIntervalNsPerOperation: [low, high],
    signIntervalCoverage: 1 - 2 * tail,
    resolutionAdjustedIntervalNsPerOperation: [low - timerGuard, high + timerGuard],
    timerGuardNsPerOperation: timerGuard,
  };
}

// 预算必须由调用者事前提供；当前样本不能用来拟合它自己的通过阈值。
export function evaluateBoundaryCosts(operations, target, policy) {
  assert.equal(policy?.schema, "libos-boundary-budget-v1", "预算缺少确定的 schema");
  assert.equal(policy.target, target, "预算必须匹配本次平台与实现");
  assert.ok(policy.cases && typeof policy.cases === "object" && !Array.isArray(policy.cases), "缺少具名操作预算");
  const names = Object.keys(operations).sort();
  assert.ok(names.length > 0, "没有可判定操作");
  assert.deepEqual(Object.keys(policy.cases).sort(), names, "预算必须覆盖所有且仅覆盖实测操作");
  const cases = Object.fromEntries(names.map((name) => {
    const { maxMedianDeltaNs, maxMedianDeltaFraction } = policy.cases[name];
    assert.ok(Number.isFinite(maxMedianDeltaNs) && maxMedianDeltaNs >= 0 &&
      Number.isFinite(maxMedianDeltaFraction) && maxMedianDeltaFraction >= 0, `无效阈值：${name}`);
    const { medianDirectNsPerOperation, resolutionAdjustedIntervalNsPerOperation: [low, high] } = operations[name];
    assert.ok(Number.isFinite(medianDirectNsPerOperation) && medianDirectNsPerOperation > 0 &&
      Number.isFinite(low) && Number.isFinite(high) && low <= high, `无效配对区间：${name}`);
    // 同时守住绝对预算和相对预算；区间跨阈值是证据不足，不能视作通过。
    const limitNs = Math.min(maxMedianDeltaNs, maxMedianDeltaFraction * medianDirectNsPerOperation);
    return [name, { limitNs, intervalNs: [low, high], verdict:
      high <= limitNs ? "pass" : low > limitNs ? "regression" : "inconclusive" }];
  }));
  const verdicts = Object.values(cases).map((row) => row.verdict);
  return { verdict: verdicts.includes("regression") ? "regression" :
    verdicts.includes("inconclusive") ? "inconclusive" : "pass", cases };
}

async function inNative(binary) {
  const { stdout } = await run(binary, [], { maxBuffer: 512 * 1024, timeout: 200_000 });
  return { samples: stdout.trim().split("\n").map((line) => JSON.parse(line)) };
}

async function inBrowser(binary, bytes, contractsOnly) {
  const js = await readFile(new URL("./libos-boundary-worker.mjs", import.meta.url));
  const { chromium } = await import("../../apps/web/node_modules/@playwright/test/index.mjs");
  const http = createServer((request, response) => {
    const file = {
      "/": ["text/html; charset=utf-8", "<!doctype html><title>LibOS release 边界测量</title>"],
      "/worker.mjs": ["text/javascript; charset=utf-8", js],
      "/worker.mjs?contracts=1": ["text/javascript; charset=utf-8", js],
      "/probe.wasm": ["application/wasm", bytes],
    }[request.url];
    if (file === undefined || request.method !== "GET") { response.writeHead(404).end(); return; }
    response.writeHead(200, {
      "content-type": file[0], "cross-origin-opener-policy": "same-origin",
      "cross-origin-embedder-policy": "require-corp", "cache-control": "no-store",
    }).end(file[1]);
  });
  let browser;
  try {
    await new Promise((resolve, reject) => {
      http.once("error", reject);
      http.listen(0, "127.0.0.1", resolve);
    });
    const { port } = http.address();
    browser = await chromium.launch({ executablePath: binary, headless: true, timeout: 20_000 });
    const page = await browser.newPage();
    await page.goto("http://127.0.0.1:" + port + "/", { timeout: 10_000 });
    const result = await page.evaluate(async (contractsOnly) => {
      if (!crossOriginIsolated) throw new Error("Browser 页面没有跨源隔离");
      const worker = new Worker(contractsOnly ? "/worker.mjs?contracts=1" : "/worker.mjs", { type: "module" });
      try {
        return await new Promise((resolve, reject) => {
          const deadline = setTimeout(() => reject(new Error("Worker 性能测量超时")), 200_000);
          worker.addEventListener("message", ({ data }) => {
            clearTimeout(deadline);
            if (data.error !== undefined) reject(new Error(data.error));
            else resolve(data);
          }, { once: true });
          worker.addEventListener("error", (event) => {
            clearTimeout(deadline);
            reject(new Error(event.message));
          }, { once: true });
        });
      } finally {
        worker.terminate();
      }
    }, contractsOnly);
    return { ...result, hostVersion: browser.version(), workerKind: "cross-origin-isolated-dedicated-worker" };
  } finally {
    try {
      if (browser) await browser.close();
    } finally {
      if (http.listening) await new Promise((resolve, reject) => http.close((error) => error ? reject(error) : resolve()));
    }
  }
}

async function main(args) {
  const budgetIndex = args.indexOf("--budget");
  assert.ok(budgetIndex === -1 || budgetIndex === args.length - 2, "--budget <绝对 JSON 路径> 必须在最后");
  const budgetPath = budgetIndex === -1 ? undefined : args[budgetIndex + 1];
  const positional = budgetIndex === -1 ? args : args.slice(0, budgetIndex);
  const [target, artifact, output, chromium] = positional;
  const contractsOnly = target === "browser-contracts";
  const browser = contractsOnly || target === "browser";
  assert.ok((target === "native" && positional.length === 3) || (browser && positional.length === 4),
    "用法：libos-boundary-cost.mjs <native|browser|browser-contracts> <release 探针制品> <新的报告 JSON> [Chromium 路径] [--budget 预算JSON]");
  assert.ok(!contractsOnly || budgetPath === undefined, "语义检查不能产生性能预算结果");
  assert.ok([artifact, output, ...(browser ? [chromium] : []), ...(budgetPath === undefined ? [] : [budgetPath])].every(path.isAbsolute),
    "要求绝对路径");
  assert.equal(platform(), "linux", "目前要求可观测 Linux affinity");
  const budgetBytes = budgetPath === undefined ? undefined : await readFile(budgetPath);
  const budget = budgetBytes === undefined ? undefined : JSON.parse(budgetBytes);
  const cpuSet = parseCpuSet(await readFile("/proc/self/status", "utf8"));
  const bytes = await readFile(artifact);
  const result = browser ? await inBrowser(chromium, bytes, contractsOnly) : await inNative(artifact);
  assert.deepEqual(await readFile(artifact), bytes, "探针制品在采样期间被替换");
  if (browser) assert.equal(result.clockWitness?.probeVersion, 2, "不能读取旧 Browser 性能制品");
  if (contractsOnly) {
    assert.deepEqual(result.clockWitness?.contractsPassed,
      ["clock-duration", "worker-delivery", "coordinator-delivery"]);
    const report = { schema: "libos-browser-contracts-v1", target, artifactSha256: sha(bytes), ...result };
    await writeFile(output, JSON.stringify(report, null, 2) + "\n", { flag: "wx" });
    console.log(JSON.stringify(report, null, 2));
    return;
  }
  const { samples, ...browserWitness } = result;
  const cases = target === "native" ?
    [...commonCases, "spawn-join", "message-handoff", "shutdown-wake", "queued-drop-return", "queued-drop-complete"] :
    commonCases;
  assert.ok(Array.isArray(samples) && samples.length === 31 * cases.length, "缺少规定操作的完整分组");
  assert.ok(samples.every(({ case: name }) => cases.includes(name)), "未知探针操作");
  const operations = Object.fromEntries(cases.map((name) => {
    const rows = samples.filter(({ case: which }) => which === name);
    const resolution = rows[0]?.observedResolutionNs;
    assert.ok(rows.every(({ observedResolutionNs }) => Object.is(observedResolutionNs, resolution)),
      "同一操作应在同一计时分辨率下执行");
    return [name, summarizeBoundarySamples(rows, resolution)];
  }));
  const report = {
    schema: "libos-boundary-cost-v1", target,
    platform: platform() + "-" + arch() + " " + release(), cpuSet,
    artifactSha256: sha(bytes), runnerSha256: sha(await readFile(fileURLToPath(import.meta.url))),
    ...(target === "browser"
      ? { ...browserWitness, workerProbeSha256: sha(await readFile(new URL("./libos-boundary-worker.mjs", import.meta.url))) }
      : { hostVersion: process.version }),
    operations, samples,
    ...(budget === undefined ? { budgetEvaluation: { verdict: "not-asserted" } } :
      { budgetEvaluation: { ...evaluateBoundaryCosts(operations, target, budget), budgetSha256: sha(budgetBytes) } }),
  };
  await writeFile(output, JSON.stringify(report, null, 2) + "\n", { flag: "wx" });
  console.log(JSON.stringify(operations, null, 2));
  if (budget !== undefined && report.budgetEvaluation.verdict !== "pass") {
    throw new Error("性能预算未通过：" + report.budgetEvaluation.verdict);
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    await main(process.argv.slice(2));
  } catch (error) {
    console.error(error);
    process.exitCode = 1;
  }
}
