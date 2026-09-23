import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { PublicReportPage, PublicReportQuery, PublicReportSummary, SaveSlot, SessionSetup } from "../types/engine.ts";
import type { PausePreferences } from "../types/generated/PausePreferences.ts";
import type { EngineHost } from "./engine-host.ts";
import { createBaselineUpdate, createProtocolUpdate, type HostFailure, type HostUpdate, UI_TARGET_HZ } from "./host-update.ts";
import { parseProtocolSnapshot } from "./protocol/index.ts";
import { normalizePublicReportById, normalizePublicReportPage } from "./serde-normalize.ts";
import { assertValidSpeedMultiplier, parseSpeedMetrics } from "./speed.ts";

type EngineEventPayload = {
  readonly session_id: string;
  readonly timeline_id: string;
  readonly update: unknown;
};

type EngineFailurePayload = {
  readonly session_id: string;
  readonly timeline_id: string;
  readonly code: string;
  readonly message: string;
  readonly events: readonly unknown[];
};

type RestoreResponse = {
  readonly snapshot: unknown;
  readonly timeline_id: string;
  readonly generation: string;
};

function record(value: unknown, where: string): Readonly<Record<string, unknown>> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Error(`${where} 必须是对象`);
  return value as Readonly<Record<string, unknown>>;
}

function text(value: unknown, where: string): string {
  if (typeof value !== "string" || value.length === 0) throw new Error(`${where} 必须是非空字符串`);
  return value;
}

function generation(value: unknown, where: string): string {
  const parsed = text(value, where);
  if (!/^(0|[1-9]\d*)$/.test(parsed)) throw new Error(`${where} 必须是规范非负十进制整数`);
  return parsed;
}

export function parseTauriEventPayload(value: unknown): EngineEventPayload {
  const source = record(value, "Tauri engine-event");
  const keys = Object.keys(source);
  if (keys.length !== 3 || !["session_id", "timeline_id", "update"].every((key) => Object.hasOwn(source, key))) {
    throw new Error("Tauri engine-event 字段不符合完整协议更新契约");
  }
  return { session_id: text(source.session_id, "Tauri engine-event.session_id"), timeline_id: text(source.timeline_id, "Tauri engine-event.timeline_id"), update: source.update };
}

export function tauriPausePreferenceArgs(sessionId: string, preferences: PausePreferences): { readonly sessionId: string; readonly preferences: PausePreferences } {
  return { sessionId, preferences };
}

function parseFailurePayload(value: unknown): EngineFailurePayload {
  const source = record(value, "Tauri engine-failure");
  const keys = Object.keys(source);
  if (keys.length !== 5 || !["session_id", "timeline_id", "code", "message", "events"].every((key) => Object.hasOwn(source, key))) {
    throw new Error("Tauri engine-failure 字段不符合契约");
  }
  if (!Array.isArray(source.events) || source.events.length !== 0) throw new Error("Tauri engine-failure 不得携带旧版 flat events");
  return {
    session_id: text(source.session_id, "Tauri engine-failure.session_id"),
    timeline_id: text(source.timeline_id, "Tauri engine-failure.timeline_id"),
    code: text(source.code, "Tauri engine-failure.code"),
    message: text(source.message, "Tauri engine-failure.message"),
    events: source.events,
  };
}

function parseRestore(value: unknown): RestoreResponse {
  const source = record(value, "Tauri restore_session");
  const keys = Object.keys(source);
  if (keys.length !== 3 || !["snapshot", "timeline_id", "generation"].every((key) => Object.hasOwn(source, key))) {
    throw new Error("Tauri restore_session 响应字段无效");
  }
  return { snapshot: source.snapshot, timeline_id: text(source.timeline_id, "Tauri restore_session.timeline_id"), generation: generation(source.generation, "Tauri restore_session.generation") };
}

function nextGeneration(value: string): string {
  return (BigInt(value) + 1n).toString();
}

export async function createTauriHost(setup: SessionSetup, seed: bigint): Promise<EngineHost> {
  let sessionId: string | null = null;
  let timelineId: string | null = null;
  let currentGeneration = "1";
  let cachedBaseline: Extract<HostUpdate, { type: "baseline" }> | null = null;
  let callback: ((update: HostUpdate) => void) | null = null;
  let fatalCallback: ((failure: HostFailure) => void) | null = null;
  let running = false;
  let disposed = false;
  let eventUnlisten: UnlistenFn | null = null;
  let failureUnlisten: UnlistenFn | null = null;

  const fail = (failure: HostFailure) => {
    running = false;
    fatalCallback?.(failure);
  };

  eventUnlisten = await listen<unknown>("engine-event", (event) => {
    try {
      const payload = parseTauriEventPayload(event.payload);
      if (payload.session_id !== sessionId || payload.timeline_id !== timelineId || disposed) return;
      callback?.(createProtocolUpdate(currentGeneration, payload.update));
    } catch (error) {
      fail({ code: "TAURI_EVENT_PROTOCOL", where: "tauri-host.engine-event", message: error instanceof Error ? error.message : String(error) });
    }
  });
  failureUnlisten = await listen<unknown>("engine-failure", (event) => {
    try {
      const payload = parseFailurePayload(event.payload);
      if (payload.session_id !== sessionId || payload.timeline_id !== timelineId || disposed) return;
      fail({ code: payload.code, where: "tauri-host.engine-failure", message: payload.message });
    } catch (error) {
      fail({ code: "TAURI_FAILURE_PROTOCOL", where: "tauri-host.engine-failure", message: error instanceof Error ? error.message : String(error) });
    }
  });

  try {
    sessionId = await invoke<string>("create_session", { setup, seed: seed.toString() });
    timelineId = sessionId;
    const rawSnapshot = await invoke<unknown>("snapshot", { sessionId });
    cachedBaseline = createBaselineUpdate(currentGeneration, parseProtocolSnapshot(rawSnapshot, "Tauri snapshot"));
  } catch (error) {
    await eventUnlisten();
    await failureUnlisten();
    throw new Error(`Tauri 会话初始化失败：${error instanceof Error ? error.message : String(error)}`);
  }

  const requireSession = (): string => {
    if (sessionId === null) throw new Error("Tauri 会话尚未就绪");
    return sessionId;
  };

  return {
    capabilities: { deliveryModes: [], targetUiHz: UI_TARGET_HZ, sharedMemory: false, reconnect: false, publicCompanyReports: true, npcDecisionDiagnostics: false },
    start(onUpdate, onFatalError) {
      if (disposed) throw new Error("Tauri 会话已经销毁，不能重新启动");
      callback = onUpdate;
      fatalCallback = onFatalError ?? null;
      if (cachedBaseline !== null) callback(cachedBaseline);
      const id = requireSession();
      void invoke("resume_session", { sessionId: id }).then(() => { running = true; }, (error: unknown) => fail({ code: "TAURI_RESUME", where: "tauri-host.start", message: error instanceof Error ? error.message : String(error) }));
    },
    stop() {
      const id = requireSession();
      running = false;
      void invoke("pause_session", { sessionId: id }).catch((error: unknown) => fail({ code: "TAURI_PAUSE", where: "tauri-host.stop", message: error instanceof Error ? error.message : String(error) }));
    },
    dispose() {
      if (disposed) return;
      disposed = true;
      const id = sessionId;
      sessionId = null;
      timelineId = null;
      callback = null;
      fatalCallback = null;
      cachedBaseline = null;
      void eventUnlisten?.();
      void failureUnlisten?.();
      if (id !== null) void invoke("stop_session", { sessionId: id });
    },
    setSpeed(multiplier) {
      assertValidSpeedMultiplier(multiplier);
      const speed = multiplier === Infinity ? "Fastest" : { Fixed: multiplier };
      void invoke("set_speed", { sessionId: requireSession(), speed }).catch((error: unknown) => fail({ code: "TAURI_SPEED", where: "tauri-host.setSpeed", message: error instanceof Error ? error.message : String(error) }));
    },
    async setPausePreferences(preferences: PausePreferences) {
      await invoke("set_pause_preferences", tauriPausePreferenceArgs(requireSession(), preferences));
    },
    setFrameRate() {},
    async readSpeedMetrics() {
      return parseSpeedMetrics(await invoke<unknown>("speed_metrics", { sessionId: requireSession() }));
    },
    async submitIntent(intent) {
      await invoke("enqueue", { sessionId: requireSession(), intent });
    },
    snapshot() {
      if (cachedBaseline === null) throw new Error("Tauri 基线尚未就绪");
      return cachedBaseline.snapshot;
    },
    tick() {
      if (cachedBaseline === null) throw new Error("Tauri 基线尚未就绪");
      return cachedBaseline.snapshot.tick;
    },
    day() {
      if (cachedBaseline === null) throw new Error("Tauri 基线尚未就绪");
      return cachedBaseline.snapshot.day;
    },
    async civilDate() {
      const response = record(await invoke<unknown>("civil_date", { sessionId: requireSession(), generation: currentGeneration }), "Tauri civil_date");
      return text(response.value, "Tauri civil_date.value");
    },
    async save() {
      return await invoke<SaveSlot>("save_session", { sessionId: requireSession() });
    },
    async load(slot) {
      const id = requireSession();
      const wasRunning = running;
      if (wasRunning) await invoke("pause_session", { sessionId: id });
      const restored = parseRestore(await invoke<unknown>("restore_session", { sessionId: id, generation: currentGeneration, slot }));
      if (restored.generation !== nextGeneration(currentGeneration)) throw new Error("Tauri 恢复响应没有递增 generation");
      currentGeneration = restored.generation;
      timelineId = restored.timeline_id;
      cachedBaseline = createBaselineUpdate(currentGeneration, parseProtocolSnapshot(restored.snapshot, "Tauri restore snapshot"));
      callback?.(cachedBaseline);
      if (wasRunning) {
        await invoke("resume_session", { sessionId: id });
        running = true;
      }
    },
    async queryPublicReports(query: PublicReportQuery): Promise<PublicReportPage> {
      const response = record(await invoke<unknown>("public_reports", { sessionId: requireSession(), generation: currentGeneration, query }), "Tauri public_reports");
      return normalizePublicReportPage(response.value);
    },
    async publicReportById(id: string): Promise<PublicReportSummary> {
      const response = record(await invoke<unknown>("public_report_by_id", { sessionId: requireSession(), generation: currentGeneration, id }), "Tauri public_report_by_id");
      return normalizePublicReportById(response.value);
    },
  };
}
