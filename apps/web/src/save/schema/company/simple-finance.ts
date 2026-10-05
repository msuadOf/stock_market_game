import { array, exact, oneOf, record, string, SaveSchemaError } from "../primitives.ts";
import { simpleCivilDate as civilDate } from "./simple-values.ts";
import { parseBooks } from "./accounting/books.ts";
import { parseClosingRegistry } from "./closing.ts";
import { period, u64 } from "./value.ts";
import { parseSimpleFinanceConfig } from "./simple-finance-config.ts";
import { parseIncomeTaxPosition } from "./books/income-tax.ts";
import { validateSimpleChart } from "./simple-chart.ts";

export function parseSimpleFinanceState(value: unknown, path = "Simple 汇总财务状态") {
  const parsed = record(value, path);
  exact(parsed, ["company", "kind", "config", "books", "closing", "opening_date", "as_of", "last_month", "next_event_id", "income_tax_position", "recognized_periods"], path);
  const kind = oneOf(parsed.kind, `${path}.kind`, ["Industrial", "Bank", "Insurance", "RealEstate"] as const);
  const company = string(parsed.company, `${path}.company`);
  if (company.trim().length === 0) throw new SaveSchemaError(`${path}.company`, "不能为空");
  const asOf = civilDate(parsed.as_of, `${path}.as_of`);
  const openingDate = civilDate(parsed.opening_date, `${path}.opening_date`);
  if (openingDate > asOf) throw new SaveSchemaError(`${path}.opening_date`, "开账日期不得晚于当前财务日期");
  const lastMonth = period(parsed.last_month, `${path}.last_month`);
  if (asOf.slice(0, 7) !== lastMonth) throw new SaveSchemaError(`${path}.last_month`, "核算月份必须与财务日期一致");
  const nextId = u64(parsed.next_event_id, `${path}.next_event_id`);
  if (!/^[1-9]\d*$/.test(nextId)) throw new SaveSchemaError(`${path}.next_event_id`, "必须是规范正 u64 字符串");
  const config = parseSimpleFinanceConfig(parsed.config, `${path}.config`);
  const books = parseBooks(parsed.books, `${path}.books`);
  validateSimpleChart(books.chart, kind, `${path}.books.chart`);
  for (const [index, line] of config.opening_lines.entries()) {
    const definition = books.chart.accounts[line.account];
    if (definition === undefined || definition.element === "Revenue" || definition.element === "Expense") throw new SaveSchemaError(`${path}.config.opening_lines[${index}].account`, "期初科目必须属于当前类别科目表，不能直接填入收入费用");
  }
  const incomeTaxPosition = parseIncomeTaxPosition(parsed.income_tax_position, `${path}.income_tax_position`, config.tax_policy.income_tax, books);
  const currentYear = Number(asOf.slice(0, 4));
  if (incomeTaxPosition.loss_pool.some(loss => loss.origin_year > currentYear) || Object.keys(incomeTaxPosition.assessments).some(year => Number(year) > currentYear)) throw new SaveSchemaError(`${path}.income_tax_position.loss_pool`, "税务事实不能属于未来年度");
  let through = openingDate;
  const recognizedPeriods = array(parsed.recognized_periods, `${path}.recognized_periods`).map((value, index) => {
    const periodPath = `${path}.recognized_periods[${index}]`;
    const span = array(value, periodPath);
    if (span.length !== 2) throw new SaveSchemaError(periodPath, "必须为日期二元组");
    const start = civilDate(span[0], `${periodPath}[0]`), end = civilDate(span[1], `${periodPath}[1]`);
    const next = new Date(`${through}T00:00:00Z`); next.setUTCDate(next.getUTCDate() + 1);
    const lastDay = new Date(Date.UTC(Number(end.slice(0, 4)), Number(end.slice(5, 7)), 0)).toISOString().slice(0, 10);
    if (start !== next.toISOString().slice(0, 10) || !start.endsWith("-01") || end !== lastDay || start > end || start.slice(0, 4) !== end.slice(0, 4)) throw new SaveSchemaError(periodPath, "已确认账面期间必须连续、覆盖完整自然月份且不跨核算年度");
    through = end;
    return [start, end] as const;
  });
  if (through !== asOf) throw new SaveSchemaError(`${path}.recognized_periods`, "已确认期间必须覆盖开账后至当前财务日期");
  return { company, kind, config, books, closing: parseClosingRegistry(parsed.closing, `${path}.closing`), opening_date: openingDate, as_of: asOf, last_month: lastMonth, next_event_id: nextId, income_tax_position: incomeTaxPosition, recognized_periods: recognizedPeriods };
}
