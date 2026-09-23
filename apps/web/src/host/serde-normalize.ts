type WasmSaveMaps = {
  readonly snapshot: { readonly accounts: Record<string, unknown> };
  readonly npc_attention: Record<string, unknown>;
  readonly retail_experience: Record<string, unknown>;
  readonly parent_orders: Record<string, unknown>;
  readonly strategy_profiles: Record<string, unknown>;
  readonly information_states: Record<string, unknown>;
  readonly belief_books: Record<string, unknown>;
  readonly watchlists: Record<string, unknown>;
  readonly price_memories: Record<string, unknown>;
  readonly plans: { readonly plans: Record<string, unknown> };
};

type PublicReportPage = {
  readonly reports: PublicReportSummary[];
  readonly next_cursor: string | null;
};

type PublicReportSummary = {
  readonly id: string;
  readonly company_id: string;
  readonly period: string;
  readonly kind: "Monthly" | "Quarter" | "HalfYear" | "Annual";
  readonly version_sequence: string;
  readonly supersedes: string | null;
  readonly approved_date: string;
  readonly approved_second_of_day: number;
  readonly published_date: string;
  readonly published_second_of_day: number;
  readonly accounting: PublicReportAccountingSummary;
};

type PublicReportAccountingSummary = {
  readonly total_assets: string;
  readonly total_liabilities: string;
  readonly total_equity: string;
  readonly closing_cash: string;
  readonly quarter_net_income: string;
  readonly net_income: string;
  readonly income_tax: string;
  readonly operating_cash_flow: string;
  readonly investing_cash_flow: string;
  readonly financing_cash_flow: string;
  readonly net_cash_change: string;
  readonly prior_year_net_income: { readonly Available: { readonly amount: string } }
    | { readonly Unavailable: { readonly reason: "NoPriorYearHistory" } };
};

const PUBLIC_REPORT_KINDS = new Set(["Monthly", "Quarter", "HalfYear", "Annual"]);
const UNSIGNED_DECIMAL = /^(0|[1-9]\d*)$/;
const PUBLIC_ACCOUNTING_AMOUNT_KEYS = [
  "total_assets", "total_liabilities", "total_equity", "closing_cash", "quarter_net_income",
  "net_income", "income_tax", "operating_cash_flow", "investing_cash_flow", "financing_cash_flow",
  "net_cash_change",
] as const;

function record(value: unknown, path: string): Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new TypeError(`${path} 必须是对象`);
  }
  return value as Record<string, unknown>;
}

function exactKeys(value: Record<string, unknown>, keys: readonly string[], path: string): void {
  const actual = Object.keys(value);
  if (actual.length !== keys.length || actual.some((key) => !keys.includes(key))) {
    throw new TypeError(`${path} 字段不符合公共 DTO 契约`);
  }
}

function opaqueDecimalId(value: unknown, path: string): string {
  if (typeof value !== "string" || !UNSIGNED_DECIMAL.test(value)) {
    throw new TypeError(`${path} 必须是无损非负十进制字符串`);
  }
  return value;
}

function secondOfDay(value: unknown, path: string): number {
  if (!Number.isSafeInteger(value) || typeof value !== "number" || value < 0 || value > 86_400) {
    throw new TypeError(`${path} 必须是有效日内秒数`);
  }
  return value;
}

function accountingDecimal(value: unknown, path: string): string {
  if (typeof value !== "string" || !/^-?\d+\.\d{2}$/.test(value)) {
    throw new TypeError(`${path} 必须是精确十进制字符串`);
  }
  return value;
}

function normalizePublicReport(value: unknown): PublicReportSummary {
  const report = record(value, "WASM 公开报告");
  exactKeys(report, [
    "id", "company_id", "period", "kind", "version_sequence", "supersedes", "approved_date",
    "approved_second_of_day", "published_date", "published_second_of_day", "accounting",
  ], "WASM 公开报告");
  if (typeof report.company_id !== "string" || typeof report.period !== "string"
    || typeof report.approved_date !== "string"
    || typeof report.published_date !== "string" || (report.supersedes !== null && typeof report.supersedes !== "string")
    || typeof report.kind !== "string" || !PUBLIC_REPORT_KINDS.has(report.kind)) {
    throw new TypeError("WASM 公开报告字段类型不符合公共 DTO 契约");
  }
  opaqueDecimalId(report.id, "WASM 公开报告.id");
  opaqueDecimalId(report.version_sequence, "WASM 公开报告.version_sequence");
  if (report.supersedes !== null) opaqueDecimalId(report.supersedes, "WASM 公开报告.supersedes");
  if (!/^\d{4}-\d{2}-\d{2}$/.test(report.period) || !/^\d{4}-\d{2}-\d{2}$/.test(report.approved_date)
    || !/^\d{4}-\d{2}-\d{2}$/.test(report.published_date)) {
    throw new TypeError("WASM 公开报告日期不符合公共 DTO 契约");
  }
  secondOfDay(report.approved_second_of_day, "WASM 公开报告.approved_second_of_day");
  secondOfDay(report.published_second_of_day, "WASM 公开报告.published_second_of_day");
  const accounting = record(report.accounting, "WASM 公开报告.accounting");
  exactKeys(accounting, [...PUBLIC_ACCOUNTING_AMOUNT_KEYS, "prior_year_net_income"], "WASM 公开报告.accounting");
  for (const key of PUBLIC_ACCOUNTING_AMOUNT_KEYS) accountingDecimal(accounting[key], `WASM 公开报告.accounting.${key}`);
  const comparative = record(accounting.prior_year_net_income, "WASM 公开报告.accounting.prior_year_net_income");
  const available = comparative.Available;
  const unavailable = comparative.Unavailable;
  if (available !== undefined) {
    exactKeys(comparative, ["Available"], "WASM 公开报告.accounting.prior_year_net_income");
    const amount = record(available, "WASM 公开报告.accounting.prior_year_net_income.Available");
    exactKeys(amount, ["amount"], "WASM 公开报告.accounting.prior_year_net_income.Available");
    accountingDecimal(amount.amount, "WASM 公开报告.accounting.prior_year_net_income.Available.amount");
  } else if (unavailable !== undefined) {
    exactKeys(comparative, ["Unavailable"], "WASM 公开报告.accounting.prior_year_net_income");
    const reason = record(unavailable, "WASM 公开报告.accounting.prior_year_net_income.Unavailable");
    exactKeys(reason, ["reason"], "WASM 公开报告.accounting.prior_year_net_income.Unavailable");
    if (reason.reason !== "NoPriorYearHistory") throw new TypeError("WASM 公开报告比较期不可用原因无效");
  } else {
    throw new TypeError("WASM 公开报告比较期字段不符合公共 DTO 契约");
  }
  return report as PublicReportSummary;
}

export function normalizePublicReportPage(value: unknown): PublicReportPage {
  const page = record(normalizeSerdeMaps(value), "WASM 公开报告页");
  exactKeys(page, ["reports", "next_cursor"], "WASM 公开报告页");
  if (!Array.isArray(page.reports)) {
    throw new TypeError("WASM 公开报告页字段类型不符合公共 DTO 契约");
  }
  const nextCursor = page.next_cursor === null ? null : opaqueDecimalId(page.next_cursor, "WASM 公开报告页.next_cursor");
  return { reports: page.reports.map(normalizePublicReport), next_cursor: nextCursor };
}

export function normalizePublicReportById(value: unknown): PublicReportSummary {
  return normalizePublicReport(normalizeSerdeMaps(value));
}

/**
 * serde-wasm-bindgen 将 Rust Map 默认解码为 JS Map；UI、Redux 与 JSON 存档要求普通对象。
 * 所有 WASM 边界统一经过这个递归转换，避免某个宿主遗漏嵌套字段。
 */
export function normalizeSerdeMaps<T>(value: unknown): T {
  if (typeof value === "bigint") {
    const numericValue = Number(value);
    if (!Number.isSafeInteger(numericValue)) {
      throw new RangeError(`WASM 整数超出 JavaScript 安全整数范围：${value}`);
    }
    return numericValue as T;
  }
  if (value instanceof Map) {
    const record: Record<string, unknown> = {};
    for (const [key, entry] of value.entries()) {
      record[String(key)] = normalizeSerdeMaps(entry);
    }
    return record as T;
  }
  if (Array.isArray(value)) return value.map(normalizeSerdeMaps) as T;
  if (value !== null && typeof value === "object") {
    const record: Record<string, unknown> = {};
    for (const [key, entry] of Object.entries(value)) {
      record[key] = normalizeSerdeMaps(entry);
    }
    return record as T;
  }
  return value as T;
}

/**
 * 准备 JSON 形态的存档供 serde-wasm-bindgen 反序列化。
 *
 * JSON 对象键只能是字符串；Rust `BTreeMap<AccountId, _>` 经本地存档往返后会得到
 * `"0"` 这样的键，而 serde-wasm-bindgen 的 u64 map key 需要数值。仅在 WASM 入站
 * 边界恢复这一处 Map，远程/Tauri 的 JSON 协议仍保持标准对象形态。
 */
export function prepareSaveForWasm<T extends WasmSaveMaps>(slot: T): unknown {
  const accountMap = (entries: Record<string, unknown>, label: string): Map<number, unknown> => {
    const result = new Map<number, unknown>();
    for (const [rawAccountId, value] of Object.entries(entries)) {
    if (!/^\d+$/.test(rawAccountId)) {
        throw new Error(`${label}账户 ID 不是非负十进制整数：${rawAccountId}`);
    }
    const accountId = Number(rawAccountId);
    if (!Number.isSafeInteger(accountId)) {
        throw new Error(`${label}账户 ID 超出 JavaScript 安全整数范围：${rawAccountId}`);
    }
      result.set(accountId, value);
    }
    return result;
  };
  const accounts = accountMap(slot.snapshot.accounts, "");
  const npcAttention = accountMap(slot.npc_attention, "NPC 注意力");
  const retailExperience = accountMap(slot.retail_experience, "散户经历");
  const parentOrders = accountMap(slot.parent_orders, "机构母单");
  const strategyProfiles = accountMap(slot.strategy_profiles, "策略身份档案");
  const informationStates = accountMap(slot.information_states, "个人信息");
  const beliefBooks = accountMap(slot.belief_books, "个人信念");
  const watchlists = accountMap(slot.watchlists, "个人关注");
  const priceMemories = accountMap(slot.price_memories, "价格记忆");
  const plans = accountMap(slot.plans.plans, "交易计划");
  return {
    ...slot,
    npc_attention: npcAttention,
    retail_experience: retailExperience,
    parent_orders: parentOrders,
    strategy_profiles: strategyProfiles,
    information_states: informationStates,
    belief_books: beliefBooks,
    watchlists,
    price_memories: priceMemories,
    plans: {
      ...slot.plans,
      plans,
    },
    snapshot: {
      ...slot.snapshot,
      accounts,
    },
  };
}
