import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";
import { fileURLToPath } from "node:url";
import type { EngineHost } from "../host/engine-host.ts";
import type { NpcDecisionTraceRecord } from "../host/npc-decision-trace.ts";

let vite: ViteDevServer;
let NpcDecisionInspector: typeof import("./NpcDecisionInspector.tsx").NpcDecisionInspector;
let TraceRecord: typeof import("./NpcDecisionInspector.tsx").TraceRecord;

before(async () => {
  vite = await createServer({ root: fileURLToPath(new URL("../../", import.meta.url)), configFile: false, appType: "custom", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  ({ NpcDecisionInspector, TraceRecord } = await vite.ssrLoadModule("/src/dev/NpcDecisionInspector.tsx") as typeof import("./NpcDecisionInspector.tsx"));
});

after(async () => {
  if (vite) await vite.close();
});

test("requires the active EngineHost and gives the standalone page an explicit no-host message", () => {
  const html = renderToStaticMarkup(createElement(NpcDecisionInspector));
  assert.match(html, /没有活动 EngineHost/);
  assert.doesNotMatch(html, /推进一 tick|初始化 NPC 决策诊断/);
});

test("renders lossless tick and actual plan transitions without numeric coercion", { timeout: 10_000 }, () => {
  const record: NpcDecisionTraceRecord = {
    account: 1, tick: "18446744073709551615", source_report_ids: ["2"],
    expectation_method: "CashFlow", plan_ids: [3], plan_changes: ["PlanId(3): Active -> Paused"],
    budget_constraints: ["PlanId(3): InsufficientAvailableCash"], order_ids: [4], codes: ["600001"],
  };
  const html = renderToStaticMarkup(createElement(TraceRecord, { record }));
  assert.match(html, /18446744073709551615/);
  assert.match(html, /计划变化/);
  assert.match(html, /Active -&gt; Paused/);
  assert.match(html, /InsufficientAvailableCash/);
});

test("renders the inspector from a diagnostics-enabled active host without creating another session", () => {
  const records: readonly NpcDecisionTraceRecord[] = [];
  const host = {
    capabilities: { npcDecisionDiagnostics: true },
    npcDecisionTrace: async () => records,
  } as unknown as EngineHost;
  const html = renderToStaticMarkup(createElement<NonNullable<Parameters<typeof NpcDecisionInspector>[0]>>(NpcDecisionInspector, { host }));
  assert.match(html, /读取当前会话记录/);
  assert.match(html, /当前会话/);
  assert.doesNotMatch(html, /推进一 tick|推进 129 ticks/);
});
