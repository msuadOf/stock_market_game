import { parseShareQuantity, parseYuanPrice } from "../utils/trade-input.ts";
import type { LimitPriceChoice, PlayerOrderKind } from "../utils/symbolic-limit-order.ts";

export interface TradingFieldErrors { code?: string; price?: string; quantity?: string }

export function tradingFieldErrors(orderKind: PlayerOrderKind, priceChoice: LimitPriceChoice, priceText: string, quantityText: string, maxQuantity?: number): TradingFieldErrors {
  const errors: TradingFieldErrors = {};
  if (orderKind === "limit" && priceChoice === "fixed") {
    try { parseYuanPrice(priceText); } catch (failure) { errors.price = failure instanceof Error ? failure.message : String(failure); }
  }
  try {
    const quantity = parseShareQuantity(quantityText);
    if (maxQuantity !== undefined && quantity > maxQuantity) throw new Error(`该证券单笔数量不能超过 ${maxQuantity} 股`);
  } catch (failure) { errors.quantity = failure instanceof Error ? failure.message : String(failure); }
  return errors;
}
