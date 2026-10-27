// 输入结构校验不代表更正获批、凭证平衡或报告已经发布。
const BUSINESS_KINDS = ["SimpleRevenueAccrual", "SimpleFixedExpenseAccrual", "SimpleVariableExpenseAccrual", "CompanyDividendDeclaration", "CompanyDividendPayment", "CompanyStockDistributionCredit", "CompanyRightsOfferingSettlement", "CompanyRepurchaseCancellation", "CompanyShareReDenomination", "OpeningBalance", "LoanDisbursement", "LoanRepayment", "CashRevenue", "CreditSale", "ReceivableCollection", "CashExpense", "InterestAccrual", "InterestPayment", "TaxAccrual", "TaxPayment", "Depreciation", "CustomerDeposit", "CustomerWithdrawal", "LoanIssued", "LoanPrincipalCollected", "LoanInterestAccrued", "LoanInterestCollected", "DepositInterestAccrued", "DepositInterestPaid", "FeeAndCommissionEarned", "CreditImpairment", "LoanWriteOff", "WriteOffRecovery", "LandAcquisition", "DevelopmentCostIncurred", "PresaleCollection", "RealEstateDelivery", "FinalPaymentCollected", "BorrowingCostCapitalized", "DevelopmentImpairment", "InsurancePremiumAccrued", "InsurancePremiumCollected", "InsuranceServiceRevenue", "InsuranceFinance", "InsuranceLossComponent", "InsuranceClaimIncurred", "InsuranceClaimPaid"] as const;
type BusinessKind = typeof BUSINESS_KINDS[number];
type CashFlowClass = "Operating" | "Investing" | "Financing" | "NonCash";
type PostingSide = "Debit" | "Credit";
export interface CorrectionJournalLine { account: string; side: PostingSide; amount: string }
export interface CorrectionJournalEntry { source: number; date: string; kind: BusinessKind; cash_flow: CashFlowClass; lines: CorrectionJournalLine[] }
export interface CompanyReportCorrectionInput { operation_id: string; company: number; supersedes: number; entries: CorrectionJournalEntry[]; reason: string }
export interface CompletedReportCorrectionInput { request: CompanyReportCorrectionInput; publications: number[]; committed_at: { date: string; second_of_day: number } }
export interface ReportCorrectionStatusInput { pending: CompanyReportCorrectionInput[]; completed: Record<string, CompletedReportCorrectionInput> }

function fail(path: string, message: string): never { throw new Error(`${path}: ${message}`); }
function record(value: unknown, path: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) fail(path, "必须是字段对象");
  return value as Record<string, unknown>;
}
function exact(value: unknown, keys: readonly string[], path: string): Record<string, unknown> {
  const result = record(value, path);
  for (const key of Object.keys(result)) if (!keys.includes(key)) fail(`${path}.${key}`, "未知字段");
  for (const key of keys) if (!Object.hasOwn(result, key)) fail(`${path}.${key}`, "缺少必填字段");
  return result;
}
function text(value: unknown, path: string): string {
  if (typeof value !== "string") fail(path, "必须是字符串");
  for (let index = 0; index < value.length; index++) {
    const unit = value.charCodeAt(index);
    if (unit >= 0xd800 && unit <= 0xdbff) {
      const next = value.charCodeAt(++index);
      if (!(next >= 0xdc00 && next <= 0xdfff)) fail(path, "包含孤立 Unicode surrogate");
    } else if (unit >= 0xdc00 && unit <= 0xdfff) fail(path, "包含孤立 Unicode surrogate");
  }
  return value;
}
function unsigned(value: unknown, max: number, path: string): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || Object.is(value, -0) || value < 0 || value > max) fail(path, `必须是 0..${max} 范围内可无损表示的整数`);
  return value;
}
function u32(value: unknown, path: string): number { return unsigned(value, 4294967295, path); }
function list<T>(value: unknown, path: string, parse: (item: unknown, itemPath: string) => T): T[] {
  if (!Array.isArray(value)) fail(path, "必须是数组");
  return value.map((item, index) => parse(item, `${path}[${index}]`));
}
function choice<T extends string>(value: unknown, options: readonly T[], path: string): T {
  const result = text(value, path);
  if (!options.includes(result as T)) fail(path, "未知枚举值");
  return result as T;
}
function civilDate(value: unknown, path: string): string {
  const result = text(value, path);
  if (!/^\d{4}-\d{2}-\d{2}$/.test(result)) fail(path, "必须是 YYYY-MM-DD 日期");
  const year = Number(result.slice(0, 4)), month = Number(result.slice(5, 7)), day = Number(result.slice(8));
  const leap = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0);
  const days = [31, leap ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
  const limit = days[month - 1];
  if (year < 1900 || year > 2199 || limit === undefined || day < 1 || day > limit) fail(path, "日期超出真实公历窗口或无效");
  return result;
}
function accountingAmount(value: unknown, path: string): string {
  const result = text(value, path);
  // Rust str::trim 使用 Unicode White_Space，包含 U+0085，不包含 U+FEFF。
  const trimmed = result.replace(/^\p{White_Space}+|\p{White_Space}+$/gu, "");
  const match = /^[+-]?([0-9]+)(?:\.([0-9]{1,2}))?$/.exec(trimmed);
  if (match === null || match[1] === undefined) fail(path, "必须是 AccountingAmount 元字符串");
  const fraction = match[2] === undefined ? "00" : match[2].padEnd(2, "0");
  const magnitude = BigInt(match[1]) * 100n + BigInt(fraction);
  // 实际 Rust codec 先构造绝对值，故负 i128::MIN 也不能解码。
  if (magnitude > 170141183460469231731687303715884105727n) fail(path, "金额超出实际 i128 codec 范围");
  return result;
}
function journalLine(value: unknown, path: string): CorrectionJournalLine {
  const item = exact(value, ["account", "side", "amount"], path);
  return { account: text(item.account, `${path}.account`), side: choice(item.side, ["Debit", "Credit"], `${path}.side`), amount: accountingAmount(item.amount, `${path}.amount`) };
}
function journalEntry(value: unknown, path: string): CorrectionJournalEntry {
  const item = exact(value, ["source", "date", "kind", "cash_flow", "lines"], path);
  return { source: unsigned(item.source, Number.MAX_SAFE_INTEGER, `${path}.source`), date: civilDate(item.date, `${path}.date`), kind: choice(item.kind, BUSINESS_KINDS, `${path}.kind`), cash_flow: choice(item.cash_flow, ["Operating", "Investing", "Financing", "NonCash"], `${path}.cash_flow`), lines: list(item.lines, `${path}.lines`, journalLine) };
}
function correction(value: unknown, path: string): CompanyReportCorrectionInput {
  const item = exact(value, ["operation_id", "company", "supersedes", "entries", "reason"], path);
  return { operation_id: text(item.operation_id, `${path}.operation_id`), company: u32(item.company, `${path}.company`), supersedes: u32(item.supersedes, `${path}.supersedes`), entries: list(item.entries, `${path}.entries`, journalEntry), reason: text(item.reason, `${path}.reason`) };
}
function completed(value: unknown, path: string): CompletedReportCorrectionInput {
  const item = exact(value, ["request", "publications", "committed_at"], path);
  const instant = exact(item.committed_at, ["date", "second_of_day"], `${path}.committed_at`);
  return { request: correction(item.request, `${path}.request`), publications: list(item.publications, `${path}.publications`, u32), committed_at: { date: civilDate(instant.date, `${path}.committed_at.date`), second_of_day: unsigned(instant.second_of_day, 86399, `${path}.committed_at.second_of_day`) } };
}
export function parseCompanyReportCorrection(value: unknown): CompanyReportCorrectionInput { return correction(value, "Correction"); }
export function parseReportCorrectionStatus(value: unknown): ReportCorrectionStatusInput {
  const item = exact(value, ["pending", "completed"], "CorrectionStatus");
  const table = record(item.completed, "CorrectionStatus.completed");
  const result: Record<string, CompletedReportCorrectionInput> = Object.create(null);
  for (const key of Object.keys(table)) { text(key, "CorrectionStatus.completed.key"); result[key] = completed(table[key], `CorrectionStatus.completed[${JSON.stringify(key)}]`); }
  return { pending: list(item.pending, "CorrectionStatus.pending", correction), completed: result };
}

// 直接解析 raw JSON，避免 JSON.parse 先覆盖重复 key 或舍入数字；字符串 codec 仍由 JSON.parse 校验。
function rawJson(raw: string): unknown {
  if (typeof raw !== "string") fail("CorrectionJSON", "必须是原始 JSON 字符串");
  let cursor = 0;
  const skip = () => { while (cursor < raw.length && /[ \t\r\n]/.test(raw[cursor])) cursor++; };
  const string = (path: string): string => {
    if (raw[cursor] !== '"') fail(path, "必须是 JSON 字符串");
    const start = cursor++;
    while (cursor < raw.length) {
      const char = raw[cursor++];
      if (char === "\\") { cursor++; continue; }
      if (char === '"') {
        let result: unknown;
        try { result = JSON.parse(raw.slice(start, cursor)); }
        catch (error) { throw new Error(`${path}: 非法 JSON 字符串`, { cause: error }); }
        return text(result, path);
      }
    }
    return fail(path, "JSON 字符串未闭合");
  };
  const value = (path: string): unknown => {
    skip(); const char = raw[cursor];
    if (char === '"') return string(path);
    if (char === "{") {
      cursor++; skip(); const result: Record<string, unknown> = Object.create(null);
      if (raw[cursor] === "}") { cursor++; return result; }
      for (;;) {
        skip(); const key = string(`${path}.key`);
        if (Object.hasOwn(result, key)) fail(`${path}.${key}`, "JSON 包含重复字段");
        skip(); if (raw[cursor++] !== ":") fail(path, "JSON 对象字段缺少冒号");
        result[key] = value(`${path}.${key}`); skip();
        if (raw[cursor] === "}") { cursor++; return result; }
        if (raw[cursor++] !== ",") fail(path, "JSON 对象缺少逗号或结束符");
      }
    }
    if (char === "[") {
      cursor++; skip(); const result: unknown[] = [];
      if (raw[cursor] === "]") { cursor++; return result; }
      for (;;) {
        result.push(value(`${path}[${result.length}]`)); skip();
        if (raw[cursor] === "]") { cursor++; return result; }
        if (raw[cursor++] !== ",") fail(path, "JSON 数组缺少逗号或结束符");
      }
    }
    for (const [token, decoded] of [["null", null], ["true", true], ["false", false]] as const) {
      if (raw.startsWith(token, cursor)) { cursor += token.length; return decoded; }
    }
    const start = cursor;
    while (cursor < raw.length && !/[ \t\r\n,\]}]/.test(raw[cursor])) cursor++;
    const token = raw.slice(start, cursor);
    if (!/^(?:0|[1-9][0-9]*)$/.test(token)) fail(path, "数值 token 必须是规范 unsigned 整数，不接受负零、小数或指数");
    return unsigned(Number(token), Number.MAX_SAFE_INTEGER, path);
  };
  const result = value("CorrectionJSON"); skip();
  if (cursor !== raw.length) fail("CorrectionJSON", "JSON 根值后存在额外内容");
  return result;
}
export function parseCompanyReportCorrectionJson(raw: string): CompanyReportCorrectionInput { return parseCompanyReportCorrection(rawJson(raw)); }
export function parseReportCorrectionStatusJson(raw: string): ReportCorrectionStatusInput { return parseReportCorrectionStatus(rawJson(raw)); }
