import type { PeriodGenerationParameters } from "../../../types/generated/PeriodGenerationParameters";
import type { AnnualTrendConfig } from "../../../types/generated/AnnualTrendConfig";
import type { PeriodNoiseConfig } from "../../../types/generated/PeriodNoiseConfig";
import type { SettlementCycle } from "../../../types/generated/SettlementCycle";
import { array, decimal, exact, integer, oneOf, record, string, SaveSchemaError } from "../primitives.ts";
import { simpleAmount, simpleBasisPoints } from "./simple-values.ts";

export function parseSettlementCycle(value: unknown, path = "settlement_cycle"): SettlementCycle {
  return oneOf(value, path, ["Monthly", "Quarterly", "HalfYear", "Annual"] as const);
}
export function settlementMonths(cycle: SettlementCycle): number { return { Monthly: 1, Quarterly: 3, HalfYear: 6, Annual: 12 }[cycle]; }

function months(value: unknown, path: string, maximum: number, minimum = 1): number {
  const parsed = integer(value, path, minimum);
  if (parsed > maximum) throw new SaveSchemaError(path, `不能超过 ${maximum} 自然月`);
  return parsed;
}
function annualTrend(value: unknown, path: string): AnnualTrendConfig {
  const parsed = record(value, path);
  if (parsed.kind === "Fixed") {
    exact(parsed, ["kind", "annual_growth_bp"], path);
    return { kind: "Fixed", annual_growth_bp: simpleBasisPoints(parsed.annual_growth_bp, `${path}.annual_growth_bp`, -10000) };
  }
  if (parsed.kind === "Persistent") {
    exact(parsed, ["kind", "annual_growth_min_bp", "annual_growth_max_bp", "duration_min_months", "duration_max_months"], path);
    const low = simpleBasisPoints(parsed.annual_growth_min_bp, `${path}.annual_growth_min_bp`, -10000);
    const high = simpleBasisPoints(parsed.annual_growth_max_bp, `${path}.annual_growth_max_bp`, -10000);
    const minimum = months(parsed.duration_min_months, `${path}.duration_min_months`, 65535);
    const maximum = months(parsed.duration_max_months, `${path}.duration_max_months`, 65535);
    if (low > high || minimum > maximum) throw new SaveSchemaError(path, "趋势区间或自然持续时间上下界非法");
    return { kind: "Persistent", annual_growth_min_bp: low, annual_growth_max_bp: high, duration_min_months: minimum, duration_max_months: maximum };
  }
  throw new SaveSchemaError(`${path}.kind`, "必须为 Fixed 或 Persistent");
}
export function parsePeriodNoiseConfig(value: unknown, path: string): PeriodNoiseConfig {
  const parsed = record(value, path); exact(parsed, ["monthly_bp", "quarterly_bp", "half_year_bp", "annual_bp"], path);
  return { monthly_bp: simpleBasisPoints(parsed.monthly_bp, `${path}.monthly_bp`, 0), quarterly_bp: simpleBasisPoints(parsed.quarterly_bp, `${path}.quarterly_bp`, 0), half_year_bp: simpleBasisPoints(parsed.half_year_bp, `${path}.half_year_bp`, 0), annual_bp: simpleBasisPoints(parsed.annual_bp, `${path}.annual_bp`, 0) };
}

export function parsePeriodGenerationParameters(value: unknown, path = "期间生成参数"): PeriodGenerationParameters {
  const parsed = record(value, path);
  exact(parsed, ["initial_revenue", "initial_fixed_expense", "revenue_trend", "fixed_expense_trend", "revenue_noise", "fixed_expense_noise", "demand_sensitivity_bp", "variable_expense"], path);
  const variablePath = `${path}.variable_expense`, variable = record(parsed.variable_expense, variablePath);
  let variableExpense: PeriodGenerationParameters["variable_expense"];
  if (variable.rule === "RevenueRatio") {
    exact(variable, ["rule", "ratio_bp", "noise"], variablePath);
    const ratio = simpleBasisPoints(variable.ratio_bp, `${variablePath}.ratio_bp`, 0), variability = parsePeriodNoiseConfig(variable.noise, `${variablePath}.noise`);
    if (Object.values(variability).some(bound => ratio < bound || ratio + bound > 2147483647)) throw new SaveSchemaError(variablePath, "各结算跨度开支比例区间必须非负且不溢出");
    variableExpense = { rule: "RevenueRatio", ratio_bp: ratio, noise: variability };
  } else if (variable.rule === "Growth") {
    exact(variable, ["rule", "initial_amount", "trend", "noise"], variablePath);
    variableExpense = { rule: "Growth", initial_amount: simpleAmount(variable.initial_amount, `${variablePath}.initial_amount`, true), trend: annualTrend(variable.trend, `${variablePath}.trend`), noise: parsePeriodNoiseConfig(variable.noise, `${variablePath}.noise`) };
  } else throw new SaveSchemaError(`${variablePath}.rule`, "必须为 RevenueRatio 或 Growth");
  return { initial_revenue: simpleAmount(parsed.initial_revenue, `${path}.initial_revenue`, true), initial_fixed_expense: simpleAmount(parsed.initial_fixed_expense, `${path}.initial_fixed_expense`, true), revenue_trend: annualTrend(parsed.revenue_trend, `${path}.revenue_trend`), fixed_expense_trend: annualTrend(parsed.fixed_expense_trend, `${path}.fixed_expense_trend`), revenue_noise: parsePeriodNoiseConfig(parsed.revenue_noise, `${path}.revenue_noise`), fixed_expense_noise: parsePeriodNoiseConfig(parsed.fixed_expense_noise, `${path}.fixed_expense_noise`), demand_sensitivity_bp: simpleBasisPoints(parsed.demand_sensitivity_bp, `${path}.demand_sensitivity_bp`), variable_expense: variableExpense };
}

export function parsePeriodAmounts(value: unknown, path = "期间金额") {
  const parsed = record(value, path); exact(parsed, ["revenue", "fixed_expense", "variable_expense"], path);
  return { revenue: simpleAmount(parsed.revenue, `${path}.revenue`, true), fixed_expense: simpleAmount(parsed.fixed_expense, `${path}.fixed_expense`, true), variable_expense: simpleAmount(parsed.variable_expense, `${path}.variable_expense`, true) };
}
function trendState(value: unknown, path: string) {
  const parsed = record(value, path); exact(parsed, ["annual_growth_bp", "remaining_months"], path);
  return { annual_growth_bp: simpleBasisPoints(parsed.annual_growth_bp, `${path}.annual_growth_bp`, -10000), remaining_months: months(parsed.remaining_months, `${path}.remaining_months`, 65535, 0) };
}
export function parsePeriodGenerationState(value: unknown, path = "期间生成状态") {
  const parsed = record(value, path); exact(parsed, ["amounts", "rng", "revenue_trend", "fixed_expense_trend", "variable_expense_trend"], path);
  const rng = record(parsed.rng, `${path}.rng`); exact(rng, ["state"], `${path}.rng`);
  const state = decimal(rng.state, `${path}.rng.state`);
  if (!/^(0|[1-9]\d*)$/.test(state)) throw new SaveSchemaError(`${path}.rng.state`, "必须为规范 u64 字符串");
  return { amounts: parsePeriodAmounts(parsed.amounts, `${path}.amounts`), rng: { state }, revenue_trend: trendState(parsed.revenue_trend, `${path}.revenue_trend`), fixed_expense_trend: trendState(parsed.fixed_expense_trend, `${path}.fixed_expense_trend`), variable_expense_trend: parsed.variable_expense_trend === null ? null : trendState(parsed.variable_expense_trend, `${path}.variable_expense_trend`) };
}
export function parsePeriodChangeExplanation(value: unknown, path = "期间变化解释") {
  const parsed = record(value, path);
  exact(parsed, ["previous", "cycle", "environment_change_bp", "demand_contribution_bp", "revenue_segments", "fixed_expense_segments", "variable_expense_segments", "revenue_noise_bp", "fixed_expense_noise_bp", "variable_expense_noise_bp", "restart_revenue", "restart_source"], path);
  const cycle = parseSettlementCycle(parsed.cycle, `${path}.cycle`), span = settlementMonths(cycle);
  const segments = (name: "revenue_segments" | "fixed_expense_segments" | "variable_expense_segments") => {
    const rows = array(parsed[name], `${path}.${name}`).map((value, index) => {
      const rowPath = `${path}.${name}[${index}]`, row = record(value, rowPath); exact(row, ["annual_growth_bp", "months"], rowPath);
      return { annual_growth_bp: simpleBasisPoints(row.annual_growth_bp, `${rowPath}.annual_growth_bp`, -10000), months: months(row.months, `${rowPath}.months`, 12) };
    });
    if ((name !== "variable_expense_segments" || rows.length !== 0) && rows.reduce((total, row) => total + row.months, 0) !== span) throw new SaveSchemaError(`${path}.${name}`, "趋势段必须覆盖本次完整结算跨度");
    return rows;
  };
  const restartRevenue = parsed.restart_revenue === null ? null : simpleAmount(parsed.restart_revenue, `${path}.restart_revenue`, true);
  const restartSource = parsed.restart_source === null ? null : string(parsed.restart_source, `${path}.restart_source`);
  if ((restartRevenue === null) !== (restartSource === null) || (restartSource !== null && (restartSource.trim().length === 0 || restartRevenue === "0.00"))) throw new SaveSchemaError(path, "复业正金额与明确来源必须成对");
  return { previous: parsePeriodAmounts(parsed.previous, `${path}.previous`), cycle, environment_change_bp: simpleBasisPoints(parsed.environment_change_bp, `${path}.environment_change_bp`), demand_contribution_bp: simpleBasisPoints(parsed.demand_contribution_bp, `${path}.demand_contribution_bp`), revenue_segments: segments("revenue_segments"), fixed_expense_segments: segments("fixed_expense_segments"), variable_expense_segments: segments("variable_expense_segments"), revenue_noise_bp: simpleBasisPoints(parsed.revenue_noise_bp, `${path}.revenue_noise_bp`), fixed_expense_noise_bp: simpleBasisPoints(parsed.fixed_expense_noise_bp, `${path}.fixed_expense_noise_bp`), variable_expense_noise_bp: simpleBasisPoints(parsed.variable_expense_noise_bp, `${path}.variable_expense_noise_bp`), restart_revenue: restartRevenue, restart_source: restartSource };
}
