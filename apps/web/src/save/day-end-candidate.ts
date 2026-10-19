import { parseSaveSlot } from "./save-schema.ts";
import type { StrictSaveEnvelope } from "./schema/root.ts";

export type DayEndSaveReference = { readonly seq: number; readonly settledDate: string };

export function validateDayEndArchive(slot: StrictSaveEnvelope): StrictSaveEnvelope {
  if (slot.civil_clock.settled_through === null) throw new Error("存档没有已完成的日终结算");
  if (slot.snapshot.tick % slot.setup.ticks_per_day !== 0) throw new Error("日内快照不能作为日终存档加载");
  // T3 批四：pending_npc 载体删除（旧格式档在根解析层显式拒绝）。
  if (slot.pending_player.length > 0) {
    throw new Error("日终存档不能包含未处理的日内请求");
  }
  if (Object.values(slot.resting_orders).some((orders) => orders.length > 0)
    // 架构项 8：母单执行状态唯一所有者是计划本体。日终必须整体结束执行腿
    // （引擎 end_of_trading_day 同时清除 execution 与 active_child_order_id），
    // 带腿或在途子单引用过日都不是合法日终存档。
    || Object.values(slot.plans.plans).some((plan) => plan.execution !== undefined || plan.active_child_order_id !== null)
    || slot.runtime_state.live_envelopes.length > 0
    || slot.npc_order_lifecycles.length > 0) {
    throw new Error("日终存档不能包含日内活动委托或冻结资源");
  }
  // B2 保存域（W3 接入）：日结清空当日分钟K桶（bars 冻结进保留链——
  // 引擎 clear_runtime_minute_history）。桶非空的档是日内档，不能当日终档。
  if (Object.values(slot.market_minute_bars).some((bars) => bars.length > 0)) {
    throw new Error("日终存档不能包含日内活动分钟（应当已冻结进保留历史）");
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
