import type { CompanySystemConfig } from "../../../types/generated/CompanySystemConfig";
import { array, exact, integer, oneOf, record, string } from "../primitives.ts";
import { parsePeriodGenerationParameters, parsePeriodNoiseConfig, parseSettlementCycle } from "./period-generation.ts";
import { parseSimpleFinanceConfig } from "./simple-finance-config.ts";

type SimpleConfig = Extract<CompanySystemConfig, { mode: "Simple" }>;

function boundedInteger(value: unknown, path: string, minimum: number, maximum: number): number {
  const parsed = integer(value, path, minimum);
  if (parsed > maximum) throw new RangeError(`${path} 不能超过 ${maximum}`);
  return parsed;
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
    exact(company, ["company", "kind", "generation", "finance"], path);
    const id = string(company.company, `${path}.company`);
    if (id.trim().length === 0 || seen.has(id)) throw new TypeError(`${path}.company 为空或重复`);
    seen.add(id);
    return {
      company: id,
      kind: oneOf(company.kind, `${path}.kind`, ["Industrial", "Bank", "Insurance", "RealEstate"] as const),
      generation: parsePeriodGenerationParameters(company.generation, `${path}.generation`),
      finance: parseSimpleFinanceConfig(company.finance, `${path}.finance`),
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
