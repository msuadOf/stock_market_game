import type { CivilUpdate } from "../../types/generated/CivilUpdate.ts";
import type { EngineUpdate } from "../../types/generated/EngineUpdate.ts";
import type { Event } from "../../types/generated/Event.ts";
import type { EventFact } from "../../types/generated/EventFact.ts";
import type { StockSpec } from "../../types/generated/StockSpec.ts";
import type { TickBatch } from "../../types/generated/TickBatch.ts";
import type { TickFrame } from "../../types/generated/TickFrame.ts";
import { canonicalJson, eventStableKey, factIdentity } from "./canonical.ts";
import { ProtocolError } from "./types.ts";

function malformed(where: string, message: string): never {
  throw new ProtocolError("PROTOCOL_MALFORMED", where, message);
}

function eventSeq(event: Event): number {
  if ("Trade" in event) return event.Trade.seq;
  if ("AuctionTick" in event) return event.AuctionTick.seq;
  if ("AuctionCompleted" in event) return event.AuctionCompleted.seq;
  if ("PriceTick" in event) return event.PriceTick.seq;
  if ("DayBoundary" in event) return event.DayBoundary.seq;
  if ("CivilDateAdvanced" in event) return event.CivilDateAdvanced.seq;
  if ("CompanyDisclosurePublished" in event) return event.CompanyDisclosurePublished.seq;
  if ("IntentRejected" in event) return event.IntentRejected.seq;
  if ("SettlementError" in event) return event.SettlementError.seq;
  if ("OrderCanceled" in event) return event.OrderCanceled.seq;
  if ("OrderAccepted" in event) return event.OrderAccepted.seq;
  return assertNever(event);
}

function assertNever(value: never): never {
  throw new ProtocolError("PROTOCOL_MALFORMED", "protocol.validate", `未处理事件变体：${String(value)}`);
}

function eventMap(events: readonly Event[], from: number, to: number, where: string): ReadonlyMap<number, string> {
  if (to < from || events.length !== to - from) {
    malformed(where, `事件覆盖必须精确等于 (${from}, ${to}]`);
  }
  const represented = new Map<number, string>();
  for (const event of events) {
    const seq = eventSeq(event);
    if (seq <= from || seq > to || represented.has(seq)) {
      malformed(where, `事件 seq 必须在 (${from}, ${to}] 内唯一`);
    }
    represented.set(seq, canonicalJson(event));
  }
  return represented;
}

export function validateFacts(
  facts: readonly EventFact[],
  events: readonly Event[],
  from: number,
  to: number,
  where: string,
): void {
  const supplied = eventMap(events, from, to, where);
  if (facts.length !== events.length) malformed(where, "事实数量必须等于事件数量");
  const identities = new Set<string>();
  const represented = new Map<number, string>();
  for (const fact of facts) {
    const identity = factIdentity(fact);
    const payload = canonicalJson(fact.event);
    if (
      identities.has(identity)
      || canonicalJson(fact.key) !== canonicalJson(eventStableKey(fact.event, fact.key.local_event_index))
      || fact.canonical_payload !== payload
      || represented.has(eventSeq(fact.event))
    ) {
      malformed(where, "事实身份、稳定键或规范载荷不匹配");
    }
    identities.add(identity);
    represented.set(eventSeq(fact.event), payload);
  }
  if (represented.size !== supplied.size) malformed(where, "事实与事件不构成一一映射");
  for (const [seq, payload] of supplied) {
    if (represented.get(seq) !== payload) malformed(where, "事实与事件 seq 到载荷映射不匹配");
  }
}

export function validateFrame(frame: TickFrame, where = "protocol.frame"): void {
  validateFacts(frame.facts, frame.events, frame.seq_from, frame.seq_to, where);
}

export function validateTickBatch(batch: TickBatch): void {
  if (batch.frames.length === 0) malformed("protocol.tick-batch", "TickBatch 不得为空");
  for (const [index, frame] of batch.frames.entries()) {
    validateFrame(frame, `protocol.tick-batch.frames[${index}]`);
    if (index > 0) {
      const previous = batch.frames[index - 1];
      if (previous === undefined || previous.tick + 1 !== frame.tick || previous.seq_to !== frame.seq_from) {
        malformed(`protocol.tick-batch.frames[${index}]`, "TickBatch 的 tick 与 seq 必须连续");
      }
    }
  }
  const finalFrame = batch.frames.at(-1);
  if (finalFrame === undefined) malformed("protocol.tick-batch", "TickBatch 不得为空");
  if (batch.runtime_snapshot !== null && (
    batch.runtime_snapshot.tick !== finalFrame.tick || batch.runtime_snapshot.seq !== finalFrame.seq_to
  )) {
    malformed("protocol.tick-batch.runtime_snapshot", "最终权威快照必须匹配最后帧 tick 与 seq");
  }
}

function civilAdvanceMatches(update: CivilUpdate): boolean {
  const advances = update.events.filter((event): event is Extract<Event, { CivilDateAdvanced: unknown }> => "CivilDateAdvanced" in event);
  const advance = advances[0];
  return advances.length === 1
    && advance !== undefined
    && advance.CivilDateAdvanced.settled_date === update.boundary.settled_date
    && advance.CivilDateAdvanced.next_date === update.boundary.next_date
    && canonicalJson(advance.CivilDateAdvanced.next_status) === canonicalJson(update.boundary.next_status);
}

function expectedKinds(update: CivilUpdate): readonly CivilUpdate["kinds"][number][] {
  if (update.boundary.settled_phase === "IntradayTrading") {
    return update.boundary.next_status === "Trading" ? ["AfterClose", "BeforeOpen"] : ["AfterClose"];
  }
  return update.boundary.next_status === "Trading" ? ["BeforeOpen"] : ["CivilAdvance"];
}

function nextDate(settledDate: string): string | null {
  const matched = /^(\d{4})-(\d{2})-(\d{2})$/.exec(settledDate);
  if (!matched) return null;
  const date = new Date(Date.UTC(Number(matched[1]), Number(matched[2]) - 1, Number(matched[3]) + 1));
  return `${date.getUTCFullYear()}-${String(date.getUTCMonth() + 1).padStart(2, "0")}-${String(date.getUTCDate()).padStart(2, "0")}`;
}

export function validateCivilUpdate(update: CivilUpdate): void {
  validateFacts(update.facts, update.events, update.seq_from, update.seq_to, "protocol.civil.facts");
  if (
    update.refresh.snapshot.tick !== update.tick
    || update.refresh.snapshot.seq !== update.seq_to
    || !civilAdvanceMatches(update)
    || canonicalJson(update.kinds) !== canonicalJson(expectedKinds(update))
    || nextDate(update.boundary.settled_date) !== update.boundary.next_date
    || update.boundary.next_date !== update.civil_date
    || (update.boundary.settled_phase === "IntradayTrading" && (
      update.tick === 0 || update.tick % update.refresh.ticks_per_day !== 0
    ))
  ) {
    malformed("protocol.civil", "CivilUpdate 的快照、屏障元数据或自然日事件不匹配");
  }
  const codes = new Set<string>(update.refresh.securities.map((security: StockSpec) => security.code));
  if (codes.size === 0 || codes.size !== update.refresh.securities.length || codes.size !== Object.keys(update.refresh.snapshot.markets).length) {
    malformed("protocol.civil.refresh", "CivilUpdate 证券与快照市场必须一一对应");
  }
  for (const code of Object.keys(update.refresh.snapshot.markets)) {
    if (!codes.has(code)) malformed("protocol.civil.refresh", "CivilUpdate 快照市场缺少证券定义");
  }
  const publicationIds = new Set(update.refresh.public_publication_ids);
  if (publicationIds.size !== update.refresh.public_publication_ids.length) malformed("protocol.civil.refresh", "公开披露 ID 不得重复");
  for (const event of update.events) {
    if ("CompanyDisclosurePublished" in event && !publicationIds.has(String(event.CompanyDisclosurePublished.publication_id))) {
      malformed("protocol.civil.refresh", "披露事件必须存在于公开披露 ID 刷新中");
    }
  }
  if (update.kinds.includes("AfterClose")) {
    if (update.refresh.intraday.length !== update.refresh.ticks_per_day) {
      malformed("protocol.civil.refresh.intraday", "收盘屏障必须保留完整日内帧");
    }
    validateFrames(update.refresh.intraday, "protocol.civil.refresh.intraday");
    const finalFrame = update.refresh.intraday.at(-1);
    const firstFrame = update.refresh.intraday[0];
    const expectedFirstTick = update.tick - update.refresh.ticks_per_day + 1;
    if (
      firstFrame === undefined
      || firstFrame.tick !== expectedFirstTick
      || finalFrame === undefined
      || finalFrame.tick !== update.tick
      || finalFrame.seq_to !== update.seq_from
    ) {
      malformed("protocol.civil.refresh.intraday", "收盘日内历史必须在 CivilUpdate 前一个 seq 结束");
    }
  } else if (update.refresh.intraday.length !== 0) {
    malformed("protocol.civil.refresh.intraday", "非收盘屏障不得携带日内历史");
  }
}

function validateFrames(frames: readonly TickFrame[], where: string): void {
  if (frames.length === 0) malformed(where, "帧序列不得为空");
  for (const [index, frame] of frames.entries()) {
    validateFrame(frame, `${where}[${index}]`);
    if (index > 0) {
      const previous = frames[index - 1];
      if (previous === undefined || previous.tick + 1 !== frame.tick || previous.seq_to !== frame.seq_from) {
        malformed(`${where}[${index}]`, "帧序列的 tick 与 seq 必须连续");
      }
    }
  }
}

export function validateEngineUpdate(update: EngineUpdate): void {
  if ("TickBatch" in update) {
    validateTickBatch(update.TickBatch);
    return;
  }
  if ("CivilUpdate" in update) {
    validateCivilUpdate(update.CivilUpdate);
    return;
  }
  assertNever(update);
}

export function canonicalEngineUpdate(update: EngineUpdate): string {
  if ("TickBatch" in update) {
    return canonicalJson({
      TickBatch: {
        frames: update.TickBatch.frames.map(canonicalFrame),
        runtime_snapshot: update.TickBatch.runtime_snapshot,
      },
    });
  }
  return canonicalJson({
    CivilUpdate: {
      ...update.CivilUpdate,
      events: [...update.CivilUpdate.events].sort((left, right) => eventSeq(left) - eventSeq(right)),
      facts: [...update.CivilUpdate.facts].sort((left, right) => factIdentity(left).localeCompare(factIdentity(right))),
      refresh: {
        ...update.CivilUpdate.refresh,
        intraday: update.CivilUpdate.refresh.intraday.map(canonicalFrame),
      },
    },
  });
}

function canonicalFrame(frame: TickFrame): TickFrame {
  return {
    ...frame,
    events: [...frame.events].sort((left, right) => eventSeq(left) - eventSeq(right)),
    facts: [...frame.facts].sort((left, right) => factIdentity(left).localeCompare(factIdentity(right))),
  };
}
