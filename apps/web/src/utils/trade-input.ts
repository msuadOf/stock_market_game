import type { SecurityCategory, Side } from "../types/engine";

export const A_SHARE_LOT_SIZE = 100;
export const A_SHARE_MAX_ORDER_QUANTITY = 1_000_000;

export function parseYuanPrice(text: string): number {
  const normalized = text.trim();
  const match = /^(\d+)(?:\.(\d{1,2}))?$/.exec(normalized);
  if (!match) throw new Error("价格必须是最多两位小数的正数");
  const yuan = BigInt(match[1]!);
  const decimals = (match[2] ?? "").padEnd(2, "0");
  const cents = yuan * 100n + BigInt(decimals || "0");
  if (cents <= 0n || cents > BigInt(Number.MAX_SAFE_INTEGER)) {
    throw new Error("价格超出可处理范围");
  }
  return Number(cents);
}

export function parseShareQuantity(text: string): number {
  const normalized = text.trim();
  if (!/^\d+$/.test(normalized)) throw new Error("数量必须是正整数股");
  const quantity = Number(normalized);
  if (!Number.isSafeInteger(quantity) || quantity <= 0) throw new Error("数量必须是正整数股");
  if (quantity > A_SHARE_MAX_ORDER_QUANTITY) {
    throw new Error(`单笔数量不能超过 ${A_SHARE_MAX_ORDER_QUANTITY} 股`);
  }
  return quantity;
}

export function maxAShareOrderQuantity(category: SecurityCategory, isMarket = false): number {
  return category === "ChiNext" ? (isMarket ? 150_000 : 300_000) : A_SHARE_MAX_ORDER_QUANTITY;
}

export function validateAShareQuantity(
  side: Side,
  quantity: number,
  sellable: number,
  maxQuantity = A_SHARE_MAX_ORDER_QUANTITY,
): void {
  if (quantity > maxQuantity) throw new Error(`该证券单笔数量不能超过 ${maxQuantity} 股`);
  if (side === "Buy") {
    if (quantity % A_SHARE_LOT_SIZE !== 0) throw new Error("买入数量必须是 100 股的整数倍");
    return;
  }
  if (quantity > sellable) throw new Error(`可卖数量不足：当前可卖 ${sellable} 股`);
  const remainder = sellable % A_SHARE_LOT_SIZE;
  if (quantity % A_SHARE_LOT_SIZE !== 0 && quantity % A_SHARE_LOT_SIZE !== remainder) {
    throw new Error("卖出零股时必须一次性包含全部不足 100 股的余股");
  }
}

/** 与 engine 相同的 A 股正数四舍五入和至少一价位保护；金额单位为分。 */
export function aSharePriceLimits(
  lastClose: number,
  category: SecurityCategory,
): { down: number; up: number } {
  if (!Number.isSafeInteger(lastClose) || lastClose <= 0) {
    throw new Error("昨收价必须是可安全处理的正整数分");
  }
  const limitBps = category === "ChiNext" ? 2_000n : 1_000n;
  const close = BigInt(lastClose);
  const roundHalfUp = (ratioBps: bigint) => (close * ratioBps + 5_000n) / 10_000n;
  const roundedUp = roundHalfUp(10_000n + limitBps);
  const roundedDown = roundHalfUp(10_000n - limitBps);
  const upBigInt = roundedUp > close + 1n ? roundedUp : close + 1n;
  const cappedDown = roundedDown < close - 1n ? roundedDown : close - 1n;
  const downBigInt = cappedDown > 1n ? cappedDown : 1n;
  if (upBigInt > BigInt(Number.MAX_SAFE_INTEGER)) {
    throw new Error("涨跌停价格超出可安全处理范围");
  }
  const up = Number(upBigInt);
  const down = Number(downBigInt);
  return { down, up };
}
