import assert from "node:assert/strict";

function roundRatio(value, numerator, denominator) {
  const product = value * numerator;
  const negative = product < 0n;
  const magnitude = negative ? -product : product;
  const quotient = magnitude / denominator;
  const remainder = magnitude % denominator;
  const rounded = quotient + (remainder * 2n > denominator || (remainder * 2n === denominator && quotient % 2n !== 0n) ? 1n : 0n);
  return negative ? -rounded : rounded;
}

function presentValue(cash, periods, costBp) {
  let result = cash;
  for (let period = 0; period < periods; period += 1) result = roundRatio(result, 10_000n, 10_000n + costBp);
  return result;
}

function calculate(label, initialCash, growthBp, costBp = 1_000n, terminalBp = 0n) {
  const futureCash = [initialCash];
  for (let year = 1; year <= 5; year += 1) futureCash.push(roundRatio(futureCash[year - 1], 10_000n + growthBp, 10_000n));
  const discountedCash = futureCash.slice(1).map((cash, index) => presentValue(cash, index + 1, costBp));
  const finalCash = roundRatio(futureCash[5], 10_000n + terminalBp, 10_000n);
  const terminal = roundRatio(finalCash, 10_000n, costBp - terminalBp);
  const discountedTerminal = presentValue(terminal, 5, costBp);
  const total = discountedCash.reduce((sum, cash) => sum + cash, discountedTerminal);
  return { label, initialCash, growthBp, futureCash: futureCash.slice(1), discountedCash, discountedTerminal, total };
}

assert.equal(roundRatio(5n, 1n, 2n), 2n);
assert.equal(roundRatio(7n, 1n, 2n), 4n);
assert.equal(roundRatio(-5n, 1n, 2n), -2n);
const cases = [
  calculate("甲FY2029修订前", 16_200_000n, 3_000n),
  calculate("甲FY2030修订后", 17_820_000n, 2_500n),
  calculate("乙FY2028修订前", 7_200_000n, -3_000n),
  calculate("乙FY2030修订后", 17_820_000n, -2_000n),
  calculate("同事实到期重估", 17_820_000n, 0n),
  calculate("真实利息现金流路径", 1_000_000n - 50_000n, 0n),
];
console.log(JSON.stringify(cases, (_key, value) => typeof value === "bigint" ? value.toString() : value, 2));
