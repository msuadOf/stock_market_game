import assert from "node:assert/strict";
import test from "node:test";
import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { createTauriHost } from "./tauri-host.ts";
import { createRemoteHost } from "./remote-host.ts";
import { createWorkerHost } from "./worker-host.ts";
import { market, dailyCandle } from "./protocol-test-fixtures.ts";
import type { EngineHost } from "./engine-host.ts";
import type { SessionSetup } from "../types/engine.ts";

const snapshot = { seq: 0, tick: 0, day: 0, phase: "Continuous", markets: { "0": market() }, accounts: {}, daily_candles: { "0": [] }, active_daily_candles: { "0": dailyCandle() } };
const stocks = [{ id: 0, code: "600101" }];
const companies = [{ id: 7, code: "实际发行人", name: "公司", listed_stock: 0 }];
const status = { mode: "AShareIndividual", identity: "Personal", stocks: [{ stock: "600101", status: "IndividualPublicMarket" }] };
const flat = { payment_id: "pay1", plan_id: "plan1", account: "0", paid_on: "2030-06-01", gross: "1000", rate_bp: 1000, withheld: "100" };
type Calls = { query: unknown; generation: unknown; account?: unknown }[];
type Answer = (kind: string) => unknown | Promise<unknown>;
const answer: Answer = kind => ({ owner: "0", value: { kind, data: kind === "AccountDividendTaxStatus" ? status : kind === "OwnerFlatWithholdingReceipts" ? [flat] : [] } });

async function withHost(kind: "Worker" | "Tauri" | "Remote", body: (host: EngineHost, calls: Calls, respond: (value: Answer) => void) => Promise<void>) {
  const calls: Calls = []; let response = answer;
  let host: EngineHost | undefined;
  const restore: (() => void)[] = [];
  try {
    if (kind === "Worker") {
      const original = Object.getOwnPropertyDescriptor(globalThis, "Worker");
      class QueryWorker extends EventTarget {
        static current: QueryWorker;
        constructor() { super(); QueryWorker.current = this; }
        postMessage(raw: Record<string, unknown>) {
          if (raw.type !== "companyReadQuery") return;
          calls.push({ query: raw.query, generation: raw.generation, account: raw.account });
          const query = raw.query as { kind: string };
          void Promise.resolve(response(query.kind)).then(value => this.emit({ type: "companyReadQuery", requestId: raw.requestId, generation: raw.generation, result: value }));
        }
        terminate() {}
        emit(data: unknown) { this.dispatchEvent(Object.assign(new Event("message"), { data })); }
      }
      Object.defineProperty(globalThis, "Worker", { configurable: true, value: QueryWorker });
      restore.push(() => { if (original) Object.defineProperty(globalThis, "Worker", original); else Reflect.deleteProperty(globalThis, "Worker"); });
      const pending = createWorkerHost({} as SessionSetup, 1n);
      QueryWorker.current.emit({ type: "ingress", generation: 1, shared: false });
      QueryWorker.current.emit({ type: "baseline", generation: 1, snapshot, stockCatalog: stocks, companyCatalog: companies, civilDate: "2030-06-01" });
      host = await pending;
    } else if (kind === "Tauri") {
      const original = Object.getOwnPropertyDescriptor(globalThis, "window");
      Object.defineProperty(globalThis, "window", { configurable: true, value: { crypto: globalThis.crypto } });
      restore.push(() => { clearMocks(); if (original) Object.defineProperty(globalThis, "window", original); else Reflect.deleteProperty(globalThis, "window"); });
      mockIPC(async (command, input) => {
        if (command === "create_session") return "session-1";
        if (command === "host_capabilities") return { npcDecisionDiagnostics: false };
        if (command === "engine_baseline") return { generation: "1", timeline_id: "t1", snapshot, stock_catalog: stocks, company_catalog: companies, civil_date: "2030-06-01" };
        if (command === "company_read_query") {
          const args = input as Record<string, unknown>;
          calls.push({ query: args.query, generation: args.generation, account: args.account });
          return { generation: "1", ...await response((args.query as { kind: string }).kind) as Record<string, unknown> };
        }
        return null;
      }, { shouldMockEvents: true });
      host = await createTauriHost({} as SessionSetup, 1n);
    } else {
      const fetchFn = (async (input: string | URL | Request, init?: RequestInit) => {
        const url = String(input);
        if (url.endsWith("/api/new")) return new Response(JSON.stringify({ session_id: "session-1", session_token: "token-1" }));
        if (url.includes("/api/host-capabilities?")) return new Response(JSON.stringify({ npcDecisionDiagnostics: false }));
        if (url.endsWith("/api/company-read-query")) {
          assert.equal(init?.method, "POST");
          assert.equal((init?.headers as Record<string, string>).authorization, "Bearer token-1");
          const args = JSON.parse(String(init?.body)) as Record<string, unknown>;
          calls.push({ query: args.query, generation: args.generation, account: args.account });
          return new Response(JSON.stringify({ generation: "1", ...await response((args.query as { kind: string }).kind) as Record<string, unknown> }));
        }
        return new Response(null);
      }) as typeof fetch;
      const socket = { readyState: 1, close() {}, send() {}, onmessage: null, onclose: null, onerror: null } as unknown as WebSocket;
      host = await createRemoteHost({} as SessionSetup, 1n, { baseUrl: "http://127.0.0.1:3000", fetchFn, webSocketFactory: () => socket });
      await host.start(() => undefined);
      socket.onmessage!({ data: JSON.stringify({ Baseline: { timeline_generation: 1, snapshot, stock_catalog: stocks, company_catalog: companies, civil_date: "2030-06-01", public_revision: 0, public_report_ids: [] } }) } as MessageEvent);
    }
    await body(host, calls, next => { response = next; });
  } finally {
    await host?.dispose();
    for (const reset of restore.reverse()) reset();
  }
}

for (const kind of ["Worker", "Tauri", "Remote"] as const) {
  test(`${kind} 真实 host 注册公司只读查询，发送同代请求且不能指定其他账户`, { timeout: 10_000 }, async () => {
    await withHost(kind, async (host, calls) => {
      assert.deepEqual(await host.queryAccountDividendTaxStatus!(), status);
      assert.deepEqual(await host.queryOwnerFlatWithholdingReceipts!(), [flat]);
      assert.deepEqual(calls.map(call => call.query), [{ kind: "AccountDividendTaxStatus" }, { kind: "OwnerFlatWithholdingReceipts" }]);
      assert.equal(calls.every(call => call.generation === (kind === "Worker" ? 1 : "1") && call.account === undefined), true);
    });
  });
  test(`${kind} 真实 host 拒绝其他账户回执与错 kind，不用空列表掩盖失败`, { timeout: 10_000 }, async () => {
    await withHost(kind, async (host, _calls, respond) => {
      respond(() => ({ owner: "0", value: { kind: "OwnerFlatWithholdingReceipts", data: [{ ...flat, account: "9" }] } }));
      await assert.rejects(host.queryOwnerFlatWithholdingReceipts!(), /account/);
      respond(() => ({ owner: "0", value: { kind: "AccountDividendTaxStatus", data: status } }));
      await assert.rejects(host.queryOwnerRightsOfferings!(), /kind/);
    });
  });
  test(`${kind} 真实 host 销毁后拒绝迟到的本人查询成功`, { timeout: 10_000 }, async () => {
    await withHost(kind, async (host, _calls, respond) => {
      let complete!: (value: unknown) => void;
      respond(() => new Promise(resolve => { complete = resolve; }));
      const pending = host.queryAccountDividendTaxStatus!();
      const rejected = assert.rejects(pending, /销毁|关闭|过期|generation|中断/);
      await host.dispose();
      complete({ owner: "0", value: { kind: "AccountDividendTaxStatus", data: status } });
      await rejected;
    });
  });
  if (kind !== "Remote") test(`${kind} 本地固定玩家零不能接受改写为其他 owner 的完整回执`, { timeout: 10_000 }, async () => {
    await withHost(kind, async (host, _calls, respond) => {
      respond(() => ({ owner: "9", value: { kind: "OwnerFlatWithholdingReceipts", data: [{ ...flat, account: "9" }] } }));
      await assert.rejects(host.queryOwnerFlatWithholdingReceipts!(), /owner/);
    });
  });
}
