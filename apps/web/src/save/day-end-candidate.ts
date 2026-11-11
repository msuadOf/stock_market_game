import { parseSaveSlot } from "./save-schema.ts";
import type { StrictSaveEnvelope } from "./schema/root.ts";

export type DayEndSaveReference = { readonly seq: number; readonly settledDate: string };

export function validateDayEndArchive(slot: StrictSaveEnvelope): StrictSaveEnvelope {
  validateDayEndArchiveFacts({
    settledThrough: slot.civil_clock.settled_through,
    tick: slot.snapshot.tick,
    ticksPerDay: slot.setup.ticks_per_day,
    hasPendingPlayer: slot.pending_player.length > 0,
    hasIntradayOrders: Object.values(slot.resting_orders).some(orders => orders.length > 0)
      || slot.plans.plans.some(entry => "Investor" in entry
        ? entry.Investor.execution !== undefined || entry.Investor.active_child_order_id !== null
        : entry.IssuerRepurchase.execution !== null)
      || slot.runtime_state.live_envelopes.length > 0 || slot.npc_order_lifecycles.length > 0,
    hasActiveMinutes: Object.values(slot.market_minute_bars).some(bars => bars.length > 0),
  });
  return slot;
}

export type DayEndArchiveFacts = Readonly<{
  settledThrough: string | null;
  tick: number;
  ticksPerDay: number;
  hasPendingPlayer: boolean;
  hasIntradayOrders: boolean;
  hasActiveMinutes: boolean;
}>;

/** 完整图与分片摘要共享同一日终门禁；调用前必须完成全部 schema。 */
export function validateDayEndArchiveFacts(facts: DayEndArchiveFacts): void {
  if (facts.settledThrough === null) throw new Error("存档没有已完成的日终结算");
  if (facts.tick % facts.ticksPerDay !== 0) throw new Error("日内快照不能作为日终存档加载");
  if (facts.hasPendingPlayer) throw new Error("日终存档不能包含未处理的日内请求");
  // 母单执行状态由计划本体唯一持有；日终须结束整个执行腿并释放活动子单和冻结资源。
  if (facts.hasIntradayOrders) throw new Error("日终存档不能包含日内活动委托或冻结资源");
  // 日结须把真实分钟事实冻结进保留链并清空活动桶，不裁剪或补造成交。
  if (facts.hasActiveMinutes) throw new Error("日终存档不能包含日内活动分钟（应当已冻结进保留历史）");
}

export function validateDayEndCandidate(value: unknown, reference: DayEndSaveReference) {
  const slot = parseSaveSlot(value);
  if (slot.snapshot.seq !== reference.seq || slot.civil_clock.settled_through !== reference.settledDate) {
    throw new Error(`日终候选不属于请求的自然日 ${reference.settledDate} / seq ${reference.seq}`);
  }
  return validateDayEndArchive(slot);
}
