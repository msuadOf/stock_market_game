import type { SimpleFinanceConfig } from "../../../types/generated/SimpleFinanceConfig";
import { array, exact, integer, record, SaveSchemaError } from "../primitives.ts";
import { parseJournalLine } from "./accounting/journal.ts";
import { parseTaxPolicy } from "./books/industrial.ts";
import { simpleAmount } from "./simple-values.ts";

export function parseSimpleFinanceConfig(value: unknown, path = "Simple 汇总财务配置"): SimpleFinanceConfig {
  const parsed = record(value, path);
  exact(parsed, ["opening_lines", "tax_policy", "summary_rule", "book_display"], path);
  if (parsed.summary_rule !== "ReceivableRevenuePayableExpenses") throw new SaveSchemaError(`${path}.summary_rule`, "汇总规则不支持");
  // 账面展示参数（2026-10-08 用户决策）：严格必填，旧档缺字段显式拒绝；
  // 投资额比例必须为 0..10000 bp。
  const bookDisplay = record(parsed.book_display, `${path}.book_display`);
  exact(bookDisplay, ["investment_of_revenue_bp"], `${path}.book_display`);
  const ratioBp = integer(bookDisplay.investment_of_revenue_bp, `${path}.book_display.investment_of_revenue_bp`, 0);
  if (ratioBp > 10_000) throw new SaveSchemaError(`${path}.book_display.investment_of_revenue_bp`, "投资额展示比例必须为 0..10000 bp");
  const policy = parseTaxPolicy(parsed.tax_policy, `${path}.tax_policy`);
  if (policy.version < 1 || policy.version > 4294967295 || policy.income_tax.loss_carryforward_years < 1 || policy.income_tax.loss_carryforward_years > 65535 || [policy.vat.output_rate_bp, policy.vat.input_rate_bp, policy.vat.deductible_share_bp, policy.income_tax.rate_bp].some(rate => rate < 0 || rate > 10000)) throw new SaveSchemaError(`${path}.tax_policy`, "税务参数超出共同政策允许范围");
  let debits = 0n;
  let credits = 0n;
  const lines = array(parsed.opening_lines, `${path}.opening_lines`).map((entry, index) => {
    const linePath = `${path}.opening_lines[${index}]`;
    const line = parseJournalLine(entry, linePath);
    const amount = simpleAmount(line.amount, `${linePath}.amount`, true);
    if (line.account.trim().length === 0 || amount === "0.00") throw new SaveSchemaError(linePath, "期初科目不能为空且金额必须为正");
    if (line.side === "Debit") debits += BigInt(amount.replace(".", "")); else credits += BigInt(amount.replace(".", ""));
    return { ...line, amount };
  });
  if (debits === 0n || debits !== credits || debits > (1n << 127n) - 1n) throw new SaveSchemaError(`${path}.opening_lines`, "期初凭证必须非空、借贷平衡且不溢出");
  return { opening_lines: lines, tax_policy: policy, summary_rule: "ReceivableRevenuePayableExpenses", book_display: { investment_of_revenue_bp: ratioBp } };
}
