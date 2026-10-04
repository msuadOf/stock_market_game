import type { PositionSnap } from "../types/engine.ts";

function integer(value: number, field: string): bigint {
  if (!Number.isSafeInteger(value)) throw new RangeError(`${field} 必须是安全整数，实际 ${value}`);
  return BigInt(value);
}

function displayAmount(value: bigint, field: string): number {
  const result = Number(value);
  if (!Number.isSafeInteger(result)) throw new RangeError(`${field} 超出当前 Web 金额安全范围：${value}`);
  return result;
}

export function valueHeldPosition(position: PositionSnap, currentPrice: number) {
  const quantity = integer(position.qty, "持仓股数");
  if (quantity <= 0n) throw new RangeError("持仓估值要求正股数");
  const price = integer(currentPrice, "持仓行情价格");
  if (price <= 0n) throw new RangeError("持仓行情价格必须为正数");
  const netInvested = integer(position.invested_cents, "累计投入分") - integer(position.recovered_cents, "累计回收分");
  const magnitude = netInvested < 0n ? -netInvested : netInvested;
  let cost = magnitude / quantity;
  const doubledRemainder = (magnitude % quantity) * 2n;
  if (doubledRemainder > quantity || (doubledRemainder === quantity && cost % 2n !== 0n)) cost += 1n;
  if (netInvested < 0n) cost = -cost;
  return {
    avgCost: displayAmount(cost, "每股成本分"),
    marketValue: displayAmount(price * quantity, "持仓市值分"),
    pnl: displayAmount((price - cost) * quantity, "持仓浮盈分"),
  };
}
