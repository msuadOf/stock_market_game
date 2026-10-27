import type { EngineHost } from "./engine-host.ts";
import type { CatalogResolver } from "./catalog-resolver.ts";

export type CompanyReadQuery =
  | { readonly kind: "CompanyCapabilities"; readonly data: { readonly company: number } }
  | { readonly kind: "CompanyPeriodExplanation"; readonly data: { readonly company: number; readonly period_end: string } }
  | { readonly kind: "AccountDividendTaxStatus" | "DividendTaxOutstandingViews" | "OwnerFlatWithholdingReceipts" | "OwnerRightsOfferings" };

type CompanyReadQueries = Required<Pick<EngineHost, "queryCompanyCapabilities" | "queryCompanyPeriodExplanation" | "queryAccountDividendTaxStatus" | "queryDividendTaxOutstandingViews" | "queryOwnerFlatWithholdingReceipts" | "queryOwnerRightsOfferings">>;

/** 三宿主只转发当前 Rust 查询；请求不能选择本人账户。 */
export function createCompanyReadQueries(_request: (query: CompanyReadQuery) => Promise<unknown>, _identity: () => CatalogResolver): CompanyReadQueries {
  const unavailable = async (): Promise<never> => { throw new Error("公司只读宿主适配尚未实现"); };
  return { queryCompanyCapabilities: unavailable, queryCompanyPeriodExplanation: unavailable, queryAccountDividendTaxStatus: unavailable, queryDividendTaxOutstandingViews: unavailable, queryOwnerFlatWithholdingReceipts: unavailable, queryOwnerRightsOfferings: unavailable };
}
