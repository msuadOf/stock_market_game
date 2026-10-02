import assert from "node:assert/strict";
import test from "node:test";
import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { createTauriHost } from "./tauri-host.ts";
import type { SessionSetup } from "../types/engine.ts";

test("Tauri save pins generation for keyed and latest candidates and rejects a late response after load", async () => {
  const originalWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
  Object.defineProperty(globalThis, "window", { configurable: true, value: { crypto: globalThis.crypto } });
  const requests: Record<string, unknown>[] = [];
  let completeSave: ((slot: unknown) => void) | null = null;
  const snapshot = { seq: 42, tick: 0, day: 0, phase: "Continuous", markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} };
  mockIPC((command, args) => {
    if (command === "create_session") return "session-1";
    if (command === "host_capabilities") return { npcDecisionDiagnostics: false };
    if (command === "engine_baseline") return { snapshot, generation: "1", timeline_id: "timeline-1" };
    if (command === "restore_session") return { snapshot, generation: "2", timeline_id: "timeline-2" };
    if (command === "save_session") {
      requests.push(args as Record<string, unknown>);
      return new Promise((resolve) => { completeSave = resolve; });
    }
    return null;
  }, { shouldMockEvents: true });
  try {
    const host = await createTauriHost({} as SessionSetup, 1n);
    const candidate = { seq: 42, settledDate: "2030-01-05" };
    const keyed = host.save(candidate);
    assert.deepEqual(requests[0], { sessionId: "session-1", generation: "1", candidate });
    completeSave!({ saved: true });
    await keyed;
    const latest = host.save();
    assert.deepEqual(requests[1], { sessionId: "session-1", generation: "1" });
    await host.load({});
    completeSave!({ old: true });
    await assert.rejects(latest, /generation/);
    host.dispose();
  } finally {
    clearMocks();
    if (originalWindow === undefined) delete (globalThis as { window?: unknown }).window;
    else Object.defineProperty(globalThis, "window", originalWindow);
  }
});
