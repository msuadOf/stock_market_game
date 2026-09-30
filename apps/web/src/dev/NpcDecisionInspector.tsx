import { useEffect, useMemo, useRef, useState } from "react";
import type { EngineHost } from "../host/engine-host.ts";
import type { NpcDecisionTraceRecord } from "../host/npc-decision-trace.ts";
import { InspectorRequestGate } from "./inspector-request-gate.ts";
import "./npc-decision-inspector.css";

type InspectorState =
  | { readonly kind: "ready"; readonly records: readonly NpcDecisionTraceRecord[] }
  | { readonly kind: "failed"; readonly message: string };

const DEFAULT_NPC_ACCOUNT = "1";

export function NpcDecisionInspector({ host }: { readonly host?: EngineHost } = {}) {
  const [accountText, setAccountText] = useState(DEFAULT_NPC_ACCOUNT);
  const [requestBusy, setRequestBusy] = useState(false);
  const requestGate = useRef(new InspectorRequestGate());
  const [state, setState] = useState<InspectorState>(() => host?.capabilities.npcDecisionDiagnostics && host.npcDecisionTrace
    ? { kind: "ready", records: [] }
    : { kind: "failed", message: host === undefined ? "此独立页面没有活动 EngineHost，请从游戏 DEV 入口打开检查器" : "当前后端未协商启用 NPC 决策诊断" });

  useEffect(() => {
    requestGate.current.invalidate();
    setRequestBusy(false);
    setState(host?.capabilities.npcDecisionDiagnostics && host.npcDecisionTrace
      ? { kind: "ready", records: [] }
      : { kind: "failed", message: host === undefined ? "此独立页面没有活动 EngineHost，请从游戏 DEV 入口打开检查器" : "当前后端未协商启用 NPC 决策诊断" });
    return () => requestGate.current.invalidate();
  }, [host]);

  const account = useMemo(() => parseAccountId(accountText), [accountText]);
  const refresh = async () => {
    if (state.kind !== "ready" || requestBusy || account === null || host?.npcDecisionTrace === undefined) return;
    const request = requestGate.current.begin(host);
    setRequestBusy(true);
    try {
      const records = await host.npcDecisionTrace(account);
      if (!requestGate.current.isCurrent(request, host)) return;
      setState({ kind: "ready", records });
    } catch (error) {
      if (!requestGate.current.isCurrent(request, host)) return;
      setState({ kind: "failed", message: error instanceof Error ? error.message : String(error) });
    } finally {
      if (requestGate.current.isCurrent(request, host)) setRequestBusy(false);
    }
  };

  if (state.kind === "failed") return <main className="npc-inspector" role="alert">NPC 决策诊断初始化失败：{state.message}</main>;
  return <main className="npc-inspector" aria-label="NPC 决策检查器" aria-busy={requestBusy}>
    <header className="npc-inspector__header">
      <h1>NPC 决策检查器</h1>
      <p>仅开发环境。读取最近 128 条因果记录，不写入存档、快照或公开状态。</p>
    </header>
    <div className="npc-inspector__controls">
      <label htmlFor="npc-account">NPC 账户 ID</label>
      <input id="npc-account" inputMode="numeric" value={accountText} onChange={(event) => setAccountText(event.target.value)} />
      <button type="button" onClick={() => void refresh()} disabled={host?.npcDecisionTrace === undefined || account === null || requestBusy}>{requestBusy ? "正在读取…" : "读取当前会话记录"}</button>
    </div>
    {account === null && <p role="alert">账户 ID 必须是非负安全整数。</p>}
    <p className="npc-inspector__count">已读取 {state.records.length} / 128 条记录</p>
    <ol className="npc-inspector__records" aria-label="NPC 决策记录">
      {state.records.map((record) => <TraceRecord key={`${record.tick}-${record.order_ids.join("-")}`} record={record} />)}
    </ol>
    {state.records.length === 0 && <p className="npc-inspector__empty">该 NPC 尚无可显示的决策链记录。</p>}
  </main>;
}

function TraceRecord({ record }: { readonly record: NpcDecisionTraceRecord }) {
  return <li>
    <strong>tick {record.tick}</strong>
    <dl>
      <dt>股票</dt><dd>{record.codes.join("、") || "无"}</dd>
      <dt>报告</dt><dd>{record.source_report_ids.join("、") || "无新增报告"}</dd>
      <dt>预期方法</dt><dd>{record.expectation_method ?? "未形成预期"}</dd>
      <dt>计划</dt><dd>{record.plan_ids.join("、") || "无"}</dd>
      <dt>预算/状态</dt><dd>{record.budget_constraints.join("、") || "无约束记录"}</dd>
      <dt>订单</dt><dd>{record.order_ids.join("、") || "未提交"}</dd>
    </dl>
  </li>;
}

function parseAccountId(value: string): number | null {
  if (!/^\d+$/.test(value)) return null;
  const account = Number(value);
  return Number.isSafeInteger(account) ? account : null;
}
