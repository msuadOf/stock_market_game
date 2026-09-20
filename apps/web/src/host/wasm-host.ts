import init, * as wasm from "../../wasm-pkg/web_wasm.js";
import type {
  PublicReportPage,
  PublicReportQuery,
  PublicReportSummary,
  SessionSetup,
  Snapshot,
} from "../types/engine.ts";
import type { PausePreferences } from "../types/generated/PausePreferences.ts";
import { parseSaveSlot } from "../save/save-schema.ts";
import type { EngineHost } from "./engine-host.ts";
import { createBaselineUpdate, createProtocolUpdate, UI_TARGET_HZ } from "./host-update.ts";
import { parseEngineUpdate, parseProtocolSnapshot } from "./protocol/index.ts";
import { normalizePublicReportById, normalizePublicReportPage, normalizeSerdeMaps } from "./serde-normalize.ts";
import { HostSpeedMeter, assertValidSpeedMultiplier } from "./speed.ts";
import { restoreWasmSession } from "./wasm-restore-transaction.ts";

const BASE_INTERVAL_MS = 1_000;
const FASTEST_SLICE_MS = 8;
const FASTEST_SLICE_MAX_STEPS = 100_000;

let wasmReady: Promise<void> | null = null;

export function ensureWasmReady(): Promise<void> {
  if (wasmReady !== null) return wasmReady;
  wasmReady = (async () => {
    const response = await fetch(new URL("../../wasm-pkg/web_wasm_bg.wasm", import.meta.url));
    if (!response.ok) {
      throw new Error(`加载 wasm 二进制失败：HTTP ${response.status} ${response.statusText}（路径 web_wasm_bg.wasm）`);
    }
    await init(new Uint8Array(await response.arrayBuffer()));
  })();
  return wasmReady;
}

function snapshot(handle: number): Snapshot {
  return parseProtocolSnapshot(wasm.snapshot(handle), "WASM snapshot");
}

function failure(where: string, error: unknown) {
  return {
    code: "WASM_PROTOCOL",
    where,
    message: error instanceof Error ? error.message : String(error),
  };
}

export function createWasmHost(setup: SessionSetup, seed: bigint): EngineHost {
  let handle: number | null = null;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let speed = 1;
  let generation = 1;
  let preferences: PausePreferences = { pause_after_close: false, pause_before_open: false };
  let onUpdate: ((update: import("./host-update.ts").HostUpdate) => void) | null = null;
  let onFatal: ((error: ReturnType<typeof failure>) => void) | null = null;
  let baselineDelivered = false;
  const speedMeter = new HostSpeedMeter(() => performance.now());

  const interval = () => {
    if (!Number.isFinite(speed)) throw new Error("最快模式不使用固定定时器");
    return Math.max(1, Math.round(BASE_INTERVAL_MS / speed));
  };

  const publish = (rawUpdate: unknown) => {
    onUpdate?.(createProtocolUpdate(String(generation), rawUpdate));
    const parsed = parseEngineUpdate(rawUpdate);
    if ("CivilUpdate" in parsed && (
      (preferences.pause_after_close && parsed.CivilUpdate.kinds.includes("AfterClose"))
      || (preferences.pause_before_open && parsed.CivilUpdate.kinds.includes("BeforeOpen"))
    )) {
      stopTimer();
      speedMeter.setRunning(false);
    }
  };

  const stepOnce = (): boolean => {
    if (handle === null) return false;
    try {
      try {
        publish(wasm.step(handle));
      } catch (error) {
        if (!String(error).includes("civil day barrier must be published before stepping")) throw error;
        publish(wasm.end_civil_day(handle));
      }
      speedMeter.recordTicks();
      return true;
    } catch (error) {
      stopTimer();
      onFatal?.(failure("wasm-host.step", error));
      return false;
    }
  };

  const runFastestSlice = () => {
    if (timer === null || speed !== Infinity) return;
    const startedAt = performance.now();
    let steps = 0;
    while (steps < FASTEST_SLICE_MAX_STEPS && performance.now() - startedAt < FASTEST_SLICE_MS) {
      if (!stepOnce()) return;
      steps += 1;
    }
    timer = setTimeout(runFastestSlice, 0);
  };

  const startTimer = () => {
    if (timer !== null) return;
    if (speed === Infinity) {
      timer = setTimeout(runFastestSlice, 0);
      return;
    }
    timer = setInterval(() => { stepOnce(); }, interval());
  };

  const stopTimer = () => {
    if (timer !== null) {
      clearTimeout(timer);
      timer = null;
    }
  };

  return {
    capabilities: {
      deliveryModes: [],
      targetUiHz: UI_TARGET_HZ,
      sharedMemory: false,
      reconnect: false,
      publicCompanyReports: true,
      npcDecisionDiagnostics: import.meta.env.DEV,
    },
    start(callback, fatalCallback) {
      onUpdate = callback;
      onFatal = fatalCallback ?? null;
      if (handle === null) handle = wasm.create_session(setup, seed);
      if (!baselineDelivered) {
        onUpdate(createBaselineUpdate(String(generation), snapshot(handle)));
        baselineDelivered = true;
      }
      speedMeter.setRunning(true);
      startTimer();
    },
    stop() {
      stopTimer();
      speedMeter.setRunning(false);
    },
    dispose() {
      stopTimer();
      if (handle !== null) wasm.drop_session(handle);
      handle = null;
      onUpdate = null;
      onFatal = null;
      speedMeter.setRunning(false);
    },
    setSpeed(multiplier) {
      assertValidSpeedMultiplier(multiplier);
      speed = multiplier;
      speedMeter.setSpeed(multiplier);
      if (timer !== null) {
        stopTimer();
        startTimer();
      }
    },
    async setPausePreferences(next) {
      preferences = { ...next };
      if (preferences.pause_after_close || preferences.pause_before_open) return;
    },
    setFrameRate() {},
    async readSpeedMetrics() {
      return speedMeter.read();
    },
    async submitIntent(intent) {
      if (handle === null) throw new Error("会话尚未创建，无法提交意图（请先 start）");
      wasm.enqueue(handle, intent);
    },
    snapshot() {
      if (handle === null) throw new Error("会话尚未创建，无法读取快照");
      return snapshot(handle);
    },
    tick() {
      if (handle === null) throw new Error("会话尚未创建，无法读取 tick");
      return Number(wasm.tick(handle));
    },
    day() {
      if (handle === null) throw new Error("会话尚未创建，无法读取交易日");
      return wasm.day(handle);
    },
    async civilDate() {
      if (handle === null) throw new Error("会话尚未创建，无法读取自然日");
      return wasm.civil_date(handle);
    },
    async endCivilDay() {
      if (handle === null) throw new Error("会话尚未创建，无法推进自然日");
      publish(wasm.end_civil_day(handle));
    },
    async save() {
      if (handle === null) throw new Error("会话尚未创建，无法保存");
      return parseSaveSlot(normalizeSerdeMaps(wasm.save(handle)));
    },
    async load(slot) {
      const parsed = parseSaveSlot(slot);
      const wasRunning = timer !== null;
      const restored = restoreWasmSession({
        currentHandle: () => handle,
        replaceHandle: (restoredHandle) => { handle = restoredHandle; },
        restore: () => wasm.restore_json(JSON.stringify(parsed)),
        snapshot,
        drop: wasm.drop_session,
        wasRunning,
        stop: stopTimer,
        restart: startTimer,
      });
      generation += 1;
      onUpdate?.(createBaselineUpdate(String(generation), restored));
    },
    async queryPublicReports(query: PublicReportQuery): Promise<PublicReportPage> {
      if (handle === null) throw new Error("会话尚未创建，无法查询公开报告");
      return normalizePublicReportPage(wasm.public_report_page(handle, query));
    },
    async publicReportById(id: string): Promise<PublicReportSummary> {
      if (handle === null) throw new Error("会话尚未创建，无法查询公开报告");
      return normalizePublicReportById(wasm.public_report_by_id(handle, id));
    },
  };
}
