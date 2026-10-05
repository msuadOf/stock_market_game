export const U128_MAX = 340_282_366_920_938_463_463_374_607_431_768_211_455n;

export function parseTurnoverCents(value: unknown, path: string): string {
  if (typeof value !== "string" || value.length > 39 || !/^(0|[1-9]\d*)$/.test(value)) {
    throw new RangeError(`${path} 必须是规范非负十进制分字符串`);
  }
  if (BigInt(value) > U128_MAX) throw new RangeError(`${path} 超出 u128 范围`);
  return value;
}
