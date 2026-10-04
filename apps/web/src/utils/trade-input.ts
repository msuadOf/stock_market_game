import type { SecurityCategory, Side } from "../types/engine";
import { moneyFromBigInt, moneyToBigInt, yuanTextToCents } from "./money.ts";

export const A_SHARE_LOT_SIZE = 100;
export const A_SHARE_MAX_ORDER_QUANTITY = 1_000_000;

export function parseYuanPrice(text: string): string {
  try {
    const cents = yuanTextToCents(text);
    if (moneyToBigInt(cents) <= 0n) throw new Error("价格必须是最多两位小数的正数");
    return cents;
  } catch (error) {
    if (!(error instanceof Error)) throw error;
    throw new Error(`价格输入无效：${error.message}`, { cause: error });
  }
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
  lastClose: string,
  category: SecurityCategory,
): { down: string; up: string } {
  const close = moneyToBigInt(lastClose);
  if (close <= 0n) throw new Error("昨收价必须是正整数分");
  const limitBps = category === "ChiNext" ? 2_000n : 1_000n;
  const roundHalfUp = (ratioBps: bigint) => (close * ratioBps + 5_000n) / 10_000n;
  const roundedUp = roundHalfUp(10_000n + limitBps);
  const roundedDown = roundHalfUp(10_000n - limitBps);
  const upBigInt = roundedUp > close + 1n ? roundedUp : close + 1n;
  const cappedDown = roundedDown < close - 1n ? roundedDown : close - 1n;
  const downBigInt = cappedDown > 1n ? cappedDown : 1n;
  const up = moneyFromBigInt(upBigInt);
  const down = moneyFromBigInt(downBigInt);
  return { down, up };
}
