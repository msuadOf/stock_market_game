import type { CompanySystemConfig } from "../types/generated/CompanySystemConfig.ts";
import { array, exact, integer, record } from "../save/schema/primitives.ts";
import { parseCompanySystemConfig } from "../save/schema/setup-company-options.ts";

export interface AmountRange { readonly min: string; readonly max: string }
export interface CompanyPeriodBasis {
  readonly company: number;
  readonly revenue: AmountRange;
  readonly fixedExpense: AmountRange;
  readonly variableExpense: AmountRange | null;
}
const U64_MAX = 18446744073709551615n;
const I128_MAX = (1n << 127n) - 1n;

function fail(path: string, message: string): never { throw new RangeError(`${path}：${message}`); }
function cents(value: unknown, path: string): bigint {
  if (typeof value !== "string" || !/^(0|[1-9]\d*)\.\d{2}$/.test(value)) return fail(path, "必须为规范非负两位小数元字符串");
  const result = BigInt(value.replace(".", ""));
  if (result > I128_MAX) return fail(path, "金额超出 i128 分上界");
  return result;
}
function sample(seed: bigint, company: number, tag: string): bigint {
  let state = seed;
  for (const byte of new TextEncoder().encode(`simple-initial:${company}:${tag}`)) state = ((state ^ BigInt(byte)) * 1099511628211n) & U64_MAX;
  state = (state + 0x9e3779b97f4a7c15n) & U64_MAX;
  state = ((state ^ (state >> 30n)) * 0xbf58476d1ce4e5b9n) & U64_MAX;
  state = ((state ^ (state >> 27n)) * 0x94d049bb133111ebn) & U64_MAX;
  return state ^ (state >> 31n);
}
function periodAmount(value: unknown, seed: bigint, company: number, tag: string, path: string): string {
  const range = record(value, path);
  exact(range, ["min", "max"], path);
  const minimum = cents(range.min, `${path}.min`);
  const maximum = cents(range.max, `${path}.max`);
  const width = maximum - minimum + 1n;
  if (width < 1n || width > U64_MAX + 1n) return fail(path, "闭区间宽度必须为 1..=2^64 分");
  const result = minimum + sample(seed, company, tag) % width;
  return `${result / 100n}.${(result % 100n).toString().padStart(2, "0")}`;
}

/** 仅生成显式期间范围内的虚拟参数；Rust 创建门禁负责完整领域校验。 */
export function createCompanyInitialPreset(template: CompanySystemConfig, seed: bigint, bases: readonly CompanyPeriodBasis[]): Extract<CompanySystemConfig, { mode: "Simple" }> {
  if (typeof seed !== "bigint" || seed < 0n || seed > U64_MAX) fail("seed", "必须为 u64 bigint");
  const result = parseCompanySystemConfig(template, "公司系统");
  if (result.mode !== "Simple") throw new Error("Simulation 尚未实现");
  const companies = new Set<number>();
  for (const entry of result.config.companies) {
    if (companies.has(entry.company)) fail("公司系统.config.companies", `重复 company ${entry.company}`);
    companies.add(entry.company);
  }
  const ranges = new Map<number, Record<string, unknown>>();
  for (const [index, value] of array(bases, "periodBases").entries()) {
    const path = `periodBases[${index}]`;
    const basis = record(value, path);
    exact(basis, ["company", "revenue", "fixedExpense", "variableExpense"], path);
    const company = integer(basis.company, `${path}.company`, 0);
    if (company > 4294967295 || !companies.has(company) || ranges.has(company)) fail(`${path}.company`, "必须为模板中唯一的 numeric u32 CompanyId");
    ranges.set(company, basis);
  }
  for (const company of result.config.companies) {
    const basis = ranges.get(company.company);
    if (basis === undefined) fail(`periodBases.company[${company.company}]`, "公司缺少显式期间范围");
    const path = `periodBases.company[${company.company}]`;
    const generation = company.generation;
    generation.initial_revenue = periodAmount(basis.revenue, seed, company.company, "revenue", `${path}.revenue`);
    generation.initial_fixed_expense = periodAmount(basis.fixedExpense, seed, company.company, "fixedExpense", `${path}.fixedExpense`);
    if (generation.variable_expense.rule === "Growth") {
      generation.variable_expense.initial_amount = periodAmount(basis.variableExpense, seed, company.company, "variableExpense", `${path}.variableExpense`);
    } else if (basis.variableExpense !== null) fail(`${path}.variableExpense`, "RevenueRatio 必须显式为 null");
  }
  // 不检查期初平衡、发行人映射或全部联合域；不把结构预检冒充 Rust 创建成功。
  return result;
}
