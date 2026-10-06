import { array, exact, integer, map, oneOf, record, string, SaveSchemaError } from "../primitives.ts";
import { simpleAmount, simpleCivilDate as civilDate } from "./simple-values.ts";
import { parseBooks } from "./accounting/books.ts";
import { parseClosingRegistry } from "./closing.ts";
import { period, u64 } from "./value.ts";
import { parseSimpleFinanceConfig } from "./simple-finance-config.ts";
import { parseIncomeTaxPosition } from "./books/income-tax.ts";
import { validateSimpleChart } from "./simple-chart.ts";
import { accountingMinorUnits } from "./accounting/amount.ts";

type DividendPayment = { readonly source: number; readonly paid_on: string; readonly amount: string };
type DividendDeclaration = { readonly plan_id: string; readonly approved_on: string; readonly total_gross: string; readonly registered_capital: string };
type DividendPlan = { readonly declaration: DividendDeclaration; readonly declaration_source: number; readonly reserve: string; readonly reserve_basis_year: number | null; readonly payments: Readonly<Record<string, DividendPayment>> };
type DividendPosting = { readonly account: string; readonly side: "Debit" | "Credit"; readonly amount: string };
type DividendLegalFacts = { readonly registered_capital: string; readonly source_evidence: string };
type StockDistributionKind = "BonusShares" | "CapitalReserveConversion";
type StockDistributionFact = { readonly event_id: string; readonly approval_reference: string; readonly kind: StockDistributionKind; readonly approved_on: string; readonly new_shares: string; readonly par_value_per_share: string; readonly capital_increase: string; readonly registered_capital_at_approval: string; readonly credited_on: string | null };

function parseStockDistributionFact(value: unknown, path: string): StockDistributionFact {
  const parsed = record(value, path)
  exact(parsed, ["event_id", "approval_reference", "kind", "approved_on", "new_shares", "par_value_per_share", "capital_increase", "registered_capital_at_approval", "credited_on"], path)
  const event_id = string(parsed.event_id, `${path}.event_id`)
  const approval_reference = string(parsed.approval_reference, `${path}.approval_reference`)
  const approved_on = civilDate(parsed.approved_on, `${path}.approved_on`)
  const credited_on = parsed.credited_on === null ? null : civilDate(parsed.credited_on, `${path}.credited_on`)
  if (!event_id.trim() || !approval_reference.trim()) throw new SaveSchemaError(path, "送转事件身份与批准引用不能为空")
  const kind = oneOf(parsed.kind, `${path}.kind`, ["BonusShares", "CapitalReserveConversion"] as const)
  const new_shares = string(parsed.new_shares, `${path}.new_shares`)
  if (!/^[1-9]\d*$/.test(new_shares)) throw new SaveSchemaError(`${path}.new_shares`, "送转新增股数必须为正 u64 十进制字符串")
  const par = string(parsed.par_value_per_share, `${path}.par_value_per_share`)
  if (!/^[1-9]\d*$/.test(par)) throw new SaveSchemaError(`${path}.par_value_per_share`, "每股面值必须为正整数分")
  const capital_increase = simpleAmount(parsed.capital_increase, `${path}.capital_increase`, true)
  const registered_capital_at_approval = simpleAmount(parsed.registered_capital_at_approval, `${path}.registered_capital_at_approval`, true)
  // 股本增加金额 = 每股面值（分）× 新增股数；两侧都以分核对（`Money` wire 为分、
  // `AccountingAmount` 元字符串换算为分），不再额外乘 100。
  if (accountingMinorUnits(capital_increase) !== BigInt(par) * BigInt(new_shares)) throw new SaveSchemaError(path, "送转股本增加金额必须等于每股面值乘以新增股数")
  if (credited_on !== null && credited_on < approved_on) throw new SaveSchemaError(`${path}.credited_on`, "送转入账日期不得早于批准日期")
  return { event_id, approval_reference, kind, approved_on, new_shares, par_value_per_share: par, capital_increase, registered_capital_at_approval, credited_on }
}
const I128_MAX = (1n << 127n) - 1n;
const I128_MIN = -(1n << 127n);

function checkedLedgerAdd(left: bigint, right: bigint, path: string): bigint {
  const result = left + right;
  if (result < I128_MIN || result > I128_MAX) throw new SaveSchemaError(path, "Journal 账簿重放超出 AccountingAmount 范围");
  return result;
}

function checkedLedgerSubtract(left: bigint, right: bigint, path: string): bigint {
  const result = left - right;
  if (result < I128_MIN || result > I128_MAX) throw new SaveSchemaError(path, "Journal 账簿重放超出 AccountingAmount 范围");
  return result;
}

function stringKey(key: string, path: string): void {
  if (key.trim().length === 0) throw new SaveSchemaError(path, "映射键不能为空");
}

function parseDividendDeclaration(value: unknown, path: string): DividendDeclaration {
  const parsed = record(value, path);
  exact(parsed, ["plan_id", "approved_on", "total_gross", "registered_capital"], path);
  const planId = string(parsed.plan_id, `${path}.plan_id`);
  const approvedOn = civilDate(parsed.approved_on, `${path}.approved_on`);
  const gross = simpleAmount(parsed.total_gross, `${path}.total_gross`, true);
  const capital = simpleAmount(parsed.registered_capital, `${path}.registered_capital`, true);
  if (planId.trim().length === 0 || accountingMinorUnits(gross) <= 0n || accountingMinorUnits(capital) <= 0n) throw new SaveSchemaError(path, "分红方案标识、总额及注册资本必须有效且为正");
  return { plan_id: planId, approved_on: approvedOn, total_gross: gross, registered_capital: capital };
}

function parseDividendLegalFacts(value: unknown, path: string): DividendLegalFacts | null {
  if (value === null) return null;
  const parsed = record(value, path);
  exact(parsed, ["registered_capital", "source_evidence"], path);
  const registeredCapital = simpleAmount(parsed.registered_capital, `${path}.registered_capital`, true);
  const sourceEvidence = string(parsed.source_evidence, `${path}.source_evidence`);
  if (accountingMinorUnits(registeredCapital) <= 0n) throw new SaveSchemaError(`${path}.registered_capital`, "注册资本必须为正");
  if (sourceEvidence.trim().length === 0) throw new SaveSchemaError(`${path}.source_evidence`, "必须明确记录注册资本事实来源");
  return { registered_capital: registeredCapital, source_evidence: sourceEvidence };
}

function parseDividendPayment(value: unknown, path: string): DividendPayment {
  const parsed = record(value, path);
  exact(parsed, ["source", "paid_on", "amount"], path);
  const source = integer(parsed.source, `${path}.source`, 0);
  const paidOn = civilDate(parsed.paid_on, `${path}.paid_on`);
  const paidAmount = simpleAmount(parsed.amount, `${path}.amount`, true);
  if (accountingMinorUnits(paidAmount) <= 0n) throw new SaveSchemaError(`${path}.amount`, "分红实付金额必须为正");
  return { source, paid_on: paidOn, amount: paidAmount };
}

function parseDividendPlan(value: unknown, path: string): DividendPlan {
  const parsed = record(value, path);
  exact(parsed, ["declaration", "declaration_source", "reserve", "reserve_basis_year", "payments"], path);
  const declaration = parseDividendDeclaration(parsed.declaration, `${path}.declaration`);
  const declarationSource = integer(parsed.declaration_source, `${path}.declaration_source`, 0);
  const reserve = simpleAmount(parsed.reserve, `${path}.reserve`, true);
  const reserveMinor = accountingMinorUnits(reserve);
  if (reserveMinor < 0n) throw new SaveSchemaError(`${path}.reserve`, "公积金提取额不得为负");
  const year = parsed.reserve_basis_year === null ? null : integer(parsed.reserve_basis_year, `${path}.reserve_basis_year`, 1900);
  if (year !== null && year > 2199) throw new SaveSchemaError(`${path}.reserve_basis_year`, "必须为有效年度或 null");
  if ((reserveMinor > 0n) !== (year !== null)) throw new SaveSchemaError(`${path}.reserve_basis_year`, "公积金提取额与依据年度必须同时存在或同时为空");
  const payments = map(parsed.payments, `${path}.payments`, stringKey, parseDividendPayment);
  let paid = 0n;
  for (const [paymentId, payment] of Object.entries(payments)) {
    if (paymentId.trim().length === 0 || payment.paid_on < declaration.approved_on) throw new SaveSchemaError(`${path}.payments.${paymentId}`, "付款批次标识不能为空且付款日期不得早于决议");
    paid += accountingMinorUnits(payment.amount);
  }
  if (paid > accountingMinorUnits(declaration.total_gross)) throw new SaveSchemaError(`${path}.payments`, "累计实际付款超过批准分红总额");
  return { declaration, declaration_source: declarationSource, reserve, reserve_basis_year: year, payments };
}

function validateDividendBooks(dividends: Readonly<Record<string, DividendPlan>>, books: ReturnType<typeof parseBooks>, path: string): void {
  const journalEntries = books.journal.batches.flat();
  const entries = new Map(journalEntries.map((entry) => [entry.source, entry]));
  if (entries.size !== journalEntries.length) throw new SaveSchemaError(`${path}.books.journal`, "Simple Journal 凭证来源重复");
  const referencedSources = new Set<number>();
  const verify = (source: number, kind: "CompanyDividendDeclaration" | "CompanyDividendPayment", date: string, lines: readonly DividendPosting[], sourcePath: string) => {
    if (referencedSources.has(source)) throw new SaveSchemaError(`${sourcePath}.source`, "多个分红事实不能共用凭证来源");
    referencedSources.add(source);
    const entry = entries.get(source);
    if (entry === undefined || entry.kind !== kind || entry.date !== date || entry.cash_flow !== "NonCash" || entry.lines.length !== lines.length || entry.lines.some((line, index) => line.account !== lines[index].account || line.side !== lines[index].side || accountingMinorUnits(line.amount) !== accountingMinorUnits(lines[index].amount))) {
      throw new SaveSchemaError(sourcePath, "分红状态与绑定的 Simple Journal 凭证事实不一致");
    }
  };
  for (const [planId, plan] of Object.entries(dividends)) {
    const { declaration, reserve } = plan;
    const reserveMinor = accountingMinorUnits(reserve);
    const grossMinor = accountingMinorUnits(declaration.total_gross);
    const retainedMinor = grossMinor + reserveMinor;
    if (retainedMinor > I128_MAX) throw new SaveSchemaError(`${path}.dividends.${planId}.reserve`, "分红及公积金合计超出 AccountingAmount 范围");
    const declarationLines: DividendPosting[] = [{ account: "4103", side: "Debit", amount: `${retainedMinor / 100n}.${(retainedMinor % 100n).toString().padStart(2, "0")}` }];
    if (reserveMinor > 0n) declarationLines.push({ account: "simple_statutory_reserve", side: "Credit", amount: reserve });
    declarationLines.push({ account: "simple_dividend_payable", side: "Credit", amount: declaration.total_gross });
    verify(plan.declaration_source, "CompanyDividendDeclaration", declaration.approved_on, declarationLines, `${path}.dividends.${planId}.declaration_source`);
    for (const [paymentId, payment] of Object.entries(plan.payments)) {
      verify(payment.source, "CompanyDividendPayment", payment.paid_on, [
        { account: "simple_dividend_payable", side: "Debit", amount: payment.amount },
        { account: "simple_dividend_settlement_asset", side: "Credit", amount: payment.amount },
      ], `${path}.dividends.${planId}.payments.${paymentId}`);
    }
  }
  for (const entry of books.journal.batches.flat()) {
    if ((entry.kind === "CompanyDividendDeclaration" || entry.kind === "CompanyDividendPayment") && !referencedSources.has(entry.source)) throw new SaveSchemaError(`${path}.books.journal`, "存在未绑定到分红方案或付款批次的分红凭证");
  }
}

function validateSimpleLedger(books: ReturnType<typeof parseBooks>, path: string): ReadonlyMap<string, bigint> {
  const balances = new Map<string, { debit: bigint; credit: bigint }>();
  for (const [batchIndex, batch] of books.journal.batches.entries()) {
    for (const [entryIndex, entry] of batch.entries()) {
      const entryPath = `${path}.books.journal.batches[${batchIndex}][${entryIndex}]`;
      if (entry.lines.length === 0) throw new SaveSchemaError(`${entryPath}.lines`, "Journal 凭证不得为空");
      let debits = 0n;
      let credits = 0n;
      for (const [lineIndex, line] of entry.lines.entries()) {
        const linePath = `${entryPath}.lines[${lineIndex}]`;
        const definition = books.chart.accounts[line.account];
        if (definition === undefined) throw new SaveSchemaError(`${linePath}.account`, "凭证科目不在当前 Simple 科目表中");
        const value = accountingMinorUnits(line.amount, `${linePath}.amount`);
        if (value <= 0n) throw new SaveSchemaError(`${linePath}.amount`, "Journal 凭证金额必须为正");
        if (line.side === "Debit") debits = checkedLedgerAdd(debits, value, entryPath);
        else credits = checkedLedgerAdd(credits, value, entryPath);
        const balance = balances.get(line.account) ?? { debit: 0n, credit: 0n };
        balances.set(line.account, line.side === "Debit"
          ? { ...balance, debit: checkedLedgerAdd(balance.debit, value, `${linePath}.account`) }
          : { ...balance, credit: checkedLedgerAdd(balance.credit, value, `${linePath}.account`) });
      }
      if (debits === 0n || credits === 0n || debits !== credits) throw new SaveSchemaError(entryPath, "Journal 凭证借贷不平衡或缺少借贷方向");
    }
    for (const [account, balance] of balances) {
      if (books.chart.accounts[account]?.is_cash && checkedLedgerSubtract(balance.debit, balance.credit, `${path}.books.journal.batches[${batchIndex}]`) < 0n) throw new SaveSchemaError(`${path}.books.journal.batches[${batchIndex}]`, `现金科目 ${account} 余额不得为负`);
    }
  }
  let debitBalance = 0n;
  let creditBalance = 0n;
  const netBalances = new Map<string, bigint>();
  for (const balance of balances.values()) {
    debitBalance = checkedLedgerAdd(debitBalance, balance.debit, `${path}.books.journal`);
    creditBalance = checkedLedgerAdd(creditBalance, balance.credit, `${path}.books.journal`);
  }
  if (debitBalance !== creditBalance) throw new SaveSchemaError(`${path}.books.journal`, "Journal 账簿试算不平衡");
  for (const [account, balance] of balances) netBalances.set(account, checkedLedgerSubtract(balance.debit, balance.credit, `${path}.books.journal.accounts.${account}`));
  return netBalances;
}

export function parseSimpleFinanceState(value: unknown, path = "Simple 汇总财务状态") {
  const parsed = record(value, path);
  exact(parsed, ["company", "kind", "config", "books", "closing", "opening_date", "as_of", "last_month", "next_event_id", "income_tax_position", "recognized_periods", "dividends", "stock_distributions", "legal_facts"], path);
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
  const dividends = map(parsed.dividends, `${path}.dividends`, stringKey, parseDividendPlan);
  const legalFacts = parseDividendLegalFacts(parsed.legal_facts, `${path}.legal_facts`);
  const registeredCapital = legalFacts?.registered_capital ?? null;
  const stock_distributions = map(parsed.stock_distributions, `${path}.stock_distributions`, stringKey, parseStockDistributionFact);
  // 送转入账按 面值×新增股数 演进注册资本法定事实；分红与送转声明冻结的是各自
  // 批准时点的注册资本。按「当前法定注册资本 − 批准日当天及之后才入账的送转股本
  // 增加」重构批准时点口径：批准发生在日内、送转入账发生在日终，批准日当天的
  // 入账也尚未反映在声明口径中。
  const registeredCapitalMinor = registeredCapital === null ? null : accountingMinorUnits(registeredCapital);
  const capitalAtApproval = (approvedOn: string): bigint | null => {
    if (registeredCapitalMinor === null) return null;
    let capital = registeredCapitalMinor;
    for (const fact of Object.values(stock_distributions)) {
      if (fact.credited_on !== null && fact.credited_on >= approvedOn) capital -= accountingMinorUnits(fact.capital_increase);
    }
    return capital;
  };
  let reserved = 0n;
  let unpaid = 0n;
  for (const [planId, plan] of Object.entries(dividends)) {
    if (planId.trim().length === 0 || plan.declaration.plan_id !== planId || accountingMinorUnits(plan.declaration.registered_capital) !== capitalAtApproval(plan.declaration.approved_on)) throw new SaveSchemaError(`${path}.dividends.${planId}.declaration`, "方案身份或注册资本与公司绑定事实不一致");
    if (plan.declaration.approved_on <= openingDate) throw new SaveSchemaError(`${path}.dividends.${planId}.declaration.approved_on`, "批准日期不得早于公司开账日");
    if (plan.reserve_basis_year !== null && (plan.reserve_basis_year < Number(openingDate.slice(0, 4)) || plan.reserve_basis_year > Number(asOf.slice(0, 4)))) throw new SaveSchemaError(`${path}.dividends.${planId}.reserve_basis_year`, "公积金依据年度不得早于开账年度或晚于存档年度");
    reserved += accountingMinorUnits(plan.reserve);
    unpaid += accountingMinorUnits(plan.declaration.total_gross) - Object.values(plan.payments).reduce((paidTotal, payment) => paidTotal + accountingMinorUnits(payment.amount), 0n);
    if (reserved > I128_MAX || unpaid > I128_MAX) throw new SaveSchemaError(`${path}.dividends`, "累计分红或公积金事实超出 AccountingAmount 范围");
  }
  const dividendCount = Object.keys(dividends).length;
  if (dividendCount > 0 && legalFacts === null) throw new SaveSchemaError(`${path}.legal_facts`, "存在分红方案时必须保存注册资本法定事实及来源证据");
  for (const [eventId, fact] of Object.entries(stock_distributions)) {
    if (eventId !== fact.event_id) throw new SaveSchemaError(`${path}.stock_distributions.${eventId}.event_id`, "送转事实键与事件身份不一致");
    if (fact.approved_on <= openingDate) throw new SaveSchemaError(`${path}.stock_distributions.${eventId}.approved_on`, "送转批准日期不得早于公司开账日");
    if (registeredCapital === null || accountingMinorUnits(fact.registered_capital_at_approval) !== capitalAtApproval(fact.approved_on)) throw new SaveSchemaError(`${path}.stock_distributions.${eventId}.registered_capital_at_approval`, "送转声明的注册资本与公司绑定法定事实不一致");
  }
  validateDividendBooks(dividends, books, path);
  const ledgerBalances = validateSimpleLedger(books, path);
  const latestJournalSource = books.journal.batches.flat().reduce((latest, entry) => Math.max(latest, entry.source), -1);
  if (latestJournalSource >= 0 && BigInt(nextId) <= BigInt(latestJournalSource)) throw new SaveSchemaError(`${path}.next_event_id`, "下一个财务事件来源不得重用已过账 Journal 来源");
  const payable = checkedLedgerSubtract(0n, ledgerBalances.get("simple_dividend_payable") ?? 0n, `${path}.dividends`);
  const reservedInBooks = checkedLedgerSubtract(0n, ledgerBalances.get("simple_statutory_reserve") ?? 0n, `${path}.dividends`);
  if (payable !== unpaid || reservedInBooks < reserved) throw new SaveSchemaError(`${path}.dividends`, "应付股利或法定公积金科目余额与分红事实不一致");
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
  return { company, kind, config, books, closing: parseClosingRegistry(parsed.closing, `${path}.closing`), opening_date: openingDate, as_of: asOf, last_month: lastMonth, next_event_id: nextId, income_tax_position: incomeTaxPosition, recognized_periods: recognizedPeriods, dividends, stock_distributions, legal_facts: legalFacts };
}
