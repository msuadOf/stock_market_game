import assert from "node:assert/strict";
import test from "node:test";
import { annualGrowthFactor, composeGrowthFactors, applyGrowthFactor } from "./annual-growth.ts";

test("年化增长按周期取精确复利根，不能线性除以月份", { timeout: 10000 }, () => {
  for (const [months, expected] of [[1, 1009488793n], [3, 1028737345n], [6, 1058300524n], [12, 1120000000n]] as const) assert.equal(annualGrowthFactor(1200, months), expected);
  assert.equal(annualGrowthFactor(-10000, 3), 0n);
  assert.throws(() => annualGrowthFactor(-10001, 1));
  assert.throws(() => annualGrowthFactor(1200, 13));
});

test("实际持续趋势段复合且金额只进行一次半偶舍入", { timeout: 10000 }, () => {
  const factor = composeGrowthFactors(annualGrowthFactor(1200, 2), annualGrowthFactor(-1000, 1));
  assert.ok(factor > 1000000000n);
  assert.ok(factor < annualGrowthFactor(1200, 3));
  assert.equal(applyGrowthFactor(3n, 1500000000n), 4n);
  assert.equal(applyGrowthFactor(1n, 1500000000n), 2n);
  assert.equal(applyGrowthFactor(-3n, 1500000000n), -4n);
  assert.equal(applyGrowthFactor((1n << 127n) - 1n, 1000000000n), (1n << 127n) - 1n);
  assert.throws(() => applyGrowthFactor((1n << 127n) - 1n, 2000000000n));
});
