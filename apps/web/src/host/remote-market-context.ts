import type { SessionSetup } from "../types/engine.ts";
import { parseSetup } from "../save/schema/market.ts";
import { parseMoney } from "../utils/money.ts";
import { boolean, exact, record, text } from "./protocol/guards.ts";

export interface RemoteMarketContext {
  readonly session_id: string;
  readonly setup: SessionSetup;
  readonly seed: string;
  readonly resumed: boolean;
  readonly generation: string;
  readonly member: { readonly account_id: string; readonly admission_funding: { readonly external_cash: string } } | null;
  readonly can_control: boolean;
  readonly needs_rejoin: boolean;
}

export function canonicalU64(value: unknown, path: string): string {
  if (typeof value !== "string" || !/^(0|[1-9]\d*)$/.test(value) || value.length > 20 || BigInt(value) > 18446744073709551615n) throw new Error(`${path} 必须是规范 u64 十进制字符串`);
  return value;
}

export function parseRemoteMarketContext(value: unknown): RemoteMarketContext {
  const source = record(value, "远程市场 context");
  exact(source, ["session_id", "setup", "seed", "resumed", "generation", "member", "can_control", "needs_rejoin"], "远程市场 context");
  const sessionId = text(source.session_id, "context.session_id");
  if (sessionId.trim().length === 0) throw new Error("context.session_id 不能为空");
  let member: RemoteMarketContext["member"] = null;
  if (source.member !== null) {
    const entry = record(source.member, "context.member");
    exact(entry, ["account_id", "admission_funding"], "context.member");
    const funding = record(entry.admission_funding, "context.member.admission_funding");
    exact(funding, ["external_cash"], "context.member.admission_funding");
    const externalCash = parseMoney(funding.external_cash, "context.member.admission_funding.external_cash");
    if (BigInt(externalCash) < 0n) throw new Error("入场 external_cash 不能为负分金额");
    member = { account_id: canonicalU64(entry.account_id, "context.member.account_id"), admission_funding: { external_cash: externalCash } };
  }
  const needsRejoin = boolean(source.needs_rejoin, "context.needs_rejoin");
  if (member !== null && needsRejoin) throw new Error("context 已有 member 不能同时要求重新加入");
  return {
    session_id: sessionId,
    setup: parseSetup(source.setup, "context.setup"),
    seed: canonicalU64(source.seed, "context.seed"),
    resumed: boolean(source.resumed, "context.resumed"),
    generation: canonicalU64(source.generation, "context.generation"),
    member,
    can_control: boolean(source.can_control, "context.can_control"),
    needs_rejoin: needsRejoin,
  };
}
