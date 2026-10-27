import type { CompanyCatalogEntry } from "../types/generated/CompanyCatalogEntry.ts";
import type { StockIdentityEntry } from "../types/generated/StockIdentityEntry.ts";
import { exact, field, record, safeU32, text, values } from "./protocol/guards.ts";
import { ProtocolError } from "./protocol/types.ts";

function invalid(path: string, message: string): never {
  throw new ProtocolError("PROTOCOL_MALFORMED", path, message);
}

function identityText(value: unknown, path: string): string {
  const parsed = text(value, path);
  if (parsed.trim().length === 0) invalid(path, `${path} 不得为空`);
  return parsed;
}

/** 本代 actual 目录的只读翻译器；不按数组位置分配身份。 */
export interface CatalogResolver {
  readonly stockCatalog: readonly StockIdentityEntry[];
  readonly companyCatalog: readonly CompanyCatalogEntry[];
  stockId(code: string): number;
  stockCode(id: number, path?: string): string;
  companyCode(id: number, path?: string): string;
  companyId(code: string): number;
  reportScopeCode(company: number, scope: unknown): string;
}

export function prepareCatalogResolver(stockValue: unknown, companyValue: unknown): CatalogResolver {
  const stockCatalog = values(stockValue, "stockCatalog").map((entry, index): StockIdentityEntry => {
    const path = `stockCatalog[${index}]`;
    const source = record(entry, path);
    exact(source, ["id", "code"], path);
    return Object.freeze({ id: safeU32(field(source, "id", path), `${path}.id`), code: identityText(field(source, "code", path), `${path}.code`) });
  });
  const companyCatalog = values(companyValue, "companyCatalog").map((entry, index): CompanyCatalogEntry => {
    const path = `companyCatalog[${index}]`;
    const source = record(entry, path);
    exact(source, ["id", "code", "name", "listed_stock"], path);
    const listed = field(source, "listed_stock", path);
    return Object.freeze({ id: safeU32(field(source, "id", path), `${path}.id`), code: identityText(field(source, "code", path), `${path}.code`), name: text(field(source, "name", path), `${path}.name`), listed_stock: listed === null ? null : safeU32(listed, `${path}.listed_stock`) });
  });
  const stocks = new Map<number, string>();
  const stockCodes = new Set<string>();
  const stockIds = new Map<string, number>();
  for (const entry of stockCatalog) {
    if (stocks.has(entry.id) || stockCodes.has(entry.code)) invalid("stockCatalog", "StockId 与 code 必须分别唯一");
    stocks.set(entry.id, entry.code);
    stockIds.set(entry.code, entry.id);
    stockCodes.add(entry.code);
  }
  const companies = new Map<number, string>();
  const companyIds = new Map<string, number>();
  const issuers = new Set<number>();
  for (const entry of companyCatalog) {
    if (companies.has(entry.id) || companyIds.has(entry.code)) invalid("companyCatalog", "CompanyId 与 code 必须分别唯一");
    companies.set(entry.id, entry.code);
    companyIds.set(entry.code, entry.id);
    if (entry.listed_stock !== null) {
      if (!stocks.has(entry.listed_stock) || issuers.has(entry.listed_stock)) invalid("companyCatalog", "上市证券必须存在且只能有一个 issuer");
      issuers.add(entry.listed_stock);
    }
  }
  if (issuers.size !== stocks.size) invalid("companyCatalog", "issuer 必须精确覆盖实际 StockCatalog");
  const companyCode = (id: number, path = "CompanyId"): string => {
    safeU32(id, path);
    const code = companies.get(id);
    if (code === undefined) invalid(path, `${path}：未知 CompanyId：${id}`);
    return code;
  };
  return Object.freeze({
    stockCatalog: Object.freeze(stockCatalog), companyCatalog: Object.freeze(companyCatalog),
    stockId(code: string): number {
      const id = stockIds.get(code);
      if (id === undefined) invalid("stockCatalog.code", `未知股票代码：${code}（不在本代 StockCatalog）`);
      return id;
    },
    stockCode(id: number, path = "StockId"): string {
      safeU32(id, path);
      const code = stocks.get(id);
      if (code === undefined) invalid(path, `未知股票 StockId：${id}`);
      return code;
    },
    companyCode,
    companyId(code: string): number {
      const id = companyIds.get(code);
      if (id === undefined) invalid("companyCatalog.code", `未知公司 code：${code}`);
      return id;
    },
    reportScopeCode(company: number, scopeValue: unknown): string {
      const scope = record(scopeValue, "report.scope");
      let member: number;
      if (Object.hasOwn(scope, "Standalone")) {
        exact(scope, ["Standalone"], "report.scope");
        const payload = record(scope.Standalone, "report.scope.Standalone");
        exact(payload, ["entity_id"], "report.scope.Standalone");
        member = safeU32(field(payload, "entity_id", "report.scope"), "report.scope.entity_id");
      } else {
        exact(scope, ["Consolidated"], "report.scope");
        const payload = record(scope.Consolidated, "report.scope.Consolidated");
        exact(payload, ["root_entity_id"], "report.scope.Consolidated");
        member = safeU32(field(payload, "root_entity_id", "report.scope"), "report.scope.root_entity_id");
      }
      // groups.rs 与 simple/finance_state.rs 现将 CompanyId 一对一投影为 MemberId。
      // 显式检查该投影及 ensure_scope_mirrors_company，不将其他成员冒充报告公司。
      const code = companyCode(member, "report.scope");
      if (member !== company) invalid("report.scope", "报告 scope 必须对应 report.company_id");
      return code;
    },
  });
}
