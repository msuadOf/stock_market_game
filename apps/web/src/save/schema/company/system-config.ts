import type { CompanySystemConfig } from "../../../types/generated/CompanySystemConfig";
import type { SimpleCompanyPreferences } from "../../../types/generated/SimpleCompanyPreferences";
import { array, exact, integer, money, nullable, oneOf, record, string } from "../primitives.ts";
import { parsePeriodGenerationParameters, parsePeriodNoiseConfig, parseSettlementCycle } from "./period-generation.ts";
import { parseSimpleFinanceConfig } from "./simple-finance-config.ts";

type SimpleConfig = Extract<CompanySystemConfig, { mode: "Simple" }>;

function boundedInteger(value: unknown, path: string, minimum: number, maximum: number): number {
  const parsed = integer(value, path, minimum);
  if (parsed > maximum) throw new RangeError(`${path} 不能超过 ${maximum}`);
  return parsed;
}

/** 正数（分）门槛：复用 Money 的规范分字符串解析并要求严格为正。 */
function positiveCents(value: unknown, path: string): string {
  const parsed = money(value, path);
  if (parsed === "0" || parsed.startsWith("-")) throw new RangeError(`${path} 必须为正数（分）`);
  return parsed;
}

/**
 * 公司行为偏好（ADR-0037）：严格持久化字段，缺失或越域显式拒绝。
 * 数值域与 engine `SimpleCompanyPreferences::validate` 完全一致。
 */
export function parseSimpleCompanyPreferences(value: unknown, path: string): SimpleCompanyPreferences {
  const parsed = record(value, path);
  exact(parsed, ["cash_dividend", "stock_distribution"], path);
  const cash = nullable(parsed.cash_dividend, `${path}.cash_dividend`, (nested, nestedPath) => {
    const item = record(nested, nestedPath);
    exact(item, ["target_payout_bp", "min_distributable_profit", "cycles_between_proposals"], nestedPath);
    return {
      target_payout_bp: boundedInteger(item.target_payout_bp, `${nestedPath}.target_payout_bp`, 1, 10000),
      min_distributable_profit: positiveCents(item.min_distributable_profit, `${nestedPath}.min_distributable_profit`),
      cycles_between_proposals: boundedInteger(item.cycles_between_proposals, `${nestedPath}.cycles_between_proposals`, 1, 65535),
    };
  });
  const stock = nullable(parsed.stock_distribution, `${path}.stock_distribution`, (nested, nestedPath) => {
    const item = record(nested, nestedPath);
    exact(item, ["min_distributable_profit", "shares_per_existing_share_micros", "max_cumulative_expansion_micros", "cycles_between_proposals"], nestedPath);
    return {
      min_distributable_profit: positiveCents(item.min_distributable_profit, `${nestedPath}.min_distributable_profit`),
      shares_per_existing_share_micros: boundedInteger(item.shares_per_existing_share_micros, `${nestedPath}.shares_per_existing_share_micros`, 1, 10000000),
      max_cumulative_expansion_micros: boundedInteger(item.max_cumulative_expansion_micros, `${nestedPath}.max_cumulative_expansion_micros`, 1, 1000000000),
      cycles_between_proposals: boundedInteger(item.cycles_between_proposals, `${nestedPath}.cycles_between_proposals`, 1, 65535),
    };
  });
  return { cash_dividend: cash, stock_distribution: stock };
}

export function parseCompanySystemConfig(value: unknown): SimpleConfig {
  const root = record(value, "公司系统");
  if (root.mode !== "Simple") throw new TypeError(`公司系统模式 ${String(root.mode)} 不支持；当前仅实现 Simple，Simulation 由独立分支实现`);
  exact(root, ["mode", "config"], "公司系统");
  const config = record(root.config, "公司系统.config");
  exact(config, ["environment", "companies", "settlement_cycle", "prehistory_periods"], "公司系统.config");
  const environment = record(config.environment, "公司系统.config.environment");
  exact(environment, ["initial_change_bp", "persistence_bp", "noise"], "公司系统.config.environment");
  const seen = new Set<string>();
  const companies = array(config.companies, "公司系统.config.companies").map((entry, index) => {
    const path = `公司系统.config.companies[${index}]`;
    const company = record(entry, path);
    exact(company, ["company", "kind", "generation", "finance", "preferences"], path);
    const id = string(company.company, `${path}.company`);
    if (id.trim().length === 0 || seen.has(id)) throw new TypeError(`${path}.company 为空或重复`);
    seen.add(id);
    return {
      company: id,
      kind: oneOf(company.kind, `${path}.kind`, ["Industrial", "Bank", "Insurance", "RealEstate"] as const),
      generation: parsePeriodGenerationParameters(company.generation, `${path}.generation`),
      finance: parseSimpleFinanceConfig(company.finance, `${path}.finance`),
      preferences: parseSimpleCompanyPreferences(company.preferences, `${path}.preferences`),
    };
  });
  return { mode: "Simple", config: {
    environment: {
      initial_change_bp: boundedInteger(environment.initial_change_bp, "公司系统.config.environment.initial_change_bp", -2147483648, 2147483647),
      persistence_bp: boundedInteger(environment.persistence_bp, "公司系统.config.environment.persistence_bp", 0, 10000),
      noise: parsePeriodNoiseConfig(environment.noise, "公司系统.config.environment.noise"),
    },
    companies,
    settlement_cycle: parseSettlementCycle(config.settlement_cycle),
    prehistory_periods: boundedInteger(config.prehistory_periods, "公司系统.config.prehistory_periods", 0, 65535),
  } };
}
