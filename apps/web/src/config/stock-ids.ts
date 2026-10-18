/**
 * 股票数值身份（StockId，ADR 0041）与 A 股代码串的唯一前端翻译表。
 *
 * 引擎侧 `StockCatalog`（packages/engine/src/stock.rs）按代码字符串升序分配
 * `StockId = 0..n`；Rust `String` 的 `Ord` 是字节序，JavaScript `Array#sort()`
 * 默认按 UTF-16 码点序比较，对纯 ASCII 数字代码两者完全一致，因此本表用
 * `STOCK_LIST` 代码去重后排序取下标，与引擎的 id 分配逐一对齐。
 *
 * 未知代码 / 未知 id 一律显式抛错（铁律二），绝不静默回退。
 */
import { STOCK_LIST } from "./defaults.ts";

/** 引擎股票数值代号（u32）。 */
export type StockId = number;

const sortedCodes = [...new Set(STOCK_LIST.map((stock) => stock.code))].sort();

/** 代码串 → StockId。 */
export const stockIdByCode = new Map<string, StockId>(sortedCodes.map((code, index) => [code, index]));

/** StockId → 代码串。 */
export const codeByStockId = new Map<StockId, string>(sortedCodes.map((code, index) => [index, code]));

/** 代码串转 StockId；未知代码显式抛错。 */
export function stockIdOf(code: string): StockId {
  const id = stockIdByCode.get(code);
  if (id === undefined) {
    throw new Error(`未知股票代码：${code}（不在 STOCK_LIST 目录内）`);
  }
  return id;
}

/** StockId 转代码串；未知或非整数 id 显式抛错。 */
export function codeOfStock(id: StockId): string {
  const code = codeByStockId.get(id);
  if (code === undefined) {
    throw new Error(`未知股票 StockId：${id}（超出 STOCK_LIST 目录范围）`);
  }
  return code;
}
