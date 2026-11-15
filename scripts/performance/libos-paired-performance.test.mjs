import assert from "node:assert/strict";
import { test } from "node:test";

import { compareLibosPerformance } from "./libos-paired-performance.mjs";

const H = "a".repeat(64);
const thresholds = {
  throughputLossPct: 5,
  p50RisePct: 10,
  p95RisePct: 10,
  cpuPerTickRisePct: 10,
  memoryRisePct: 10,
  startupRisePct: 10,
  shutdownRisePct: 10,
};

function receipt(target, variant) {
  const browser = target === "browser";
  return {
    schema: "libos-performance-receipt-v1",
    sourceSha: (variant === "baseline" ? "1" : "2").repeat(40),
    sourceTreeSha256: (variant === "baseline" ? "3" : "4").repeat(64),
    lockSha256: H,
    artifactSha256: H,
    context: {
      target,
      platform: "linux-x64",
      machineId: "bench-1",
      host: browser ? "browser-worker-shared-wasm" : "native-server-api-ws",
      hostVersion: "probe-1",
      executionVersion: browser ? "Chromium 130" : "linux 6.8",
      toolchain: "rustc 1.82.0",
      profile: "release",
      features: ["production"],
      cpuSet: [1, 2, 3, 4],
      workerCount: 4,
      workloadSha256: H,
      warmupTicks: 12,
      measuredTicks: 40,
    },
    samples: Array.from({ length: 3 }, (_, i) => ({
      pairId: `pair-${i}`,
      measurementId: `${variant}-${i}`,
      elapsedNs: 80_000_000,
      cpuNs: 100_000_000,
      memoryPeak: browser ? 360 : 16_777_216,
      startupNs: 30_000_000,
      shutdownNs: 20_000_000,
      tickNs: Array(40).fill(1_000_000),
      authority: {
        firstCommittedTick: 13, lastCommittedTick: 52, lastCommittedSeq: 71,
        readyReceiptSha256: H, factsSha256: H, activeGroups: 1,
        deskExchanges: ["Shanghai", "Shenzhen"], playerAccepts: 1, npcOrders: 2,
      },
    })),
  };
}

test("按同目标配对并分别计算真实 tick、CPU 与内存口径", () => {
  for (const target of ["native", "browser"]) {
    const report = compareLibosPerformance(receipt(target, "baseline"), receipt(target, "candidate"), thresholds);
    assert.equal(report.pass, true);
    assert.equal(report.target, target);
    assert.equal(report.memoryUnit, target === "browser" ? "wasm-pages" : "rss-bytes");
    assert.equal(report.pairs[0].baseline.throughput, 500);
    assert.equal(report.pairs[0].baseline.cpuPerTickNs, 2_500_000);
    assert.equal(report.pairs[0].baseline.p95Ns, 1_000_000);
  }
});

test("拒绝跨目标、换机器、换线程配置和事实不同的样本", () => {
  const baseline = receipt("native", "baseline");
  assert.throws(() => compareLibosPerformance(baseline, receipt("browser", "candidate"), thresholds), /context.target/);
  const changed = receipt("native", "candidate");
  changed.context.cpuSet = [2, 3, 4, 5];
  assert.throws(() => compareLibosPerformance(baseline, changed, thresholds), /context.cpuSet/);
  changed.context.cpuSet = baseline.context.cpuSet;
  changed.samples[1].authority.factsSha256 = "b".repeat(64);
  assert.throws(() => compareLibosPerformance(baseline, changed, thresholds), /authority/);
});

test("拒绝缺测、复用采样和用零 NPC 伪造市场负载", () => {
  const baseline = receipt("native", "baseline");
  const other = receipt("native", "candidate");
  other.samples[0].tickNs.pop();
  assert.throws(() => compareLibosPerformance(baseline, other, thresholds), /tickNs/);
  other.samples[0].tickNs.push(1_000_000);
  other.samples[0].authority.npcOrders = 0;
  assert.throws(() => compareLibosPerformance(baseline, other, thresholds), /npcOrders/);
  other.samples[0].authority.npcOrders = 2;
  other.samples[0].measurementId = baseline.samples[0].measurementId;
  assert.throws(() => compareLibosPerformance(baseline, other, thresholds), /measurementId/);
});

test("每项阈值必须先明确，p95 退化独立阻断", () => {
  const baseline = receipt("native", "baseline");
  const other = receipt("native", "candidate");
  assert.throws(() => compareLibosPerformance(baseline, other, {}), /thresholds/);
  for (const sample of other.samples) sample.tickNs.fill(2_000_000, 36);
  const report = compareLibosPerformance(baseline, other, thresholds);
  assert.equal(report.pass, false);
  assert.deepEqual(report.failures.map(({ gate }) => gate), ["p95RisePct"]);
  assert.equal(report.medianRegressionPct.p95RisePct, 100);
});

test("拒绝无对应样本的 pairId 或超出亲和集预算的 CPU 数据", () => {
  const baseline = receipt("native", "baseline");
  const other = receipt("native", "candidate");
  other.samples[1].pairId = "not-the-same-pair";
  assert.throws(() => compareLibosPerformance(baseline, other, thresholds), /缺少同条件/);
  other.samples[1].pairId = baseline.samples[1].pairId;
  other.samples[1].cpuNs = other.samples[1].elapsedNs * other.context.cpuSet.length + 1;
  assert.throws(() => compareLibosPerformance(baseline, other, thresholds), /cpuNs/);
});
