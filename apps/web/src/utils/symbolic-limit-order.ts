import type { Intent, LimitPrice } from "../types/engine";
import { parseYuanPrice } from "./trade-input.ts";

export type PlayerOrderKind = "limit" | "market";
export type LimitPriceChoice = "fixed" | "highest" | "lowest";

export function buildPlayerOrderIntent(
  code: string,
  side: "Buy" | "Sell",
  qty: number,
  orderKind: PlayerOrderKind,
  priceChoice: LimitPriceChoice,
  priceText: string,
): Intent {
  if (orderKind === "market") return { PlaceMarket: { code, side, qty } };
  const price: LimitPrice = priceChoice === "fixed"
    ? { Fixed: parseYuanPrice(priceText) }
    : priceChoice === "highest" ? "Highest" : "Lowest";
  return { PlaceLimit: { code, side, price, qty } };
}

export function playerOrderDescription(orderKind: PlayerOrderKind, priceChoice: LimitPriceChoice, priceText: string): string {
  if (orderKind === "market") return "市价";
  if (priceChoice === "highest") return "最高限价";
  if (priceChoice === "lowest") return "最低限价";
  return `限价 @ ${priceText} 元`;
}

export function orderPriceInputState(orderKind: PlayerOrderKind, priceChoice: LimitPriceChoice): { disabled: boolean; placeholder: string } {
  if (orderKind === "market") return { disabled: true, placeholder: "市价委托无需价格" };
  if (priceChoice === "highest") return { disabled: true, placeholder: "最高限价按受理时规则确定" };
  if (priceChoice === "lowest") return { disabled: true, placeholder: "最低限价按受理时规则确定" };
  return { disabled: false, placeholder: "委托价" };
}
