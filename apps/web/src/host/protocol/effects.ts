import type { ContinuousPoint } from "../../types/generated/ContinuousPoint.ts";
import type { Event } from "../../types/generated/Event.ts";
import type { EventFact } from "../../types/generated/EventFact.ts";
import { factIdentity } from "./canonical.ts";
import { ProtocolError, type AutomaticOrderPoint, type ProtocolEffect } from "./types.ts";

function assertNever(value: never): never {
  throw new ProtocolError("PROTOCOL_MALFORMED", "protocol.effects", `未处理拒单原因：${String(value)}`);
}

function effectsFromEvent(event: Event): readonly ProtocolEffect[] {
  if ("IntentRejected" in event) {
    return [{ kind: "notice", message: `委托被拒：${event.IntentRejected.code} - ${rejectionMessage(event.IntentRejected.reason)}` }];
  }
  if ("SettlementError" in event) {
    return [{ kind: "notice", message: `结算错误：${event.SettlementError.code} - ${event.SettlementError.reason}` }];
  }
  if ("Trade" in event) return [{ kind: "trade", event: event.Trade }];
  return [];
}

function rejectionMessage(reason: Extract<Event, { IntentRejected: unknown }>["IntentRejected"]["reason"]): string {
  switch (reason) {
    case "InsufficientCash":
      return "资金不足";
    case "InsufficientShares":
      return "持仓不足";
    case "LimitExceeded":
      return "超出涨跌停限制";
    case "PriceCageExceeded":
      return "委托价格超出连续竞价价格笼子";
    case "UnknownStock":
      return "未知股票";
    case "AuctionLimitOrderRequired":
      return "集合竞价仅接受限价委托";
    case "AuctionOrderNotCancelable":
      return "集合竞价委托当前不可撤销";
    case "AuctionOrderEntryClosed":
      return "09:25-09:30 不接受新委托";
    case "InvalidQuantity":
      return "委托数量不符合 A 股交易单位";
    case "OrderNotFound":
      return "委托不存在或已撤销";
    case "OrderAlreadyFilled":
      return "委托已全部成交，无法撤单";
    case "NotOrderOwner":
      return "不能撤销其他账户的委托";
    default:
      return assertNever(reason);
  }
}

function sortedPoints(points: Readonly<Record<string, ContinuousPoint>>): readonly AutomaticOrderPoint[] {
  return Object.entries(points)
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([code, point]) => ({ code, ...point }));
}

export function effectsFromFacts(
  facts: readonly EventFact[],
  continuousPoints: Readonly<Record<string, ContinuousPoint>>,
): readonly ProtocolEffect[] {
  const effects = facts
    .slice()
    .sort((left, right) => factIdentity(left).localeCompare(factIdentity(right)))
    .flatMap((fact) => effectsFromEvent(fact.event));
  const points = sortedPoints(continuousPoints);
  if (points.length > 0) effects.push({ kind: "automatic-order", points });
  return effects;
}

export function civilBarrierEffects(kinds: readonly string[]): readonly ProtocolEffect[] {
  const effects: ProtocolEffect[] = [];
  if (kinds.includes("AfterClose")) {
    effects.push({ kind: "civil-barrier", barrier: "AfterClose", message: "收盘后刷新已完成" });
  }
  if (kinds.includes("BeforeOpen")) {
    effects.push({ kind: "civil-barrier", barrier: "BeforeOpen", message: "开盘前资讯刷新已完成" });
  }
  return effects;
}
