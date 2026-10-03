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

before(async () => {
  vite = await createServer({ root: fileURLToPath(new URL("../../", import.meta.url)), configFile: false, appType: "custom", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  ({ NpcDecisionInspector } = await vite.ssrLoadModule("/src/dev/NpcDecisionInspector.tsx") as typeof import("./NpcDecisionInspector.tsx"));
});

after(async () => {
  if (vite) await vite.close();
});

test("requires the active EngineHost and gives the standalone page an explicit no-host message", () => {
  const html = renderToStaticMarkup(createElement(NpcDecisionInspector));
  assert.match(html, /没有活动 EngineHost/);
  assert.doesNotMatch(html, /推进一 tick|初始化 NPC 决策诊断/);
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
