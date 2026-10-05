import type { SessionSetup, StockSpec } from "../types/engine.ts";
import { settlementMonths } from "../save/schema/company/period-generation.ts";

const U64_MAX = (1n << 64n) - 1n;
const I128_MAX = (1n << 127n) - 1n;

function positiveInteger(value: string, maximum: bigint, label: string): bigint {
  if (!/^[1-9]\d*$/.test(value) || BigInt(value) > maximum) throw new RangeError(`${label} 必须为合法规范正整数字符串`);
  return BigInt(value);
}

function sample(seed: bigint, code: string, label: string, limit: bigint): bigint {
  let state = seed;
  for (const byte of new TextEncoder().encode(`simple-initial:${code}:${label}`)) state = ((state ^ BigInt(byte)) * 1099511628211n) & U64_MAX;
  state = (state + 0x9e3779b97f4a7c15n) & U64_MAX;
  state = ((state ^ (state >> 30n)) * 0xbf58476d1ce4e5b9n) & U64_MAX;
  state = ((state ^ (state >> 27n)) * 0x94d049bb133111ebn) & U64_MAX;
  return (state ^ (state >> 31n)) % limit;
}

function rounded(numerator: bigint, denominator: bigint): bigint {
  let quotient = numerator / denominator;
  const remainder = numerator % denominator;
  if (remainder * 2n > denominator || (remainder * 2n === denominator && quotient % 2n === 1n)) quotient += 1n;
  if (quotient > I128_MAX) throw new RangeError("初始化会计金额超出 i128");
  return quotient;
}

function amount(cents: bigint): string { return `${cents / 100n}.${(cents % 100n).toString().padStart(2, "0")}`; }

export function createCompanyInitialPreset(stock: Pick<StockSpec, "code" | "initial_price" | "total_shares">, seed: bigint, periodMonths: number) {
  if (seed < 0n || seed > U64_MAX) throw new RangeError("初始化 seed 必须为 u64");
  if (![1, 3, 6, 12].includes(periodMonths)) throw new RangeError("初始化期间必须为1／3／6／12自然月");
  if (stock.code.trim().length === 0) throw new RangeError("初始化证券代码不能为空");
  const price = positiveInteger(stock.initial_price, (1n << 63n) - 1n, "期初价格（分）");
  const shares = positiveInteger(stock.total_shares, U64_MAX, "总股本（股）");
  const marketCapitalization = price * shares;
  const virtualPe = 10n + sample(seed, stock.code, "pe", 21n);
  const virtualPb = 10000n + sample(seed, stock.code, "pb", 20001n);
  const revenue = rounded(marketCapitalization * 10000n * 5n * BigInt(periodMonths), virtualPe * 7500n * 12n);
  const fixedExpense = rounded(revenue * 2000n, 10000n);
  const equity = rounded(marketCapitalization * 10000n, virtualPb);
  if (equity === 0n) throw new RangeError("价格锚定的期初权益舍入为零，请明确编辑期初财务，不自动补金额");
  return {
    initial_revenue: amount(revenue), initial_fixed_expense: amount(fixedExpense), initial_equity: amount(equity),
    variable_expense_ratio_bp: 6000, virtual_pe: Number(virtualPe), virtual_pb_bp: Number(virtualPb),
  };
}

export function createPriceAnchoredCompanyConfig(setup: Pick<SessionSetup, "stocks" | "company_system">, seed: bigint): Extract<SessionSetup["company_system"], { mode: "Simple" }> {
  if (setup.company_system.mode !== "Simple") throw new TypeError("价格初始化预设当前仅用于 Simple");
  const result = structuredClone(setup.company_system);
  for (const company of result.config.companies) {
    const stock = setup.stocks.find(stock => `C-${stock.code}` === company.company);
    if (stock === undefined) throw new Error(`价格初始化预设缺少发行人 ${company.company} 的证券`);
    const initial = createCompanyInitialPreset(stock, seed, settlementMonths(result.config.settlement_cycle));
    company.generation.initial_revenue = initial.initial_revenue;
    company.generation.initial_fixed_expense = initial.initial_fixed_expense;
    if (company.generation.variable_expense.rule === "Growth") {
      company.generation.variable_expense.initial_amount = amount(rounded(BigInt(initial.initial_revenue.replace(".", "")) * BigInt(initial.variable_expense_ratio_bp), 10000n));
    }
    company.finance.opening_lines = [{ account: "simple_receivable", side: "Debit", amount: initial.initial_equity }, { account: "4001", side: "Credit", amount: initial.initial_equity }];
  }
  return result;
}
