import type { EngineHost } from "./engine-host.ts";
import type { CatalogResolver } from "./catalog-resolver.ts";
import { companyCivilDate, ownerAccount, parseCompanyCapabilities, parseOwnerRightsOfferings, parsePeriodChangeExplanation } from "./company-contract-views.ts";
import { parseAccountDividendTaxStatus, parseDividendTaxOutstandingViews, parseOwnerFlatWithholdingReceipts } from "./dividend-tax.ts";
import { exact, record } from "../save/schema/primitives.ts";

export type CompanyReadQuery =
  | { readonly kind: "CompanyCapabilities"; readonly data: { readonly company: number } }
  | { readonly kind: "CompanyPeriodExplanation"; readonly data: { readonly company: number; readonly period_end: string } }
  | { readonly kind: "AccountDividendTaxStatus" | "DividendTaxOutstandingViews" | "OwnerFlatWithholdingReceipts" | "OwnerRightsOfferings" };

type CompanyReadQueries = Required<Pick<EngineHost, "queryCompanyCapabilities" | "queryCompanyPeriodExplanation" | "queryAccountDividendTaxStatus" | "queryDividendTaxOutstandingViews" | "queryOwnerFlatWithholdingReceipts" | "queryOwnerRightsOfferings">>;

function assertIssuerStock(catalog: CatalogResolver, issuer: number, stock: string): void {
  const company = catalog.companyCatalog.find(entry => entry.id === issuer);
  const stockId = catalog.stockId(stock);
  if (company === undefined || company.listed_stock !== stockId) throw new Error(`公司查询的发行人 issuer ${issuer} 与证券 ${stock} 不符合当前目录映射`);
}

/** 三宿主只转发当前 Rust 查询；请求不能选择本人账户。 */
export function createCompanyReadQueries(request: (query: CompanyReadQuery) => Promise<unknown>, identity: () => CatalogResolver, expectedOwner?: string): CompanyReadQueries {
  async function read(query: CompanyReadQuery) {
    const path = `company_read.${query.kind}`;
    const response = record(await request(query), path);
    exact(response, ["owner", "value"], path);
    const owner = ownerAccount(response.owner, `${path}.owner`);
    if (expectedOwner !== undefined && owner !== expectedOwner) throw new Error(`${path}.owner 必须是当前本地玩家 ${expectedOwner}`);
    const value = record(response.value, `${path}.value`);
    exact(value, ["kind", "data"], `${path}.value`);
    if (value.kind !== query.kind) throw new Error(`${path}.value.kind 必须与请求 ${query.kind} 一致`);
    return { owner, data: value.data };
  }
  return {
    async queryCompanyCapabilities(companyCode) {
      const company = identity().companyId(companyCode);
      const { data } = await read({ kind: "CompanyCapabilities", data: { company } });
      const result = parseCompanyCapabilities(data);
      const catalog = identity();
      for (const rights of result.owner_rights) assertIssuerStock(catalog, company, rights.stock);
      return result;
    },
    async queryCompanyPeriodExplanation(companyCode, periodEnd) {
      const company = identity().companyId(companyCode);
      const period_end = companyCivilDate(periodEnd, "company_read.period_end");
      const { data } = await read({ kind: "CompanyPeriodExplanation", data: { company, period_end } });
      return parsePeriodChangeExplanation(data);
    },
    async queryAccountDividendTaxStatus() {
      const { data } = await read({ kind: "AccountDividendTaxStatus" });
      const result = parseAccountDividendTaxStatus(data);
      for (const stock of result.stocks) identity().stockId(stock.stock);
      return result;
    },
    async queryDividendTaxOutstandingViews() {
      const { owner, data } = await read({ kind: "DividendTaxOutstandingViews" });
      const result = parseDividendTaxOutstandingViews(data, owner);
      for (const stock of result) identity().stockId(stock.stock);
      return result;
    },
    async queryOwnerFlatWithholdingReceipts() {
      const { owner, data } = await read({ kind: "OwnerFlatWithholdingReceipts" });
      return parseOwnerFlatWithholdingReceipts(data, owner);
    },
    async queryOwnerRightsOfferings() {
      const { data } = await read({ kind: "OwnerRightsOfferings" });
      const result = parseOwnerRightsOfferings(data);
      const catalog = identity();
      for (const rights of result) assertIssuerStock(catalog, rights.issuer, rights.stock);
      return result;
    },
  };
}
