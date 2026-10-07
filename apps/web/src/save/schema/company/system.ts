import { array, decimal, exact, oneOf, record, string, SaveSchemaError } from "../primitives.ts";
import { parseCompanySpec } from "./accounting/spec.ts";
import { parseCompanySystemConfig } from "./system-config.ts";
import { parsePeriodAmounts, parsePeriodChangeExplanation, parsePeriodGenerationState, settlementMonths } from "./period-generation.ts";
import { annualGrowthFactor, composeGrowthFactors, applyGrowthFactor } from "./annual-growth.ts";
import { parseSimpleFinanceState } from "./simple-finance.ts";
import { simpleAmount, simpleBasisPoints, simpleCivilDate as civilDate } from "./simple-values.ts";
import type { SessionSetup } from "../../../types/engine.ts";

export type { CompanySystemConfig } from "../../../types/generated/CompanySystemConfig";
export type CompanySystem = ReturnType<typeof parseCompanySystemState>;

function rng(value: unknown, path: string) {
  const parsed = record(value, path); exact(parsed, ["state"], path);
  const state = decimal(parsed.state, `${path}.state`);
  if (!/^(0|[1-9]\d*)$/.test(state)) throw new SaveSchemaError(`${path}.state`, "必须为规范 u64 字符串");
  return { state };
}

/**
 * 偏好提案拒绝台账（ADR-0037）：严格持久化字段，缺失显式拒绝；键唯一、
 * 日期不得晚于已推进日，与 engine `SimplePreferenceLedger::validate` 一致。
 */
function parseSimplePreferenceLedger(value: unknown, path: string, companyId: string, advancedThrough: string) {
  const parsed = record(value, path);
  exact(parsed, ["rejections"], path);
  const seen = new Set<string>();
  const rejections = array(parsed.rejections, `${path}.rejections`).map((entry, index) => {
    const rowPath = `${path}.rejections[${index}]`;
    const row = record(entry, rowPath);
    exact(row, ["company", "evaluated_on", "kind", "detail"], rowPath);
    const company = string(row.company, `${rowPath}.company`);
    const evaluatedOn = civilDate(row.evaluated_on, `${rowPath}.evaluated_on`);
    const kind = oneOf(row.kind, `${rowPath}.kind`, ["CashDividend", "StockDistribution"] as const);
    const detail = string(row.detail, `${rowPath}.detail`);
    if (company !== companyId) throw new SaveSchemaError(`${rowPath}.company`, "拒绝台账公司身份与所属公司状态不一致");
    if (evaluatedOn > advancedThrough) throw new SaveSchemaError(`${rowPath}.evaluated_on`, "拒绝台账评估日期晚于公司系统已推进日");
    if (detail.trim().length === 0) throw new SaveSchemaError(`${rowPath}.detail`, "拒绝原因必须非空");
    const key = `${kind}:${evaluatedOn}`;
    if (seen.has(key)) throw new SaveSchemaError(rowPath, "拒绝台账含重复 (类别, 评估日) 键");
    seen.add(key);
    return { company, evaluated_on: evaluatedOn, kind, detail };
  });
  return { rejections };
}

function cyclePeriod(date: string, months: number): readonly [string, string] {
  const year = Number(date.slice(0, 4)), month = Number(date.slice(5, 7));
  const startMonth = Math.floor((month - 1) / months) * months;
  return [new Date(Date.UTC(year, startMonth, 1)).toISOString().slice(0, 10), new Date(Date.UTC(year, startMonth + months, 0)).toISOString().slice(0, 10)];
}

function nextDate(date: string, offset: number): string {
  const result = new Date(`${date}T00:00:00Z`); result.setUTCDate(result.getUTCDate() + offset); return result.toISOString().slice(0, 10);
}

export function parseCompanySystemState(value: unknown, path = "company_system") {
  const root = record(value, path); exact(root, ["issuers", "implementation"], path);
  const issuers = Object.fromEntries(Object.entries(record(root.issuers, `${path}.issuers`)).map(([id, entry]) => {
    const issuer = parseCompanySpec(entry, `${path}.issuers.${id}`);
    if (issuer.id !== id) throw new SaveSchemaError(`${path}.issuers.${id}`, "发行人 key 与身份不一致");
    return [id, issuer];
  }));
  const listed = new Set<string>();
  for (const issuer of Object.values(issuers)) {
    if (issuer.listed_stock !== null) {
      if (listed.has(issuer.listed_stock)) throw new SaveSchemaError(`${path}.issuers`, "发行人股票映射重复");
      listed.add(issuer.listed_stock);
    }
    const visited = new Set<string>([issuer.id]);
    let parent = issuer.group_parent;
    while (parent !== null) {
      if (!Object.hasOwn(issuers, parent)) throw new SaveSchemaError(`${path}.issuers`, "集团母公司身份不存在");
      if (visited.has(parent)) throw new SaveSchemaError(`${path}.issuers`, "集团存在循环引用");
      visited.add(parent); parent = issuers[parent]!.group_parent;
    }
  }
  const implementation = record(root.implementation, `${path}.implementation`);
  if (implementation.mode !== "Simple") throw new SaveSchemaError(`${path}.implementation.mode`, "模式不支持；当前仅恢复 Simple");
  exact(implementation, ["mode", "state"], `${path}.implementation`);
  const statePath = `${path}.implementation.state`;
  const state = record(implementation.state, statePath);
  exact(state, ["config", "history_start", "advanced_through", "environment_change_bp", "environment_rng", "companies", "history"], statePath);
  const config = parseCompanySystemConfig({ mode: "Simple", config: state.config }).config;
  const cycleMonths = settlementMonths(config.settlement_cycle);
  const historyStart = civilDate(state.history_start, `${statePath}.history_start`);
  const advancedThrough = civilDate(state.advanced_through, `${statePath}.advanced_through`);
  const nextAdvancedDate = nextDate(advancedThrough, 1);
  if (!historyStart.endsWith("-01") || historyStart > nextAdvancedDate) throw new SaveSchemaError(`${statePath}.history_start`, "历史必须从自然月首日开始且不能晚于已推进日的次日");
  if (cyclePeriod(historyStart, cycleMonths)[0] !== historyStart) throw new SaveSchemaError(`${statePath}.history_start`, "历史必须从选定自然结算周期首日开始");
  const [currentStart, currentEnd] = cyclePeriod(advancedThrough, cycleMonths);
  const latestEnd = advancedThrough === currentEnd ? currentEnd : nextDate(currentStart, -1);
  const entries = record(state.companies, `${statePath}.companies`); exact(entries, Object.keys(issuers), `${statePath}.companies`);
  const ids = config.companies.map(company => company.company);
  if (ids.length !== Object.keys(issuers).length || ids.some(id => !Object.hasOwn(issuers, id))) throw new SaveSchemaError(`${statePath}.config.companies`, "配置必须与发行人一一对应");
  const companies = Object.fromEntries(Object.entries(entries).map(([id, entry]) => {
    const companyPath = `${statePath}.companies.${id}`;
    const company = record(entry, companyPath); exact(company, ["generation", "finance", "pending_restart", "preference_ledger"], companyPath);
    const generation = parsePeriodGenerationState(company.generation, `${companyPath}.generation`);
    const finance = parseSimpleFinanceState(company.finance, `${companyPath}.finance`);
    if (finance.opening_date !== nextDate(historyStart, -1)) throw new SaveSchemaError(`${companyPath}.finance.opening_date`, "财务开账日期必须为前史开始前一天");
    const expected = config.companies.find(company => company.company === id)!;
    if (expected.kind !== issuers[id]!.kind) throw new SaveSchemaError(`${companyPath}.finance.kind`, "配置与发行人类别不一致");
    validateTrendState(expected.generation, generation, `${companyPath}.generation`);
    if (finance.company !== id || finance.kind !== issuers[id]!.kind || finance.as_of !== latestEnd || JSON.stringify(finance.config) !== JSON.stringify(expected.finance)) throw new SaveSchemaError(`${companyPath}.finance`, "财务身份、类别、配置或最新期间不一致");
    let pendingRestart: [string, string] | null = null;
    if (company.pending_restart !== null) {
      const restart = array(company.pending_restart, `${companyPath}.pending_restart`);
      if (restart.length !== 2) throw new SaveSchemaError(`${companyPath}.pending_restart`, "必须为二元组");
      const amount = simpleAmount(restart[0], `${companyPath}.pending_restart[0]`, true);
      const source = string(restart[1], `${companyPath}.pending_restart[1]`);
      if (amount === "0.00" || source.trim().length === 0 || generation.amounts.revenue !== "0.00") throw new SaveSchemaError(`${companyPath}.pending_restart`, "仅零收入公司可待执行具有正金额与来源的复业");
      pendingRestart = [amount, source];
    }
    const preferenceLedger = parseSimplePreferenceLedger(company.preference_ledger, `${companyPath}.preference_ledger`, id, advancedThrough);
    return [id, { generation, finance, pending_restart: pendingRestart, preference_ledger: preferenceLedger }];
  }));
  const history = array(state.history, `${statePath}.history`).map((entry, index) => {
    const entryPath = `${statePath}.history[${index}]`;
    const item = record(entry, entryPath); exact(item, ["company", "period_start", "period_end", "amounts", "explanation"], entryPath);
    const company = string(item.company, `${entryPath}.company`);
    const start = civilDate(item.period_start, `${entryPath}.period_start`);
    const end = civilDate(item.period_end, `${entryPath}.period_end`);
    if (!Object.hasOwn(issuers, company) || cyclePeriod(start, cycleMonths)[0] !== start || end !== cyclePeriod(start, cycleMonths)[1] || start < historyStart || end > latestEnd) throw new SaveSchemaError(entryPath, "公司或历史期间非法／未来");
    const explanation = parsePeriodChangeExplanation(item.explanation, `${entryPath}.explanation`);
    if (explanation.cycle !== config.settlement_cycle) throw new SaveSchemaError(`${entryPath}.explanation.cycle`, "历史结算周期与所选实现不一致");
    return { company, period_start: start, period_end: end, amounts: parsePeriodAmounts(item.amounts, `${entryPath}.amounts`), explanation };
  });
  const encoder = new TextEncoder();
  const sorted = [...ids].sort((left, right) => {
    const leftBytes = encoder.encode(left), rightBytes = encoder.encode(right);
    for (let index = 0; index < Math.min(leftBytes.length, rightBytes.length); index += 1) {
      if (leftBytes[index] !== rightBytes[index]) return leftBytes[index]! - rightBytes[index]!;
    }
    return leftBytes.length - rightBytes.length;
  });
  let cursor = historyStart;
  let offset = 0;
  const previous = new Map<string, ReturnType<typeof parsePeriodAmounts>>();
  for (const company of config.companies) previous.set(company.company, { revenue: company.generation.initial_revenue, fixed_expense: company.generation.initial_fixed_expense, variable_expense: company.generation.variable_expense.rule === "Growth" ? company.generation.variable_expense.initial_amount : formatAmount(applyBasisPoints(minor(company.generation.initial_revenue), company.generation.variable_expense.ratio_bp)) });
  while (cursor <= latestEnd) {
    const end = cyclePeriod(cursor, cycleMonths)[1];
    for (const id of sorted) {
      const candidate = history[offset];
      if (candidate === undefined || candidate.company !== id || candidate.period_start !== cursor || candidate.period_end !== end || JSON.stringify(candidate.explanation.previous) !== JSON.stringify(previous.get(id))) throw new SaveSchemaError(`${statePath}.history`, "月度历史身份、顺序或金额连续性不一致");
      const parameters = config.companies.find(company => company.company === id)!.generation;
      validateAmounts(parameters, candidate.amounts, candidate.explanation, `${statePath}.history[${offset}]`);
      previous.set(id, candidate.amounts);
      if (end === latestEnd && JSON.stringify(companies[id]!.generation.amounts) !== JSON.stringify(candidate.amounts)) throw new SaveSchemaError(`${statePath}.companies.${id}`, "最新月度金额与历史不一致");
      offset += 1;
    }
    cursor = nextDate(end, 1);
  }
  if (offset !== history.length) throw new SaveSchemaError(`${statePath}.history`, "历史包含多余或未来材料");
  return { issuers, implementation: { mode: "Simple" as const, state: { config, history_start: historyStart, advanced_through: advancedThrough, environment_change_bp: simpleBasisPoints(state.environment_change_bp, `${statePath}.environment_change_bp`), environment_rng: rng(state.environment_rng, `${statePath}.environment_rng`), companies, history } } };
}

function minor(amount: string): bigint { return BigInt(amount.replace(".", "")); }
function formatAmount(value: bigint): string { return `${value / 100n}.${(value % 100n).toString().padStart(2, "0")}`; }
function applyBasisPoints(value: bigint, rate: number): bigint {
  const product = value * BigInt(rate);
  if (product < -(1n << 127n) || product > (1n << 127n) - 1n) throw new SaveSchemaError("期间金额", "基点乘积超出 i128 范围");
  const sign = product < 0n ? -1n : 1n;
  const magnitude = product * sign;
  let quotient = magnitude / 10000n;
  const remainder = magnitude % 10000n;
  if (remainder > 5000n || (remainder === 5000n && quotient % 2n === 1n)) quotient += 1n;
  return quotient * sign;
}

type GenerationParameters = ReturnType<typeof parseCompanySystemConfig>["config"]["companies"][number]["generation"];

function trendContains(config: GenerationParameters["revenue_trend"], rate: number): boolean {
  return config.kind === "Fixed" ? config.annual_growth_bp === rate : rate >= config.annual_growth_min_bp && rate <= config.annual_growth_max_bp;
}

function validateTrendState(parameters: GenerationParameters, state: ReturnType<typeof parsePeriodGenerationState>, path: string): void {
  const validate = (config: GenerationParameters["revenue_trend"], state: ReturnType<typeof parsePeriodGenerationState>["revenue_trend"]) => {
    if (!trendContains(config, state.annual_growth_bp) || (config.kind === "Fixed" ? state.remaining_months !== 0 : state.remaining_months > config.duration_max_months)) throw new SaveSchemaError(path, "保存的年化趋势或自然月进度与配置不一致");
  };
  validate(parameters.revenue_trend, state.revenue_trend); validate(parameters.fixed_expense_trend, state.fixed_expense_trend);
  if (parameters.variable_expense.rule === "RevenueRatio") {
    if (state.variable_expense_trend !== null) throw new SaveSchemaError(path, "比例开支不得携带独立增长趋势");
  } else {
    if (state.variable_expense_trend === null) throw new SaveSchemaError(path, "金额增长开支必须保存独立趋势");
    validate(parameters.variable_expense.trend, state.variable_expense_trend);
  }
}

function validateAmounts(parameters: GenerationParameters, amounts: ReturnType<typeof parsePeriodAmounts>, explanation: ReturnType<typeof parsePeriodChangeExplanation>, path: string) {
  const add = (left: number, right: number) => { const sum = left + right; if (sum < -2147483648 || sum > 2147483647) throw new SaveSchemaError(path, "年化基点合成超出 i32 范围"); return sum; };
  const demand = applyBasisPoints(BigInt(explanation.environment_change_bp), parameters.demand_sensitivity_bp);
  if (demand !== BigInt(explanation.demand_contribution_bp)) throw new SaveSchemaError(path, "环境年化贡献与敏感度不一致");
  const key = { Monthly: "monthly_bp", Quarterly: "quarterly_bp", HalfYear: "half_year_bp", Annual: "annual_bp" }[explanation.cycle] as "monthly_bp" | "quarterly_bp" | "half_year_bp" | "annual_bp";
  if (Math.abs(explanation.revenue_noise_bp) > parameters.revenue_noise[key] || Math.abs(explanation.fixed_expense_noise_bp) > parameters.fixed_expense_noise[key] || Math.abs(explanation.variable_expense_noise_bp) > parameters.variable_expense.noise[key]) throw new SaveSchemaError(path, "随机扰动超出所选结算跨度配置");
  const factor = (config: GenerationParameters["revenue_trend"], segments: ReturnType<typeof parsePeriodChangeExplanation>["revenue_segments"], shift: number, noise: number) => {
    if (segments.reduce((sum, segment) => sum + segment.months, 0) !== settlementMonths(explanation.cycle)) throw new SaveSchemaError(path, "趋势分段未覆盖整个结算期间");
    let result = 1000000000n;
    for (const segment of segments) {
      if (!trendContains(config, segment.annual_growth_bp)) throw new SaveSchemaError(path, "历史趋势分段超出年化配置");
      result = composeGrowthFactors(result, annualGrowthFactor(add(segment.annual_growth_bp, shift), segment.months));
    }
    result += BigInt(noise) * 100000n;
    if (result < 0n || result > (1n << 64n) - 1n) throw new SaveSchemaError(path, "期间随机扰动造成负因子或溢出");
    return result;
  };
  const grownRevenue = applyGrowthFactor(minor(explanation.previous.revenue), factor(parameters.revenue_trend, explanation.revenue_segments, explanation.demand_contribution_bp, explanation.revenue_noise_bp));
  if (explanation.restart_revenue !== null && explanation.previous.revenue !== "0.00") throw new SaveSchemaError(path, "仅零收入可以复业");
  const revenue = explanation.restart_revenue === null ? grownRevenue : minor(explanation.restart_revenue);
  const fixed = applyGrowthFactor(minor(explanation.previous.fixed_expense), factor(parameters.fixed_expense_trend, explanation.fixed_expense_segments, 0, explanation.fixed_expense_noise_bp));
  const rule = parameters.variable_expense;
  let variable: bigint;
  if (rule.rule === "RevenueRatio") {
    if (explanation.variable_expense_segments.length !== 0) throw new SaveSchemaError(path, "比例开支不使用独立金额趋势");
    variable = applyBasisPoints(revenue, add(rule.ratio_bp, explanation.variable_expense_noise_bp));
  } else variable = applyGrowthFactor(minor(explanation.previous.variable_expense), factor(rule.trend, explanation.variable_expense_segments, 0, explanation.variable_expense_noise_bp));
  if (revenue !== minor(amounts.revenue) || fixed !== minor(amounts.fixed_expense) || variable !== minor(amounts.variable_expense)) throw new SaveSchemaError(path, "期间生成金额与解释不一致");
}

export function validateCompanySystemSession(system: ReturnType<typeof parseCompanySystemState>, setup: Pick<SessionSetup, "stocks" | "company_system">, currentDate: string): void {
  if (setup.company_system.mode !== "Simple" || JSON.stringify(system.implementation.state.config) !== JSON.stringify(setup.company_system.config)) throw new SaveSchemaError("company_system", "模式配置与 setup 不一致");
  const previous = new Date(`${currentDate}T00:00:00Z`); previous.setUTCDate(previous.getUTCDate() - 1);
  if (system.implementation.state.advanced_through !== previous.toISOString().slice(0, 10)) throw new SaveSchemaError("company_system.advanced_through", "公司推进日期与自然日时钟不一致");
  exact(system.issuers, setup.stocks.map(stock => `C-${stock.code}`), "company_system.issuers");
  for (const stock of setup.stocks) {
    const id = `C-${stock.code}`, issuer = system.issuers[id]!;
    const company = system.implementation.state.config.companies.find(company => company.company === id)!;
    if (issuer.name !== `虚构上市公司${stock.code}` || issuer.industry !== "listed-simple" || issuer.kind !== company.kind || issuer.listed_stock !== stock.code || BigInt(issuer.issued_shares) !== BigInt(stock.total_shares) || issuer.group_parent !== null) throw new SaveSchemaError(`company_system.issuers.${id}`, "发行人身份与 setup 股票配置不一致");
    const finance = system.implementation.state.companies[id]!.finance;
    for (const [planId, plan] of Object.entries(finance.dividends)) {
      for (const [paymentId, payment] of Object.entries(plan.payments)) {
        if (payment.paid_on > currentDate) throw new SaveSchemaError(`company_system.implementation.state.companies.${id}.finance.dividends.${planId}.payments.${paymentId}.paid_on`, "实际付款日期晚于当前会话日期");
      }
    }
  }
}
