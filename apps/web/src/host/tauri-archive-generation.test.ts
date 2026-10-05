import assert from "node:assert/strict";
import test from "node:test";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { createTauriHost } from "./tauri-host.ts";
import { DEFAULT_SETUP } from "../config/defaults.ts";

test("Tauri槽位修改必须携带当前generation，换代后拒绝旧响应", { timeout: 10000 }, async () => {
  const original = Object.getOwnPropertyDescriptor(globalThis, "window");
  Object.defineProperty(globalThis, "window", { configurable: true, value: { crypto: globalThis.crypto } });
  const requests: { command: string; args: Record<string, unknown> }[] = [];
  const snapshot = { seq: 0, tick: 0, day: 0, phase: "Continuous", markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} };
  let pending: ((value: unknown) => void) | undefined;
  let delayed = false;
  mockIPC((command, args) => {
    if (command === "create_session") return { sessionId: "archive-session", setup: DEFAULT_SETUP, seed: "1", resumed: false };
    if (command === "engine_baseline") return { snapshot, generation: "1", timeline_id: "archive-timeline" };
    if (command === "restore_session") return { snapshot, generation: "2", timeline_id: "archive-next" };
    if (command === "host_capabilities") return { npcDecisionDiagnostics: false, indicatorCapabilities: { intradayAverage: true, macd: true, priceKdj: true, candleKdj: true }, personalTradeHistory: true };
    if (command.startsWith("archive_")) {
      requests.push({ command, args: args as Record<string, unknown> });
      if (delayed) return new Promise((resolve) => { pending = resolve; });
      if (command === "archive_select") return true;
      if (command === "archive_copy") return { slot_id: "copied", name: "副本", civil_date: "2030-01-11", tick: 0 };
    }
    return null;
  }, { shouldMockEvents: true });
  try {
    const host = await createTauriHost(DEFAULT_SETUP, 1n);
    assert.ok(host.archiveStore);
    await host.archiveStore.rename("chosen", "新名");
    await host.archiveStore.copy("chosen", "副本");
    await host.archiveStore.delete("copied");
    await host.archiveStore.select("chosen");
    for (const request of requests) {
      assert.equal(request.args.sessionId, "archive-session");
      assert.equal(request.args.generation, "1", request.command);
    }
    delayed = true;
    const late = host.archiveStore.rename("chosen", "旧代");
    await host.load({});
    pending!(null);
    await assert.rejects(late, /过期|generation/);
    delayed = false;
    await host.archiveStore.rename("chosen", "新代");
    assert.equal(requests.at(-1)!.args.generation, "2");
    await host.dispose();
  } finally {
    clearMocks();
    if (original === undefined) delete (globalThis as { window?: unknown }).window;
    else Object.defineProperty(globalThis, "window", original);
  }
});
