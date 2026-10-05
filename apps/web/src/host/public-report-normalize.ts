import type { PublicReportAvailability, PublicReportAvailabilityQuery, PublicReportPage, PublicReportSummary } from "../types/engine.ts";

type Validator = (value: unknown, path: string) => void;
type Shape = Record<string, Validator>;

function record(value: unknown, path: string): Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) throw new TypeError(`${path} 必须是对象`);
  return value as Record<string, unknown>;
}

function object(shape: Shape): Validator {
  return (value, path) => {
    const fields = record(value, path);
    const keys = Object.keys(fields);
    if (keys.length !== Object.keys(shape).length || keys.some((key) => !Object.hasOwn(shape, key))) {
      throw new TypeError(`${path} 字段不符合公共 DTO 契约`);
    }
    for (const [key, validate] of Object.entries(shape)) validate(fields[key], `${path}.${key}`);
  };
}

function array(validate: Validator): Validator {
  return (value, path) => {
    if (!Array.isArray(value)) throw new TypeError(`${path} 必须是数组`);
    value.forEach((entry, index) => validate(entry, `${path}[${index}]`));
  };
}

function optional(validate: Validator): Validator {
  return (value, path) => { if (value !== null) validate(value, path); };
}

function text(value: unknown, path: string): void {
  if (typeof value !== "string" || value.trim().length === 0) throw new TypeError(`${path} 必须是非空字符串`);
}

function choice(choices: readonly string[]): Validator {
  return (value, path) => {
    if (typeof value !== "string" || !choices.includes(value)) throw new TypeError(`${path} 变体无效`);
  };
}

function tagged(variants: Shape): Validator {
  return (value, path) => {
    const fields = record(value, path);
    const keys = Object.keys(fields);
    const tag = keys[0];
    if (keys.length !== 1 || tag === undefined || !Object.hasOwn(variants, tag)) throw new TypeError(`${path} 字段不符合公共 DTO 契约`);
    variants[tag]!(fields[tag], `${path}.${tag}`);
  };
}

function decimalId(value: unknown, path: string): void {
  if (typeof value !== "string" || !/^(0|[1-9]\d*)$/.test(value)) throw new TypeError(`${path} 必须是无损非负十进制字符串`);
}

function amount(value: unknown, path: string): void {
  if (typeof value !== "string" || !/^-?(0|[1-9]\d*)\.\d{2}$/.test(value) || value === "-0.00") throw new TypeError(`${path} 必须是精确十进制字符串`);
}

function date(value: unknown, path: string): void {
  if (typeof value !== "string" || !/^\d{4}-\d{2}-\d{2}$/.test(value)) throw new TypeError(`${path} 日期不符合公共 DTO 契约`);
  const parsed = new Date(`${value}T00:00:00Z`);
  if (!Number.isFinite(parsed.getTime()) || parsed.toISOString().slice(0, 10) !== value) throw new TypeError(`${path} 日期不符合公共 DTO 契约`);
}

function secondOfDay(value: unknown, path: string): void {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0 || value > 86_400) throw new TypeError(`${path} 必须是有效日内秒数`);
}

function amounts(keys: readonly string[]): Shape {
  return Object.fromEntries(keys.map((key) => [key, amount]));
}

function comparative(available: Validator): Validator {
  return tagged({ Available: available, Unavailable: object({ reason: choice(["NoPriorYearHistory"]) }) });
}

const line = object({ subject: text, amount });
const lines = array(line);
const incomeColumns = object({
  operating: lines, investing: lines, financing: lines, discontinued: lines,
  ...amounts(["operating_subtotal", "investing_subtotal", "financing_subtotal", "discontinued_subtotal", "income_tax", "net_income"]),
});
const note = object({
  code: text, name: text, target: tagged({ BalanceSheet: text, Income: text }),
  ...amounts(["opening", "movement", "ytd_movement", "closing"]),
});
const financials = object({
  scope: tagged({ Standalone: object({ entity_id: text }), Consolidated: object({ root_entity_id: text }) }),
  window_start: date, window_end: date,
  version_supersedes: optional(decimalId),
  version_kind: (value, path) => { if (value !== "Original") tagged({ Correction: object({ reason: text }) })(value, path); },
  balance_sheet: object({
    asset_lines: lines, liability_lines: lines, equity_lines: lines,
    ...amounts(["total_assets", "total_liabilities", "total_equity", "equity_to_parent", "liabilities_and_equity", "closing_cash"]),
    prior_year_end: comparative(object({
      lines, ...amounts(["total_assets", "total_liabilities", "total_equity", "equity_to_parent", "liabilities_and_equity"]),
    })),
  }),
  income: object({ quarter: incomeColumns, cumulative: incomeColumns, prior_year: comparative(incomeColumns), minority_net_income: optional(amount), net_income_to_parent: optional(amount) }),
  cash_flow: object({ ...amounts(["operating", "investing", "financing", "net_change", "opening_cash", "closing_cash"]), indirect: lines }),
  equity: object({
    ...amounts(["opening_parent", "net_income", "other_comprehensive", "capital_contributions", "distributions", "closing_parent"]),
    opening_minority: optional(amount), minority_net_income: optional(amount), closing_minority: optional(amount),
  }),
  notes: object({ items: array(note), consolidation_split_items: array(note) }),
});
const report = object({
  source: choice(["SimpleGenerated", "SimulationAccounting"]),
  id: decimalId, company_id: text, period: date, kind: choice(["Monthly", "Quarter", "HalfYear", "Annual"]),
  version_sequence: decimalId, supersedes: optional(decimalId), approved_date: date, approved_second_of_day: secondOfDay,
  published_date: date, published_second_of_day: secondOfDay,
  accounting: object({
    ...amounts(["total_assets", "total_liabilities", "total_equity", "closing_cash", "quarter_net_income", "net_income", "income_tax", "operating_cash_flow", "investing_cash_flow", "financing_cash_flow", "net_cash_change"]),
    prior_year_net_income: comparative(object({ amount })),
  }),
  financials,
});

function validateConsistency(value: PublicReportSummary): void {
  const details = value.financials;
  const balance = details.balance_sheet;
  const income = details.income;
  const cash = details.cash_flow;
  const summaryValues = {
    total_assets: balance.total_assets, total_liabilities: balance.total_liabilities, total_equity: balance.total_equity,
    closing_cash: balance.closing_cash, quarter_net_income: income.quarter.net_income, net_income: income.cumulative.net_income,
    income_tax: income.cumulative.income_tax, operating_cash_flow: cash.operating, investing_cash_flow: cash.investing,
    financing_cash_flow: cash.financing, net_cash_change: cash.net_change,
  };
  for (const key of Object.keys(summaryValues) as (keyof typeof summaryValues)[]) {
    if (value.accounting[key] !== summaryValues[key]) throw new TypeError(`WASM 公开报告.accounting.${key} 与已披露四表不一致`);
  }
  const prior = income.prior_year;
  const summaryPrior = value.accounting.prior_year_net_income;
  if ("Available" in prior ? !("Available" in summaryPrior) || prior.Available.net_income !== summaryPrior.Available.amount : !("Unavailable" in summaryPrior) || prior.Unavailable.reason !== summaryPrior.Unavailable.reason) {
    throw new TypeError("WASM 公开报告.accounting.prior_year_net_income 与已披露比较期不一致");
  }
  const approval = `${value.approved_date} ${value.approved_second_of_day.toString().padStart(5, "0")}`;
  const publication = `${value.published_date} ${value.published_second_of_day.toString().padStart(5, "0")}`;
  if (details.window_start > details.window_end || details.window_end !== value.period || details.window_end >= value.approved_date || approval > publication) {
    throw new TypeError("WASM 公开报告.financials.window 报告期间不一致或批准/发布时序非法");
  }
  if ((details.version_kind === "Original") !== (value.supersedes === null)
    || (details.version_kind === "Original") !== (details.version_supersedes === null)) throw new TypeError("WASM 公开报告.financials.version_kind 更正关系不一致");
  const minority = [income.minority_net_income, income.net_income_to_parent, details.equity.opening_minority, details.equity.minority_net_income, details.equity.closing_minority];
  const consolidated = "Consolidated" in details.scope;
  if (minority.some((amount) => consolidated ? amount === null : amount !== null)
    || (!consolidated && details.notes.consolidation_split_items.length !== 0)) {
    throw new TypeError("WASM 公开报告.financials.scope 与少数股东/合并拆分列不一致");
  }
}

export function parsePublicReport(value: unknown): PublicReportSummary {
  report(value, "WASM 公开报告");
  const parsed = value as PublicReportSummary;
  validateConsistency(parsed);
  return parsed;
}

export function parsePublicReportPage(value: unknown): PublicReportPage {
  object({ reports: array(report), next_cursor: optional(decimalId) })(value, "WASM 公开报告页");
  const parsed = value as PublicReportPage;
  parsed.reports.forEach(validateConsistency);
  return parsed;
}

export function parsePublicReportAvailability(value: unknown, query?: PublicReportAvailabilityQuery): PublicReportAvailability {
  const fields = record(value, "公开报告可用性");
  if (fields.status === "Available") object({ status: choice(["Available"]), report })(fields, "公开报告可用性");
  else if (fields.status === "Unavailable") object({
    status: choice(["Unavailable"]),
    reason: choice(["BeforeOpening", "NotYetSettled", "PeriodNotRepresented", "NotYetPublished", "NotScheduled", "ScopeNotRepresented"]),
  })(fields, "公开报告可用性");
  else throw new TypeError("公开报告可用性.status 变体无效");
  const parsed = value as PublicReportAvailability;
  if (parsed.status === "Available") {
    validateConsistency(parsed.report);
    if (query !== undefined) {
      const reportValue = parsed.report;
      const reportScope = reportValue.financials.scope;
      const scopeMatches = "Standalone" in query.scope
        ? "Standalone" in reportScope && query.scope.Standalone.entity_id === reportScope.Standalone.entity_id
        : "Consolidated" in reportScope && query.scope.Consolidated.root_entity_id === reportScope.Consolidated.root_entity_id;
      if (reportValue.company_id !== query.company_id || reportValue.period !== query.period_end || reportValue.kind !== query.kind || !scopeMatches) {
        throw new TypeError("公开报告可用性报告与请求的 company、期间、类型或 scope 不一致");
      }
    }
  }
  return parsed;
}

export function normalizePublicReportAvailabilityQuery(value: unknown): PublicReportAvailabilityQuery {
  const query = record(value, "公开报告可用性请求");
  object({ company_id: text, period_end: date, kind: choice(["Monthly", "Quarter", "HalfYear", "Annual"]), scope: tagged({
    Standalone: object({ entity_id: text }),
    Consolidated: object({ root_entity_id: text }),
  }) })(query, "公开报告可用性请求");
  const periodEnd = query.period_end as string;
  if (new Date(Date.UTC(Number(periodEnd.slice(0, 4)), Number(periodEnd.slice(5, 7)), 0)).toISOString().slice(0, 10) !== periodEnd) {
    throw new TypeError("公开报告可用性请求.period_end 必须是 ISO 自然月末");
  }
  return query as unknown as PublicReportAvailabilityQuery;
}
