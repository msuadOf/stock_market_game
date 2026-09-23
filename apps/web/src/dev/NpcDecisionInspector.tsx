import { useEffect, useMemo, useState } from "react";
import init, * as wasm from "../../wasm-diagnostics-pkg/web_wasm.js";
import { DEFAULT_SEED, DEFAULT_SETUP } from "../config/defaults.ts";
import { parseNpcDecisionDiagnostics, type NpcDecisionTraceRecord } from "../host/npc-decision-trace.ts";
import "./npc-decision-inspector.css";

type InspectorState =
  | { readonly kind: "loading" }
  | { readonly kind: "ready"; readonly handle: number; readonly records: readonly NpcDecisionTraceRecord[] }
  | { readonly kind: "failed"; readonly message: string };

const DEFAULT_NPC_ACCOUNT = "1";
const DIAGNOSTIC_SETUP = {
  ...DEFAULT_SETUP,
  npcs: {
    retail_count: 0,
    inst_count: 1,
    hot_count: 0,
    retail_cash_median: DEFAULT_SETUP.npcs.retail_cash_median,
  },
  ticks_per_day: 130,
  auction_ticks: 0,
  closing_auction_ticks: 0,
};

export function NpcDecisionInspector() {
  const [accountText, setAccountText] = useState(DEFAULT_NPC_ACCOUNT);
  const [state, setState] = useState<InspectorState>({ kind: "loading" });

  useEffect(() => {
    void initialize().then(setState, (error: unknown) => {
      setState({ kind: "failed", message: error instanceof Error ? error.message : String(error) });
    });
  }, []);

  const account = useMemo(() => parseAccountId(accountText), [accountText]);
  const refresh = () => {
    if (state.kind !== "ready" || account === null) return;
    try {
      const result = parseNpcDecisionDiagnostics(wasm.npc_decision_trace(state.handle, account));
      if (result.kind === "unsupported") throw new Error("当前诊断 WASM 不支持 NPC 决策追踪");
      setState({ ...state, records: result.records });
    } catch (error) {
      setState({ kind: "failed", message: error instanceof Error ? error.message : String(error) });
    }
  };
  const advance = (steps: number) => {
    if (state.kind !== "ready") return;
    for (let index = 0; index < steps; index += 1) wasm.step(state.handle);
    refresh();
  };

  if (state.kind === "loading") return <main className="npc-inspector" aria-busy="true">正在初始化 NPC 决策诊断…</main>;
  if (state.kind === "failed") return <main className="npc-inspector" role="alert">NPC 决策诊断初始化失败：{state.message}</main>;
  return <main className="npc-inspector" aria-label="NPC 决策检查器">
    <header className="npc-inspector__header">
      <h1>NPC 决策检查器</h1>
      <p>仅开发环境。读取最近 128 条因果记录，不写入存档、快照或公开状态。</p>
    </header>
    <div className="npc-inspector__controls">
      <label htmlFor="npc-account">NPC 账户 ID</label>
      <input id="npc-account" inputMode="numeric" value={accountText} onChange={(event) => setAccountText(event.target.value)} />
      <button type="button" onClick={refresh} disabled={account === null}>读取记录</button>
      <button type="button" onClick={() => advance(1)}>推进一 tick</button>
      <button type="button" onClick={() => advance(129)}>推进 129 ticks</button>
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

async function initialize(): Promise<InspectorState> {
  const response = await fetch(new URL("../../wasm-diagnostics-pkg/web_wasm_bg.wasm", import.meta.url));
  if (!response.ok) throw new Error(`诊断 WASM 加载失败：HTTP ${response.status}`);
  await init(new Uint8Array(await response.arrayBuffer()));
  const handle = wasm.create_session(DIAGNOSTIC_SETUP, DEFAULT_SEED);
  return { kind: "ready", handle, records: [] };
}

function parseAccountId(value: string): bigint | null {
  if (!/^\d+$/.test(value)) return null;
  const account = BigInt(value);
  return account <= BigInt(Number.MAX_SAFE_INTEGER) ? account : null;
}
