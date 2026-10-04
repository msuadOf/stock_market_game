import type { Cents, PositionSnap } from "../types/engine.ts";
import { moneyFromBigInt, moneyToBigInt } from "../utils/money.ts";

function integer(value: number, field: string): bigint {
  if (!Number.isSafeInteger(value)) throw new RangeError(`${field} 必须是安全整数，实际 ${value}`);
  return BigInt(value);
}

export function valueHeldPosition(position: PositionSnap, currentPrice: Cents) {
  const quantity = integer(position.qty, "持仓股数");
  if (quantity <= 0n) throw new RangeError("持仓估值要求正股数");
  const price = moneyToBigInt(currentPrice);
  if (price <= 0n) throw new RangeError("持仓行情价格必须为正数");
  const netInvested = moneyToBigInt(position.invested_cents) - moneyToBigInt(position.recovered_cents);
  const magnitude = netInvested < 0n ? -netInvested : netInvested;
  let cost = magnitude / quantity;
  const doubledRemainder = (magnitude % quantity) * 2n;
  if (doubledRemainder > quantity || (doubledRemainder === quantity && cost % 2n !== 0n)) cost += 1n;
  if (netInvested < 0n) cost = -cost;
  return {
    avgCost: moneyFromBigInt(cost),
    marketValue: moneyFromBigInt(price * quantity),
    pnl: moneyFromBigInt((price - cost) * quantity),
  };
}
