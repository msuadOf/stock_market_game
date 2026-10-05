import { civilDate, integer, string, SaveSchemaError } from "../primitives.ts";

export function simpleCivilDate(value: unknown, path: string): string {
  const parsed = civilDate(value, path);
  const year = Number(parsed.slice(0, 4));
  if (year < 1900 || year > 2199) throw new SaveSchemaError(path, "超出 CivilDate 算法验证范围 1900–2199");
  return parsed;
}

export function simpleBasisPoints(value: unknown, path: string, minimum = -2147483648): number {
  const parsed = integer(value, path, minimum);
  if (parsed > 2147483647) throw new SaveSchemaError(path, "超出 i32 范围");
  return parsed;
}

export function simpleAmount(value: unknown, path: string, nonnegative = false): string {
  const parsed = string(value, path);
  if (!/^-?(0|[1-9]\d*)\.\d{2}$/.test(parsed) || parsed === "-0.00" || (nonnegative && parsed.startsWith("-"))) throw new SaveSchemaError(path, "必须是规范两位小数元金额");
  const numeric = BigInt(parsed.replace(".", ""));
  if (numeric < -((1n << 127n) - 1n) || numeric > (1n << 127n) - 1n) throw new SaveSchemaError(path, "超出 AccountingAmount 文本解码范围");
  return parsed;
}
