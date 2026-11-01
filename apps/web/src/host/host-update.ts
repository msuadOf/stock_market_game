import { prepareCatalogResolver } from "./catalog-resolver.ts";
import { normalizeSerdeValue } from "./protocol/guards.ts";
import { parseProtocolSnapshot } from "./protocol/parse.ts";
import type { CatalogResolver } from "./catalog-resolver.ts";
import type { StockSpec } from "../types/engine.ts";
import type { CodedSnapshot } from "./protocol/stock-identity.ts";

export const UI_UPDATE_INTERVAL_MS = 16;
export const UI_TARGET_HZ = 1_000 / UI_UPDATE_INTERVAL_MS;

export type PublicMetadata = {
  readonly civilDate: string | null;
  readonly revision: string | null;
};

export type HostUpdate =
  | ({
      readonly type: "baseline";
      readonly continuation?: true;
      readonly generation: string;
      readonly snapshot: CodedSnapshot;
      readonly securities: readonly StockSpec[];
      readonly catalogResolver: CatalogResolver;
      readonly stockCatalog: CatalogResolver["stockCatalog"];
      readonly companyCatalog: CatalogResolver["companyCatalog"];
      readonly publicPublicationIds: readonly string[];
    } & PublicMetadata)
  | ({
      readonly type: "protocol";
      readonly generation: string;
      readonly update: unknown;
    } & PublicMetadata);

export type HostFailure = {
  readonly code: string;
  readonly where: string;
  readonly message: string;
  readonly cause?: unknown;
  readonly context?: unknown;
  readonly recoverable?: boolean | null;
  readonly recoveryActions?: readonly string[] | null;
};

export function createBaselineUpdate(
  generation: string,
  snapshot: CodedSnapshot,
  catalogResolver: CatalogResolver,
  metadata: PublicMetadata = { civilDate: null, revision: null },
  securities: readonly StockSpec[] = [],
  publicPublicationIds: readonly string[] = [],
): Extract<HostUpdate, { type: "baseline" }> {
  if (!/^(0|[1-9]\d*)$/.test(generation)) {
    throw new Error("宿主基线 generation 必须是规范非负十进制整数");
  }
  if (!Number.isSafeInteger(snapshot.seq) || snapshot.seq < 0) {
    throw new Error("宿主基线快照缺少非负安全整数 seq");
  }
  const codes = new Set(catalogResolver.stockCatalog.map(entry => entry.code));
  if (codes.size !== Object.keys(snapshot.markets).length || Object.keys(snapshot.markets).some(code => !codes.has(code))) {
    throw new Error("宿主基线 snapshot 市场必须与实际 StockCatalog 精确对应");
  }
  return { type: "baseline", generation, snapshot, catalogResolver, stockCatalog: catalogResolver.stockCatalog, companyCatalog: catalogResolver.companyCatalog, ...metadata, securities, publicPublicationIds };
}

export function createProtocolUpdate(
  generation: string,
  update: unknown,
  metadata: PublicMetadata = { civilDate: null, revision: null },
): Extract<HostUpdate, { type: "protocol" }> {
  if (!/^(0|[1-9]\d*)$/.test(generation)) {
    throw new Error("宿主协议更新 generation 必须是规范非负十进制整数");
  }
  return { type: "protocol", generation, update, ...metadata };
}

/** 原始 wire 在任何宿主缓存或 Redux 发布前完成完整准备。 */
export function prepareBaselineUpdate(
  generation: string, snapshot: unknown, stockCatalog: unknown, companyCatalog: unknown,
  metadata: PublicMetadata, securities: readonly StockSpec[] = [], publicPublicationIds: readonly string[] = [],
): Extract<HostUpdate, { type: "baseline" }> {
  const resolver = prepareCatalogResolver(normalizeSerdeValue(stockCatalog, "stockCatalog"), normalizeSerdeValue(companyCatalog, "companyCatalog"));
  const parsed = parseProtocolSnapshot(snapshot, "baseline.snapshot", resolver);
  return createBaselineUpdate(generation, parsed, resolver, metadata, securities, publicPublicationIds);
}
