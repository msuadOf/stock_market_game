import { parseSaveSlot } from "./save-schema.ts";
import type { StrictSaveEnvelope } from "./schema/root.ts";

export type DayEndSaveReference = { readonly seq: number; readonly settledDate: string };

export function validateDayEndArchive(slot: StrictSaveEnvelope): StrictSaveEnvelope {
  if (slot.civil_clock.settled_through === null) throw new Error("存档没有已完成的日终结算");
  if (slot.snapshot.tick % slot.setup.ticks_per_day !== 0) throw new Error("日内快照不能作为日终存档加载");
  if (slot.pending_player.length > 0 || (slot.pending_npc !== null && slot.pending_npc.intents.length > 0)) {
    throw new Error("日终存档不能包含未处理的日内请求");
  }
  if (Object.values(slot.resting_orders).some((orders) => orders.length > 0)
    || Object.values(slot.auction_orders).some((orders) => orders.length > 0)
    || Object.values(slot.parent_orders).some((plans) => Object.keys(plans).length > 0)
    || slot.runtime_v2.live_envelopes.length > 0
    || slot.npc_order_lifecycles.length > 0) {
    throw new Error("日终存档不能包含日内活动委托或冻结资源");
  }
  return slot;
}

export function validateDayEndCandidate(value: unknown, reference: DayEndSaveReference) {
  const slot = parseSaveSlot(value);
  if (slot.snapshot.seq !== reference.seq || slot.civil_clock.settled_through !== reference.settledDate) {
    throw new Error(`日终候选不属于请求的自然日 ${reference.settledDate} / seq ${reference.seq}`);
  }
  return validateDayEndArchive(slot);
}
