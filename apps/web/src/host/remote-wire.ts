import { observerOrdinal } from "./observer-resync.ts";
import { prepareBaselineUpdate, createProtocolUpdate, type HostFailure, type HostUpdate } from "./host-update.ts";
import { parseHostFailure } from "./protocol-failure.ts";

export type RemoteMessage =
  | { readonly kind: "baseline"; readonly fencing: boolean; readonly protocol: 1 | 2; readonly lease: string | null; readonly failure?: HostFailure; readonly update: Extract<HostUpdate, { type: "baseline" }> }
  | { readonly kind: "observer-cut"; readonly captureId: string; readonly ordinal: string; readonly lease: string | null; readonly failure?: HostFailure; readonly update: Extract<HostUpdate, { type: "baseline" }> }
  | { readonly kind: "observer-fenced"; readonly captureId: string; readonly accepted: boolean }
  | { readonly kind: "observer-released"; readonly captureId: string; readonly accepted: boolean; readonly generation: string; readonly lease: string; readonly ordinal: string }
  | { readonly kind: "publication-accepted"; readonly accepted: boolean; readonly generation: string; readonly lease: string; readonly ordinal: string }
  | { readonly kind: "terminal"; readonly generation: string; readonly lease: string; readonly missing: { from: string; through: string } | null; readonly failure: HostFailure }
  | { readonly kind: "protocol"; readonly lease: string | null; readonly ordinal: string | null; readonly update: Extract<HostUpdate, { type: "protocol" }> }
  | { readonly kind: "failure"; readonly failure: HostFailure }
  | { readonly kind: "publication-failure"; readonly generation: string; readonly lease: string; readonly ordinal: string; readonly failure: HostFailure }
  | { readonly kind: "resync"; readonly message: string }
  | { readonly kind: "empty" }
  | { readonly kind: "queued"; readonly requestId: number }
  | { readonly kind: "gateway-error"; readonly requestId: number | null; readonly failure: HostFailure };

function record(value: unknown, where: string): Readonly<Record<string, unknown>> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Error(`${where} 必须是对象`);
  return value as Readonly<Record<string, unknown>>;
}

function exact(source: Readonly<Record<string, unknown>>, keys: readonly string[], where: string): void {
  const actual = Object.keys(source);
  if (actual.length !== keys.length || keys.some((key) => !Object.hasOwn(source, key))) throw new Error(`${where} 字段不符合远程协议契约`);
}

function optionalExact(source: Readonly<Record<string, unknown>>, required: readonly string[], optional: readonly string[], where: string): void {
  const actual = Object.keys(source);
  if (required.some((key) => !Object.hasOwn(source, key)) || actual.some((key) => !required.includes(key) && !optional.includes(key))) throw new Error(`${where} 字段不符合远程协议契约`);
}

function generation(value: unknown, where: string): string {
  if (!Number.isSafeInteger(value) || Number(value) < 0) throw new Error(`${where} 必须是非负安全整数`);
  return String(value);
}

function metadata(source: Readonly<Record<string, unknown>>, where: string): { readonly civilDate: string; readonly revision: string } {
  if (typeof source.civil_date !== "string" || !/^\d{4}-\d{2}-\d{2}$/.test(source.civil_date)) throw new Error(`${where}.civil_date 必须是 ISO 日期`);
  if (!Number.isSafeInteger(source.public_revision) || Number(source.public_revision) < 0) throw new Error(`${where}.public_revision 必须是非负安全整数`);
  return { civilDate: source.civil_date, revision: String(source.public_revision) };
}

function failure(value: unknown, where: string): HostFailure {
  return parseHostFailure(value, where);
}

function baseline(source: Readonly<Record<string, unknown>>): Extract<RemoteMessage, { kind: "baseline" }> {
  optionalExact(source, ["timeline_generation", "snapshot", "civil_date", "public_revision", "public_report_ids", "stock_catalog", "company_catalog"], ["observer_fencing", "observer_protocol", "observer_lease", "failure"], "远程 Baseline");
  if (source.observer_fencing !== undefined && source.observer_fencing !== true) throw new Error("远程观察能力无效");
  if (!Array.isArray(source.public_report_ids) || source.public_report_ids.some((id) => typeof id !== "string" || !/^(0|[1-9]\d*)$/.test(id))) throw new Error("远程 Baseline.public_report_ids 无效");
  if (source.observer_protocol !== undefined && source.observer_protocol !== 2) throw new Error("远程观察协议版本无效");
  const lease = source.observer_protocol === 2 ? observerOrdinal(source.observer_lease) : null;
  if (source.observer_protocol === undefined && source.observer_lease !== undefined) throw new Error("旧观察协议不得携带未协商租约");
  return {
    kind: "baseline", protocol: source.observer_protocol === 2 ? 2 : 1, lease,
    ...(source.failure == null ? {} : { failure: failure(source.failure, "远程基线故障") }),
    fencing: source.observer_fencing === true,
    update: prepareBaselineUpdate(generation(source.timeline_generation, "远程 Baseline.timeline_generation"), source.snapshot, source.stock_catalog, source.company_catalog, metadata(source, "远程 Baseline"), [], source.public_report_ids),
  };
}

export function parseRemoteMessage(raw: string, activeGeneration: string | null = null): RemoteMessage {
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch (error) {
    throw new Error(`远程消息不是合法 JSON：${error instanceof Error ? error.message : String(error)}`);
  }
  const envelope = record(parsed, "远程消息");
  if (Object.keys(envelope).length !== 1) throw new Error("远程消息必须且只能包含一个消息变体");
  if (Object.hasOwn(envelope, "Baseline")) return baseline(record(envelope.Baseline, "远程 Baseline"));
  if (Object.hasOwn(envelope, "ObserverCut")) {
    const { capture_id, publication_ordinal, ...snapshot } = record(envelope.ObserverCut, "远程 ObserverCut");
    const parsedBaseline = baseline(snapshot);
    return { kind: "observer-cut", captureId: observerOrdinal(capture_id), ordinal: observerOrdinal(publication_ordinal), lease: parsedBaseline.lease,
      ...(parsedBaseline.failure === undefined ? {} : { failure: parsedBaseline.failure }), update: parsedBaseline.update };
  }
  if (Object.hasOwn(envelope, "ObserverFenced")) {
    const source = record(envelope.ObserverFenced, "远程 ObserverFenced");
    exact(source, ["capture_id", "accepted"], "远程 ObserverFenced");
    if (typeof source.accepted !== "boolean") throw new Error("远程 ObserverFenced.accepted 无效");
    return { kind: "observer-fenced", captureId: observerOrdinal(source.capture_id), accepted: source.accepted };
  }
  if (Object.hasOwn(envelope, "ObserverReleased") || Object.hasOwn(envelope, "PublicationAccepted")) {
    const released = Object.hasOwn(envelope, "ObserverReleased");
    const source = record(released ? envelope.ObserverReleased : envelope.PublicationAccepted, "远程观察确认");
    exact(source, [...(released ? ["capture_id"] : []), "timeline_generation", "observer_lease", "publication_ordinal", "accepted"], "远程观察确认");
    if (typeof source.accepted !== "boolean") throw new Error("远程观察确认 accepted 无效");
    const identity = { generation: observerOrdinal(source.timeline_generation), lease: observerOrdinal(source.observer_lease), ordinal: observerOrdinal(source.publication_ordinal), accepted: source.accepted };
    return released ? { kind: "observer-released", captureId: observerOrdinal(source.capture_id), ...identity } : { kind: "publication-accepted", ...identity };
  }
  if (Object.hasOwn(envelope, "PublicationFailure")) {
    const source = record(envelope.PublicationFailure, "远程有序故障");
    exact(source, ["timeline_generation", "observer_lease", "publication_ordinal", "failure"], "远程有序故障");
    return { kind: "publication-failure", generation: observerOrdinal(source.timeline_generation), lease: observerOrdinal(source.observer_lease),
      ordinal: observerOrdinal(source.publication_ordinal), failure: failure(source.failure, "远程有序故障") };
  }
  if (Object.hasOwn(envelope, "ObserverTerminal")) {
    const source = record(envelope.ObserverTerminal, "远程 terminal");
    exact(source, ["timeline_generation", "observer_lease", "missing", "failure"], "远程 terminal");
    let missing: { from: string; through: string } | null = null;
    if (source.missing !== null) {
      const range = record(source.missing, "terminal 缺口"); exact(range, ["from", "through"], "terminal 缺口");
      missing = { from: observerOrdinal(range.from), through: observerOrdinal(range.through) };
      if (BigInt(missing.from) > BigInt(missing.through)) throw new Error("terminal 缺口范围回卷");
    }
    return { kind: "terminal", generation: observerOrdinal(source.timeline_generation), lease: observerOrdinal(source.observer_lease), missing, failure: failure(source.failure, "远程 terminal") };
  }
  if (Object.hasOwn(envelope, "PublisherFrame")) {
    const source = record(envelope.PublisherFrame, "远程 PublisherFrame");
    optionalExact(source, ["timeline_generation", "update", "civil_date", "public_revision"], ["failure", "observer_lease", "publication_ordinal"], "远程 PublisherFrame");
    if (source.failure !== undefined && source.failure !== null) throw new Error("远程 PublisherFrame 不得混入 HostFailure");
    if (source.update === null || source.update === undefined) throw new Error("远程 PublisherFrame 必须包含完整 EngineUpdate");
    if (Object.hasOwn(source, "observer_lease") !== Object.hasOwn(source, "publication_ordinal")) throw new Error("发布租约和序号必须同时存在");
    return { kind: "protocol", lease: source.observer_lease === undefined ? null : observerOrdinal(source.observer_lease),
      ordinal: source.publication_ordinal === undefined ? null : observerOrdinal(source.publication_ordinal), update: createProtocolUpdate(generation(source.timeline_generation, "远程 PublisherFrame.timeline_generation"), source.update, metadata(source, "远程 PublisherFrame")) };
  }
  if (Object.hasOwn(envelope, "HostFailure")) return { kind: "failure", failure: failure(envelope.HostFailure, "远程 HostFailure") };
  if (Object.hasOwn(envelope, "ResyncRequired")) {
    const source = record(envelope.ResyncRequired, "远程 ResyncRequired");
    exact(source, ["reason", "missed"], "远程 ResyncRequired");
    if (typeof source.reason !== "string") throw new Error("远程 ResyncRequired.reason 必须是字符串");
    if (source.missed !== null && (!Number.isSafeInteger(source.missed) || Number(source.missed) < 0)) throw new Error("远程 ResyncRequired.missed 无效");
    return { kind: "resync", message: source.reason };
  }
  if (Object.hasOwn(envelope, "FrameEmpty")) {
    exact(record(envelope.FrameEmpty, "远程 FrameEmpty"), [], "远程 FrameEmpty");
    return { kind: "empty" };
  }
  if (Object.hasOwn(envelope, "CommandQueued")) {
    const source = record(envelope.CommandQueued, "远程 CommandQueued");
    exact(source, ["request_id"], "远程 CommandQueued");
    if (!Number.isSafeInteger(source.request_id) || Number(source.request_id) < 0) throw new Error("远程 CommandQueued.request_id 无效");
    return { kind: "queued", requestId: Number(source.request_id) };
  }
  if (Object.hasOwn(envelope, "GatewayError")) {
    const source = record(envelope.GatewayError, "远程 GatewayError");
    const details = Object.fromEntries(Object.entries(source).filter(([key]) => key !== "request_id"));
    const requestId = source.request_id;
    if (requestId !== null && (!Number.isSafeInteger(requestId) || Number(requestId) < 0)) throw new Error("远程 GatewayError.request_id 无效");
    return { kind: "gateway-error", requestId: requestId === null ? null : Number(requestId), failure: failure(details, "远程 GatewayError") };
  }
  if (activeGeneration !== null) throw new Error(`远程消息不是当前 generation ${activeGeneration} 的完整协议更新；已拒绝旧版 flat 事件或帧`);
  throw new Error("远程消息必须是 Baseline、PublisherFrame、HostFailure、ResyncRequired 或控制消息");
}
