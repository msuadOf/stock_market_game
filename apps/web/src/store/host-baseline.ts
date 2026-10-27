import { createAction } from "@reduxjs/toolkit";
import type { CompanyCatalogEntry } from "../types/generated/CompanyCatalogEntry.ts";
import type { StockIdentityEntry } from "../types/generated/StockIdentityEntry.ts";
import type { CodedSnapshot } from "../host/protocol/stock-identity.ts";

/** 两个 slice 共享一次 dispatch，订阅者只观察完整同代身份。 */
export const installHostBaseline = createAction<{
  readonly snapshot: CodedSnapshot;
  readonly generation: string;
  readonly companyGeneration: number;
  readonly companyCatalog: readonly CompanyCatalogEntry[];
  readonly stockCatalog: readonly StockIdentityEntry[];
  readonly civilDate: string | null;
  readonly revision: string | null;
}>("host/installBaseline");
