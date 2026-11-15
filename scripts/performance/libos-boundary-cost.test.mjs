import assert from "node:assert/strict";
import test from "node:test";
import { evaluateBoundaryCosts, summarizeBoundarySamples } from "./libos-boundary-cost.mjs";

const samples = (direct = 2000, facade = 2002) => Array.from({ length: 31 }, (_, i) => ({
  order: i % 2 ? "facade-direct" : "direct-facade",
  operations: 100, segments: 1, directNs: direct, facadeNs: facade,
}));

test("给出配对中位数区间与显式计时量化裕量", () => {
  const report = summarizeBoundarySamples(samples(), 1);
  assert.equal(report.pairs, 31);
  assert.equal(report.medianDeltaNsPerOperation, .02);
  assert.ok(report.signIntervalCoverage >= .95);
  assert.deepEqual(report.resolutionAdjustedIntervalNsPerOperation, [0, .04]);
});

test("拒绝同向测量、次数不均以及接近分辨率的样本", () => {
  assert.throws(() => summarizeBoundarySamples(samples().map((row) => ({ ...row, order: "direct-facade" })), 1), /配对顺序/);
  assert.throws(() => summarizeBoundarySamples(samples(), 100), /量化/);
  assert.throws(() => summarizeBoundarySamples(samples().slice(1), 1), /31/);
  assert.throws(() => summarizeBoundarySamples(samples().map((row, i) => ({ ...row, operations: row.operations + i })), 1), /操作数/);
});

test("多段计时累计每一段量化裕量", () => {
  const segmented = samples(20_000, 20_010).map((row) => ({ ...row, segments: 5 }));
  const report = summarizeBoundarySamples(segmented, 1);
  assert.equal(report.segmentsPerArm, 5);
  assert.equal(report.timerGuardNsPerOperation, 0.1);
  assert.throws(() => summarizeBoundarySamples(segmented.map((row, index) => ({
    ...row, segments: index === 1 ? 6 : 5,
  })), 1), /段数/);
});

test("p95 只描述批均值分布，不宣称单次耗时的 p95", () => {
  const increasing = samples().map((pair, index) => ({
    ...pair,
    directNs: 2000 + index * 100,
    facadeNs: 2002 + index * 100,
  }));
  const report = summarizeBoundarySamples(increasing, 1);
  assert.equal(report.p95DirectBatchMeanNsPerOperation, 49);
  assert.equal(report.p95FacadeBatchMeanNsPerOperation, 49.02);
});

const policy = (name = "spawn-join") => ({
  schema: "libos-boundary-budget-v1",
  target: "native",
  cases: { [name]: { maxMedianDeltaNs: 10, maxMedianDeltaFraction: .1 } },
});
const evidence = (low, high, directMedian = 50) => ({
  medianDirectNsPerOperation: directMedian,
  resolutionAdjustedIntervalNsPerOperation: [low, high],
});

test("绝对和相对预算须同时满足，量化调整后的区间须完整落在预算内", () => {
  const operation = (low, high, median) => ({ "spawn-join": evidence(low, high, median) });
  assert.equal(evaluateBoundaryCosts(operation(2, 5, 50), "native", policy()).verdict, "pass");
  assert.equal(evaluateBoundaryCosts(operation(4, 6, 50), "native", policy()).verdict, "inconclusive");
  assert.equal(evaluateBoundaryCosts(operation(6, 9, 50), "native", policy()).verdict, "regression");
  const absoluteWins = evaluateBoundaryCosts(operation(11, 12, 500), "native", policy());
  assert.equal(absoluteWins.verdict, "regression");
  assert.equal(absoluteWins.cases["spawn-join"].limitNs, 10);
});

test("错误平台、缺项、额外预算或无效门槛不产生通过结论", () => {
  const operations = { "spawn-join": evidence(0, 2), "shutdown-wake": evidence(0, 2) };
  assert.throws(() => evaluateBoundaryCosts(operations, "browser", policy()), /匹配本次平台/);
  assert.throws(() => evaluateBoundaryCosts(operations, "native", policy()), /覆盖所有且仅覆盖/);
  const extra = policy();
  extra.cases["shutdown-wake"] = { maxMedianDeltaNs: 1, maxMedianDeltaFraction: -1 };
  assert.throws(() => evaluateBoundaryCosts(operations, "native", extra), /无效阈值/);
  assert.throws(() => evaluateBoundaryCosts({ "spawn-join": evidence(3, 1) }, "native", policy()), /无效配对区间/);
  assert.throws(() => evaluateBoundaryCosts({}, "native", policy()), /没有可判定操作/);
});
