const I64_MIN = -9_223_372_036_854_775_808n;
const I64_MAX = 9_223_372_036_854_775_807n;
const CANONICAL_CENTS = /^(?:0|-?[1-9]\d*)$/;

export function parseMoney(value: unknown, path = "Money"): string {
  if (typeof value !== "string" || value.length > 20 || !CANONICAL_CENTS.test(value)) {
    throw new Error(`${path} 金额必须是规范十进制整数分字符串`);
  }
  const parsed = BigInt(value);
  if (parsed < I64_MIN || parsed > I64_MAX) throw new Error(`${path} 金额超出 i64 范围`);
  return value;
}

export function moneyToBigInt(value: string): bigint {
  return BigInt(parseMoney(value));
}

export function moneyFromBigInt(value: bigint): string {
  return parseMoney(value.toString());
}

function integerOperand(value: number | bigint): bigint {
  if (typeof value === "bigint") return value;
  if (!Number.isSafeInteger(value)) throw new Error("金额算术的股数或除数必须是安全整数");
  return BigInt(value);
}

export function addMoney(left: string, right: string): string {
  return moneyFromBigInt(moneyToBigInt(left) + moneyToBigInt(right));
}

export function subtractMoney(left: string, right: string): string {
  return moneyFromBigInt(moneyToBigInt(left) - moneyToBigInt(right));
}

export function multiplyMoney(value: string, quantity: number | bigint): string {
  return moneyFromBigInt(moneyToBigInt(value) * integerOperand(quantity));
}

export function compareMoney(left: string, right: string): number {
  const difference = moneyToBigInt(left) - moneyToBigInt(right);
  return difference < 0n ? -1 : difference > 0n ? 1 : 0;
}

export function centsToYuanText(value: string): string {
  const parsed = moneyToBigInt(value);
  const magnitude = parsed < 0n ? -parsed : parsed;
  return `${parsed < 0n ? "-" : ""}${magnitude / 100n}.${(magnitude % 100n).toString().padStart(2, "0")}`;
}

export function yuanTextToCents(value: string): string {
  if (!/^-?(?:0|[1-9]\d*)(?:\.\d{1,2})?$/.test(value) || value.length > 23) throw new Error("金额输入必须是至多两位小数的规范元字符串");
  const negative = value.startsWith("-");
  const [whole, fraction = ""] = (negative ? value.slice(1) : value).split(".");
  if (whole === undefined) throw new Error("金额输入缺少整数部分");
  const magnitude = BigInt(whole) * 100n + BigInt(fraction.padEnd(2, "0"));
  if (negative && magnitude === 0n) throw new Error("金额输入不允许负零");
  return moneyFromBigInt(negative ? -magnitude : magnitude);
}

export function divideMoneyBankers(value: string, divisor: number | bigint): string {
  const denominator = integerOperand(divisor);
  if (denominator <= 0n) throw new Error("金额除数必须是正整数");
  const parsed = moneyToBigInt(value);
  const magnitude = parsed < 0n ? -parsed : parsed;
  const quotient = magnitude / denominator;
  const twiceRemainder = (magnitude % denominator) * 2n;
  const rounded = quotient + (twiceRemainder > denominator || (twiceRemainder === denominator && quotient % 2n !== 0n) ? 1n : 0n);
  return moneyFromBigInt(parsed < 0n ? -rounded : rounded);
}

export function moneyToChartNumber(value: string): number {
  return Number(moneyToBigInt(value));
}

export function ratioMoney(numerator: string, denominator: string): number {
  const divisor = moneyToBigInt(denominator);
  if (divisor === 0n) throw new Error("金额比例的分母不能为零");
  return Number(moneyToBigInt(numerator)) / Number(divisor);
}
