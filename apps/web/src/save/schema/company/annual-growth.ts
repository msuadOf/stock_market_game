const SCALE = 1000000000n;
const U64_MAX = (1n << 64n) - 1n;
const I128_MIN = -(1n << 127n);
const I128_MAX = (1n << 127n) - 1n;

function rounded(numerator: bigint, denominator: bigint): bigint {
  let quotient = numerator / denominator;
  const remainder = numerator % denominator;
  if (remainder * 2n > denominator || (remainder * 2n === denominator && quotient % 2n !== 0n)) quotient += 1n;
  return quotient;
}

export function annualGrowthFactor(annualBp: number, months: number): bigint {
  if (!Number.isInteger(annualBp) || annualBp < -10000 || annualBp > 2147483647 || !Number.isInteger(months) || months < 1 || months > 12) throw new RangeError("年化增长率或实际持续月数非法");
  const annualScaled = (BigInt(annualBp) + 10000n) * (SCALE / 10000n);
  const target = annualScaled ** BigInt(months) * SCALE ** BigInt(12 - months);
  let low = 0n, high = 1n;
  while (high ** 12n <= target) high *= 2n;
  while (high - low > 1n) {
    const midpoint = (low + high) / 2n;
    if (midpoint ** 12n <= target) low = midpoint; else high = midpoint;
  }
  const midpoint = (low * 2n + 1n) ** 12n, scaledTarget = target << 12n;
  const result = midpoint < scaledTarget || (midpoint === scaledTarget && low % 2n !== 0n) ? low + 1n : low;
  if (result > U64_MAX) throw new RangeError("复利定点因子超出 u64");
  return result;
}

export function composeGrowthFactors(previous: bigint, next: bigint): bigint {
  if (previous < 0n || previous > U64_MAX || next < 0n || next > U64_MAX) throw new RangeError("复利定点因子超出 u64");
  const result = rounded(previous * next, SCALE);
  if (result > U64_MAX) throw new RangeError("复合定点因子超出 u64");
  return result;
}

export function applyGrowthFactor(amount: bigint, factor: bigint): bigint {
  if (amount < I128_MIN || amount > I128_MAX || factor < 0n || factor > U64_MAX) throw new RangeError("复利金额或因子超出范围");
  const negative = amount < 0n;
  const magnitude = negative ? -amount : amount;
  const roundedAmount = rounded(magnitude * factor, SCALE);
  const result = negative ? -roundedAmount : roundedAmount;
  if (result < I128_MIN || result > I128_MAX) throw new RangeError("复利金额超出 AccountingAmount 范围");
  return result;
}
