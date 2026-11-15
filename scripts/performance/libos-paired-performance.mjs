import { closeSync, constants, fstatSync, openSync, readSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { isDeepStrictEqual } from "node:util";

const SHAS = /^[a-f0-9]{64}(?![\s\S])/;
const GIT_SHA = /^[a-f0-9]{40}(?![\s\S])/;
const HOSTS = new Map([
  ["native", "native-server-api-ws"],
  ["browser", "browser-worker-shared-wasm"],
]);
const GATES = {
  throughputLossPct: ["throughput", -1],
  p50RisePct: ["p50Ns", 1],
  p95RisePct: ["p95Ns", 1],
  cpuPerTickRisePct: ["cpuPerTickNs", 1],
  memoryRisePct: ["memoryPeak", 1],
  startupRisePct: ["startupNs", 1],
  shutdownRisePct: ["shutdownNs", 1],
};

function fail(field, reason) {
  throw new Error(`${field}: ${reason}`);
}

function exact(value, keys, label) {
  if (value === null || typeof value !== "object" || Array.isArray(value) ||
    Object.keys(value).length !== keys.length || !keys.every((key) => Object.hasOwn(value, key))) {
    fail(label, `需要且只接受字段 ${keys.join(", ")}`);
  }
}

function str(value, label, pattern = /^\S(?:.{0,198}\S)?(?![\s\S])/u) {
  if (typeof value !== "string" || !pattern.test(value)) fail(label, "文本标识格式不合法");
}

function integer(value, label, min = 1, max = Number.MAX_SAFE_INTEGER) {
  if (!Number.isSafeInteger(value) || value < min || value > max) {
    fail(label, `需要 ${min} 到 ${max} 之间的整数`);
  }
}

function thresholdValues(thresholds) {
  exact(thresholds, Object.keys(GATES), "thresholds");
  for (const [key, value] of Object.entries(thresholds)) {
    if (!Number.isFinite(value) || value < 0 || (key === "throughputLossPct" && value > 100)) {
      fail(`thresholds.${key}`, "阈值必须是非负有限百分数，吞吐下降阈值不得大于 100");
    }
  }
  return thresholds;
}

function validateContext(value, label) {
  exact(value, ["target", "platform", "machineId", "host", "hostVersion", "executionVersion",
    "toolchain", "profile", "features", "cpuSet", "workerCount", "workloadSha256",
    "warmupTicks", "measuredTicks"], label);
  if (!HOSTS.has(value.target) || value.host !== HOSTS.get(value.target)) {
    fail(`${label}.target`, "目标必须对应 Native Server API/WS 或浏览器 shared-memory Worker");
  }
  for (const key of ["platform", "machineId", "hostVersion", "executionVersion", "toolchain"]) {
    str(value[key], `${label}.${key}`);
  }
  if (value.profile !== "release") fail(`${label}.profile`, "正式宿主须使用 release");
  if (!Array.isArray(value.features) || value.features.length > 100) {
    fail(`${label}.features`, "需要明确且有序的 feature 列表，空列表也必须显式声明");
  }
  value.features.forEach((feature, index) => {
    str(feature, `${label}.features[${index}]`, /^[a-zA-Z0-9_:/+.\-]{1,100}(?![\s\S])/);
    if (index > 0 && value.features[index - 1] >= feature) fail(`${label}.features`, "应无重复并严格升序");
  });
  if (!Array.isArray(value.cpuSet) || value.cpuSet.length < 2 || value.cpuSet.length > 1024) {
    fail(`${label}.cpuSet`, "需要至少两个绑定的逻辑 CPU");
  }
  value.cpuSet.forEach((cpu, index) => {
    integer(cpu, `${label}.cpuSet[${index}]`, 0);
    if (index > 0 && value.cpuSet[index - 1] >= cpu) fail(`${label}.cpuSet`, "应无重复并严格升序");
  });
  integer(value.workerCount, `${label}.workerCount`, 2, 65536);
  str(value.workloadSha256, `${label}.workloadSha256`, SHAS);
  integer(value.warmupTicks, `${label}.warmupTicks`, 0, 100_000);
  integer(value.measuredTicks, `${label}.measuredTicks`, 20, 100_000);
}

function validateAuthority(value, ticks, label) {
  exact(value, ["firstCommittedTick", "lastCommittedTick", "lastCommittedSeq", "readyReceiptSha256",
    "factsSha256", "activeGroups", "deskExchanges", "playerAccepts", "npcOrders"], label);
  for (const key of ["firstCommittedTick", "lastCommittedTick", "lastCommittedSeq", "playerAccepts", "npcOrders"]) {
    integer(value[key], `${label}.${key}`);
  }
  integer(value.activeGroups, `${label}.activeGroups`);
  if (!isDeepStrictEqual(value.deskExchanges, ["Shanghai", "Shenzhen"])) {
    fail(`${label}.deskExchanges`, "至少两个真实 Desk 工作，且须见证沪深双市场");
  }
  if (value.lastCommittedTick - value.firstCommittedTick + 1 !== ticks) {
    fail(`${label}.lastCommittedTick`, "已提交 tick 范围必须与测量窗相符");
  }
  for (const key of ["readyReceiptSha256", "factsSha256"]) str(value[key], `${label}.${key}`, SHAS);
}

function validateReceipt(value, version, measurementIds) {
  exact(value, ["schema", "sourceSha", "sourceTreeSha256", "lockSha256", "artifactSha256", "context", "samples"], version);
  if (value.schema !== "libos-performance-receipt-v1") fail(`${version}.schema`, "不接受未知测量格式");
  str(value.sourceSha, `${version}.sourceSha`, GIT_SHA);
  for (const key of ["sourceTreeSha256", "lockSha256", "artifactSha256"]) str(value[key], `${version}.${key}`, SHAS);
  validateContext(value.context, `${version}.context`);
  if (!Array.isArray(value.samples) || value.samples.length < 3 || value.samples.length > 21 || value.samples.length % 2 !== 1) {
    fail(`${version}.samples`, "至少 3 对且最多 21 对，以奇数对独立运行，便于取中位数");
  }
  const pairs = new Map();
  for (const [index, sample] of value.samples.entries()) {
    const label = `${version}.samples[${index}]`;
    exact(sample, ["pairId", "measurementId", "elapsedNs", "cpuNs", "memoryPeak", "startupNs",
      "shutdownNs", "tickNs", "authority"], label);
    str(sample.pairId, `${label}.pairId`);
    if (pairs.has(sample.pairId)) fail(`${label}.pairId`, "同侧重复配对 ID");
    pairs.set(sample.pairId, sample);
    str(sample.measurementId, `${label}.measurementId`);
    if (measurementIds.has(sample.measurementId)) fail(`${label}.measurementId`, "不可复用一次测量");
    measurementIds.add(sample.measurementId);
    for (const key of ["elapsedNs", "cpuNs", "memoryPeak", "startupNs", "shutdownNs"]) {
      integer(sample[key], `${label}.${key}`);
    }
    const { measuredTicks, cpuSet } = value.context;
    if (sample.cpuNs / sample.elapsedNs > cpuSet.length) fail(`${label}.cpuNs`, "CPU 总时间超过绑定 CPU 在测量窗中的预算");
    if (!Array.isArray(sample.tickNs) || sample.tickNs.length !== measuredTicks) {
      fail(`${label}.tickNs`, "实际 tick 计时样本数必须与测量窗相符");
    }
    let total = 0;
    sample.tickNs.forEach((ns, tickIndex) => {
      integer(ns, `${label}.tickNs[${tickIndex}]`, 1, sample.elapsedNs);
      total += ns;
    });
    if (total > sample.elapsedNs) fail(`${label}.tickNs`, "顺序 tick 耗时总和超过同一测量窗");
    validateAuthority(sample.authority, measuredTicks, `${label}.authority`);
  }
  return pairs;
}

function nearestRank(sorted, quantile) {
  return sorted[Math.ceil(sorted.length * quantile) - 1];
}

function metrics(sample) {
  const sorted = [...sample.tickNs].sort((a, b) => a - b);
  return {
    throughput: sorted.length * 1e9 / sample.elapsedNs,
    p50Ns: nearestRank(sorted, 0.50),
    p95Ns: nearestRank(sorted, 0.95),
    cpuPerTickNs: sample.cpuNs / sorted.length,
    cpuNs: sample.cpuNs,
    elapsedNs: sample.elapsedNs,
    memoryPeak: sample.memoryPeak,
    startupNs: sample.startupNs,
    shutdownNs: sample.shutdownNs,
  };
}

function provenance(value) {
  const { sourceSha, sourceTreeSha256, lockSha256, artifactSha256 } = value;
  return { sourceSha, sourceTreeSha256, lockSha256, artifactSha256 };
}

export function compareLibosPerformance(baseline, candidate, thresholds) {
  thresholdValues(thresholds);
  const ids = new Set();
  const before = validateReceipt(baseline, "baseline", ids);
  const after = validateReceipt(candidate, "candidate", ids);
  const { context } = baseline;
  for (const key of Object.keys(context)) {
    if (!isDeepStrictEqual(context[key], candidate.context[key])) fail(`context.${key}`, "两侧测量环境或工作负载不一致");
  }
  if (baseline.sourceTreeSha256 === candidate.sourceTreeSha256) fail("sourceTreeSha256", "两侧不能来自同一源码树");
  if (before.size !== after.size) fail("samples", "两侧配对数量不一致");
  const pairs = [...before].map(([pairId, a]) => {
    const b = after.get(pairId);
    if (!b) fail(`pair.${pairId}`, "缺少同条件的 candidate 配对");
    if (!isDeepStrictEqual(a.authority, b.authority)) {
      fail(`pair.${pairId}.authority`, "受理轨迹或提交事实不一致；不比较不同业务执行的性能");
    }
    const base = metrics(a);
    const next = metrics(b);
    const regressionPct = Object.fromEntries(Object.entries(GATES).map(([gate, [metric, sign]]) =>
      [gate, 100 * sign * (next[metric] - base[metric]) / base[metric]]));
    return { pairId, baseline: base, candidate: next, authority: a.authority, regressionPct };
  });
  const medianRegressionPct = Object.fromEntries(Object.keys(GATES).map((gate) =>
    [gate, nearestRank(pairs.map((pair) => pair.regressionPct[gate]).sort((a, b) => a - b), 0.50)]));
  const failures = Object.entries(medianRegressionPct)
    .filter(([gate, observed]) => observed > thresholds[gate])
    .map(([gate, observed]) => ({ gate, observed, limit: thresholds[gate] }));
  return {
    schema: "libos-performance-comparison-v1",
    target: context.target,
    memoryUnit: context.target === "native" ? "rss-bytes" : "wasm-pages",
    context,
    baseline: provenance(baseline),
    candidate: provenance(candidate),
    thresholds, pairs, medianRegressionPct, failures,
    pass: failures.length === 0,
  };
}

function readJson(filename, maxBytes) {
  const fd = openSync(filename, constants.O_RDONLY | constants.O_NONBLOCK);
  try {
    const stat = fstatSync(fd);
    if (!stat.isFile() || stat.size === 0 || stat.size > maxBytes) {
      fail(filename, `只读取非空普通文件，且不得超过 ${maxBytes} 字节`);
    }
    const bytes = Buffer.alloc(stat.size);
    let total = 0;
    while (total < bytes.length) {
      const count = readSync(fd, bytes, total, bytes.length - total, null);
      if (count === 0) fail(filename, "读取期间测量文件截断");
      total += count;
    }
    if (readSync(fd, Buffer.alloc(1), 0, 1, null) !== 0) fail(filename, "读取期间测量文件增长");
    try {
      return JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
    } catch (error) {
      throw new Error(`${filename}: 测量文件不是合法 UTF-8 JSON`, { cause: error });
    }
  } finally {
    closeSync(fd);
  }
}

function main(args) {
  if (args.length !== 3) fail("usage", "libos-paired-performance.mjs <baseline.json> <candidate.json> <thresholds.json>");
  const report = compareLibosPerformance(readJson(args[0], 32 * 1024 * 1024),
    readJson(args[1], 32 * 1024 * 1024), readJson(args[2], 4096));
  process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
  process.exitCode = report.pass ? 0 : 1;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    console.error(`LibOS 性能配对不可判定：${error.message}`);
    process.exitCode = 2;
  }
}
