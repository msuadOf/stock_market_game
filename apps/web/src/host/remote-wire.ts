import { createBaselineUpdate, createProtocolUpdate, type HostFailure, type HostUpdate } from "./host-update.ts";
import { parseProtocolSnapshot } from "./protocol/index.ts";

export type RemoteMessage =
  | { readonly kind: "baseline"; readonly update: Extract<HostUpdate, { type: "baseline" }> }
  | { readonly kind: "protocol"; readonly update: Extract<HostUpdate, { type: "protocol" }> }
  | { readonly kind: "failure"; readonly failure: HostFailure }
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
  const source = record(value, where);
  exact(source, ["code", "message"], where);
  if (typeof source.code !== "string" || source.code.length === 0 || typeof source.message !== "string" || source.message.length === 0) {
    throw new Error(`${where} 必须包含非空字符串 code 和 message`);
  }
  return { code: source.code, where, message: source.message };
}

function baseline(source: Readonly<Record<string, unknown>>): Extract<RemoteMessage, { kind: "baseline" }> {
  exact(source, ["timeline_generation", "snapshot", "civil_date", "public_revision", "public_report_ids"], "远程 Baseline");
  if (!Array.isArray(source.public_report_ids) || source.public_report_ids.some((id) => typeof id !== "string" || !/^(0|[1-9]\d*)$/.test(id))) throw new Error("远程 Baseline.public_report_ids 无效");
  return {
    kind: "baseline",
    update: createBaselineUpdate(generation(source.timeline_generation, "远程 Baseline.timeline_generation"), parseProtocolSnapshot(source.snapshot, "远程 Baseline.snapshot"), metadata(source, "远程 Baseline"), [], source.public_report_ids),
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
  if (Object.hasOwn(envelope, "PublisherFrame")) {
    const source = record(envelope.PublisherFrame, "远程 PublisherFrame");
    optionalExact(source, ["timeline_generation", "update", "civil_date", "public_revision"], ["failure"], "远程 PublisherFrame");
    if (source.failure !== undefined && source.failure !== null) throw new Error("远程 PublisherFrame 不得混入 HostFailure");
    if (source.update === null || source.update === undefined) throw new Error("远程 PublisherFrame 必须包含完整 EngineUpdate");
    return { kind: "protocol", update: createProtocolUpdate(generation(source.timeline_generation, "远程 PublisherFrame.timeline_generation"), source.update, metadata(source, "远程 PublisherFrame")) };
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
    exact(source, ["request_id", "code", "message"], "远程 GatewayError");
    const requestId = source.request_id;
    if (requestId !== null && (!Number.isSafeInteger(requestId) || Number(requestId) < 0)) throw new Error("远程 GatewayError.request_id 无效");
    return { kind: "gateway-error", requestId: requestId === null ? null : Number(requestId), failure: failure({ code: source.code, message: source.message }, "远程 GatewayError") };
  }
  if (activeGeneration !== null) throw new Error(`远程消息不是当前 generation ${activeGeneration} 的完整协议更新；已拒绝旧版 flat 事件或帧`);
  throw new Error("远程消息必须是 Baseline、PublisherFrame、HostFailure、ResyncRequired 或控制消息");
}
