import type { SessionSetup } from "../../../types/generated/SessionSetup.ts";
import { parsePlanU64 } from "../../../utils/plan-identity.ts";
import { array, boolean, civilDate, decimal, exact, integer, money, oneOf, record, SaveSchemaError, string } from "../primitives.ts";
import { companyKey } from "../personal/common.ts";
import { parseSimpleConfig, parseSimpleFinanceConfig } from "../setup-company-options.ts";
import { parseBooks } from "./accounting/books.ts";
import { parseCompanySpec } from "./accounting/spec.ts";
import { parseClosingRegistry } from "./closing.ts";
import { parseGroups } from "./groups.ts";
import { parseCompanyOperations } from "./operations.ts";
import { parseIncomeTaxPosition } from "./books/industrial.ts";
import { parseAccountingPeriod } from "./accounting/period.ts";
import { amount, i128, instant } from "./value.ts";
import { STATE_SHAPES, type StateRule, type CompanySystemWire, type SessionCorporateActionsWire } from "./state-shapes.ts";

export type SavedSessionCompanies =
  | { LegacyOperations: { operations: ReturnType<typeof parseCompanyOperations>; closing: ReturnType<typeof parseClosingRegistry>; groups: ReturnType<typeof parseGroups> } }
  | { CompanySystem: { system: CompanySystemWire; corporate_actions: SessionCorporateActionsWire } };

function fail(path: string, detail: string): never { throw new SaveSchemaError(path, detail); }
function numeric(value: unknown, path: string, minimum: number, maximum: number): number {
  const parsed = integer(value, path, minimum);
  if (parsed > maximum) fail(path, `超出 ${maximum} 上限`);
  return parsed;
}
function decode(rule: StateRule, value: unknown, path: string): unknown {
  if (typeof rule === "string") {
    switch (rule) {
      case "string": return string(value, path);
      case "boolean": return boolean(value, path);
      case "date": { const result = civilDate(value, path); const year = Number(result.slice(0, 4)); if (year < 1900 || year > 2199) fail(path, "日期超出1900..2199范围"); return result; }
      case "instant": return instant(value, path);
      case "money": return money(value, path);
      case "amount": return amount(value, path);
      case "canonicalU64": case "account": return parsePlanU64(value, path);
      case "decimalU64": return decimal(value, path);
      case "canonicalI128": { const parsed = i128(value, path); if (!/^(?:0|-?[1-9][0-9]*)$/.test(parsed)) fail(path, "必须是规范 i128 十进制字符串"); return parsed; }
      case "nonnegativeI128": { const parsed = i128(value, path); if (!/^(?:0|[1-9][0-9]*)$/.test(parsed) || BigInt(parsed) > (1n << 127n) - 1n) fail(path, "必须是规范非负 i128 字符串"); return parsed; }
      case "u8": return numeric(value, path, 0, 255);
      case "u16": return numeric(value, path, 0, 65535);
      case "u32": return numeric(value, path, 0, 4294967295);
      case "u64number": return integer(value, path, 0);
      case "i8": return numeric(value, path, -128, 127);
      case "i16": return numeric(value, path, -32768, 32767);
      case "i32": return numeric(value, path, -2147483648, 2147483647);
      case "i64": case "i128": return integer(value, path);
      case "period": return parseAccountingPeriod(value, path);
      case "companyKind": return oneOf(value, path, ["Industrial", "Bank", "Insurance", "RealEstate"] as const);
      case "companySpec": return parseCompanySpec(value, path);
      case "books": return parseBooks(value, path);
      case "closing": return parseClosingRegistry(value, path);
      case "operations": return parseCompanyOperations(value, path);
      case "group": return parseGroups([value], path)[0];
      case "incomeTax": return parseIncomeTaxPosition(value, path);
      case "simpleFinanceConfig": return parseSimpleFinanceConfig(value, path);
      case "simpleConfig": return parseSimpleConfig(value, path);
      case "rng": { const parsed = record(value, path); exact(parsed, ["state"], path); return { state: decimal(parsed.state, `${path}.state`) }; }
      case "issuerMap": {
        return Object.fromEntries(Object.entries(record(value, path)).map(([key, entry]) => { companyKey(key, `${path}.${key}`); return [key, parseCompanySpec(entry, `${path}.${key}`)]; }));
      }
      default: return fail(path, `未知内部codec ${rule}`);
    }
  }
  const [kind, payload, third] = rule;
  switch (kind) {
    case "ref": {
      const shape = STATE_SHAPES[String(payload)]; if (shape === undefined) fail(path, `缺少内部类型 ${String(payload)}`);
      const result = decode(shape, value, path);
      if (payload === "ExactDividendTaxAmount") {
        const fraction = result as { numerator: string; denominator: string };
        let numerator = BigInt(fraction.numerator), denominator = BigInt(fraction.denominator);
        if (denominator === 0n) fail(`${path}.denominator`, "精确税额分母必须非零");
        while (denominator !== 0n) { const remainder = numerator % denominator; numerator = denominator; denominator = remainder; }
        if (numerator !== 1n) fail(path, "精确税额分数必须已约分（零必须为0/1）");
      }
      return result;
    }
    case "literal": if (value !== payload) fail(path, `必须是 ${String(payload)}`); return value;
    case "nullable": return value === null ? null : decode(payload as StateRule, value, path);
    case "array": return array(value, path).map((item, index) => decode(payload as StateRule, item, `${path}[${index}]`));
    case "tuple": { const parsers = payload as StateRule[]; const tuple = array(value, path); if (tuple.length !== parsers.length) fail(path, `必须是${parsers.length}元组`); return parsers.map((parser, index) => decode(parser, tuple[index], `${path}[${index}]`)); }
    case "object": {
      const fields = payload as Record<string, StateRule>; const parsed = record(value, path); exact(parsed, Object.keys(fields), path);
      return Object.fromEntries(Object.entries(fields).map(([name, field]) => [name, decode(field, parsed[name], `${path}.${name}`)]));
    }
    case "map": {
      return Object.fromEntries(Object.entries(record(value, path)).map(([key, entry]) => {
        const keyRule = payload as StateRule;
        if (typeof keyRule === "string" && /^i(?:8|16|32|64|128)$/.test(keyRule)) {
          if (!/^(?:0|-?[1-9][0-9]*)$/.test(key)) fail(`${path}.${key}`, "必须是规范整数键"); decode(keyRule, Number(key), `${path}.${key}`);
        } else if (keyRule === "u32") companyKey(key, `${path}.${key}`);
        else if (keyRule === "u64number") parsePlanU64(key, `${path}.${key}`);
        else decode(keyRule, key, `${path}.${key}`);
        return [key, decode(third as StateRule, entry, `${path}.${key}`)];
      }));
    }
    case "enum": {
      const variants = payload as Record<string, StateRule | null>;
      if (typeof value === "string") { if (!Object.hasOwn(variants, value) || variants[value] !== null) fail(path, "包含无效枚举变体"); return value; }
      const parsed = record(value, path); const keys = Object.keys(parsed);
      if (keys.length !== 1) fail(`${path}.${keys.find(key => !Object.hasOwn(variants, key)) ?? "variant"}`, "必须是一个已知枚举变体");
      if (!Object.hasOwn(variants, keys[0]) || variants[keys[0]] === null) fail(`${path}.${keys[0]}`, "包含无效枚举变体");
      return { [keys[0]]: decode(variants[keys[0]]!, parsed[keys[0]], `${path}.${keys[0]}`) };
    }
    default: return fail(path, `未知内部结构 ${kind}`);
  }
}

export function parseCompanySystemState(value: unknown, path: string): CompanySystemWire {
  return decode(["ref", "CompanySystem"], value, path) as CompanySystemWire;
}
export function parseSessionCorporateActions(value: unknown, path: string): SessionCorporateActionsWire {
  return decode(["ref", "SessionCorporateActions"], value, path) as SessionCorporateActionsWire;
}
export function parseSavedSessionCompanies(value: unknown, setup: Pick<SessionSetup, "company_system">, path = "company_state"): SavedSessionCompanies {
  const parsed = record(value, path);
  const tag = setup.company_system === undefined ? "LegacyOperations" : "CompanySystem";
  if (setup.company_system?.mode === "Simulation") fail(`${path}.CompanySystem`, "Simulation模式尚不支持");
  exact(parsed, [tag], path);
  const payload = record(parsed[tag], `${path}.${tag}`);
  if (tag === "LegacyOperations") {
    const fieldPath = `${path}.LegacyOperations`; exact(payload, ["operations", "closing", "groups"], fieldPath);
    return { LegacyOperations: { operations: parseCompanyOperations(payload.operations, `${fieldPath}.operations`), closing: parseClosingRegistry(payload.closing, `${fieldPath}.closing`), groups: parseGroups(payload.groups, `${fieldPath}.groups`) } };
  }
  const fieldPath = `${path}.CompanySystem`; exact(payload, ["system", "corporate_actions"], fieldPath);
  return { CompanySystem: { system: parseCompanySystemState(payload.system, `${fieldPath}.system`), corporate_actions: parseSessionCorporateActions(payload.corporate_actions, `${fieldPath}.corporate_actions`) } };
}
