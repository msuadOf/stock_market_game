import { STOCK_LIST } from "../config/defaults.ts";
import { prepareCatalogResolver } from "./catalog-resolver.ts";

/** 仅供既有默认 fixture 使用；生产摄入必须显式提供 actual baseline 目录。 */
export const defaultStockCatalogFixture = [...STOCK_LIST].sort((a, b) => a.code.localeCompare(b.code)).map((stock, id) => ({ id, code: stock.code }));
export const defaultCompanyCatalogFixture = defaultStockCatalogFixture.map(stock => ({ id: stock.id, code: `C-${stock.code}`, name: `公司${stock.code}`, listed_stock: stock.id }));
export const defaultCatalogFixture = prepareCatalogResolver(defaultStockCatalogFixture, defaultCompanyCatalogFixture);
const snapshotCatalogs = new Map<string, ReturnType<typeof prepareCatalogResolver>>();
export function catalogFixtureForSnapshot(snapshot: { readonly markets: Readonly<Record<string, unknown>> }) {
  const codes = new Set(Object.keys(snapshot.markets));
  const stocks = defaultStockCatalogFixture.filter(stock => codes.has(stock.code));
  const key = stocks.map(stock => stock.id).join(",");
  const existing = snapshotCatalogs.get(key);
  if (existing !== undefined) return existing;
  const resolver = prepareCatalogResolver(stocks, defaultCompanyCatalogFixture.filter(company => stocks.some(stock => stock.id === company.listed_stock)));
  snapshotCatalogs.set(key, resolver);
  return resolver;
}
export const numericCompanyCatalogFixture = prepareCatalogResolver([], ["600001", "600002", "600003", "11", "22", "1", "2", "3"].map(code => ({ id: Number(code), code, name: code, listed_stock: null })));

/** 仅更新测试 wire fixture；原 wire keys 对应默认 fixture 的原 id。 */
export function catalogWireFixture(snapshot: unknown) {
  const source = snapshot as { readonly markets?: Readonly<Record<string, unknown>> } | null;
  const ids = new Set(Object.keys(source?.markets ?? {}));
  const stock_catalog = defaultStockCatalogFixture.filter(stock => ids.has(String(stock.id)));
  return { stock_catalog, company_catalog: defaultCompanyCatalogFixture.filter(company => stock_catalog.some(stock => stock.id === company.listed_stock)) };
}
export function catalogWorkerFixture(snapshot: unknown) {
  const source = catalogWireFixture(snapshot);
  return { stockCatalog: source.stock_catalog, companyCatalog: source.company_catalog };
}
