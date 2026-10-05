import { invoke } from "@tauri-apps/api/core";
import { createTransportArchiveStore } from "../save/archive-store.ts";
import { parseSetup } from "../save/schema/market.ts";
import { decimal } from "../save/schema/primitives.ts";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { HistoricalStockData, PublicReportAvailability, PublicReportAvailabilityQuery, PublicReportPage, PublicReportQuery, PublicReportSummary, SaveSlot, SessionSetup, StockCode } from "../types/engine.ts";
import type { PausePreferences } from "../types/generated/PausePreferences.ts";
import type { EngineHost, IndicatorCapabilities, IntradayAverageCurveInput, IntradayAverageInput, IntradayAverageResult, PersonalTradeConfirmation } from "./engine-host.ts";
import { createProtocolUpdate, type HostFailure, type HostUpdate, UI_TARGET_HZ } from "./host-update.ts";
import { TauriTimelineState, type TauriBaselineResponse } from "./tauri-timeline-state.ts";
import { normalizePublicReportAvailability, normalizePublicReportAvailabilityQuery, normalizePublicReportById, normalizePublicReportPage } from "./serde-normalize.ts";
import { assertValidSpeedMultiplier, parseSpeedMetrics } from "./speed.ts";
import { normalizePlayerWorkingOrders, type PlayerWorkingOrder } from "./player-working-orders.ts";
import { parseHostFailure } from "./protocol-failure.ts";
import { parseNpcDecisionDiagnostics, type NpcDecisionTraceRecord } from "./npc-decision-trace.ts";
import type { IndicatorInput, IndicatorResults } from "../components/indicator-results.ts";
import { normalizeIndicatorInput, normalizeIndicatorResults } from "./indicator-transport.ts";
import { parseHistoricalStockData } from "./stock-history.ts";
import { parseInitialAllocation } from "./initial-allocation.ts";
import { exact } from "./protocol/guards.ts";
import { parseReportCorrectionResponse, reportCorrectionFailureGeneration } from "./report-corrections.ts";
import { parseCompanyReportCorrection } from "../save/schema/company/report-corrections.ts";
import { normalizeConfirmationCursor, normalizeIntradayAverageCurveInput, normalizeIntradayAverageCurveResult, normalizeIntradayAverageInput, normalizeIntradayAverageResult, normalizePersonalTradeConfirmations } from "./intraday-average.ts";
import { normalizePersonalTradeHistoryRequest, normalizePersonalTradeHistoryPage } from "./personal-trade-history.ts";
import { normalizeMarketHistoryRequest, normalizeMarketHistoryPage, type MarketHistoryRequest, type MarketHistoryPage } from "./market-history.ts";
import { normalizeCurrentMinuteHistoryRequest, normalizeCurrentMinuteHistoryResponse, type CurrentMinuteHistoryRequest, type CurrentMinuteHistoryResponse } from "./current-minute-history.ts";
import type { PersonalTradeHistoryRequest, PersonalTradeHistoryPage } from "./engine-host.ts";

type EngineEventPayload = {
  readonly session_id: string;
  readonly timeline_id: string;
  readonly update: unknown;
};

type EngineFailurePayload = {
  readonly session_id: string;
  readonly timeline_id: string;
  readonly code: string;
  readonly where: string;
  readonly message: string;
  readonly events: readonly unknown[];
  readonly cause?: unknown;
  readonly context?: unknown;
  readonly recoverable?: boolean | null;
  readonly recoveryActions?: readonly string[] | null;
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

function parseHostCapabilities(value: unknown): { readonly npcDecisionDiagnostics: boolean; readonly indicatorCapabilities: IndicatorCapabilities; readonly personalTradeHistory: boolean } {
  const source = record(value, "Tauri host_capabilities");
  const indicators = record(source.indicatorCapabilities, "Tauri host_capabilities.indicatorCapabilities");
  const indicatorKeys = ["intradayAverage", "macd", "priceKdj", "candleKdj"];
  if (Object.keys(source).length !== 3 || typeof source.npcDecisionDiagnostics !== "boolean"
    || Object.keys(indicators).length !== indicatorKeys.length
    || indicatorKeys.some((key) => typeof indicators[key] !== "boolean")
    || typeof source.personalTradeHistory !== "boolean") throw new Error("Tauri 宿主能力响应不符合契约");
  return { npcDecisionDiagnostics: source.npcDecisionDiagnostics, indicatorCapabilities: indicators as unknown as IndicatorCapabilities, personalTradeHistory: source.personalTradeHistory };
}

function parseFailurePayload(value: unknown): EngineFailurePayload {
  const source = record(value, "Tauri engine-failure");
  const required = ["session_id", "timeline_id", "code", "message", "events"];
  const optional = ["where", "cause", "context", "recoverable", "recoveryActions"];
  if (required.some((key) => !Object.hasOwn(source, key)) || Object.keys(source).some((key) => !required.includes(key) && !optional.includes(key))) {
    throw new Error("Tauri engine-failure 字段不符合契约");
  }
  if (!Array.isArray(source.events) || source.events.length !== 0) throw new Error("Tauri engine-failure 不得携带旧版 flat events");
  const details = Object.fromEntries(Object.entries(source).filter(([key]) => ["code", "where", "message", "cause", "context", "recoverable", "recoveryActions"].includes(key)));
  const failure = parseHostFailure(details, "tauri-host.engine-failure");
  return {
    session_id: text(source.session_id, "Tauri engine-failure.session_id"),
    timeline_id: text(source.timeline_id, "Tauri engine-failure.timeline_id"),
    ...failure,
    events: source.events,
  };
}

function parseRestore(value: unknown): TauriBaselineResponse {
  const source = record(value, "Tauri restore_session");
  const keys = Object.keys(source);
  if (keys.length !== 4 || !["snapshot", "timeline_id", "generation", "civil_date"].every((key) => Object.hasOwn(source, key))) {
    throw new Error("Tauri restore_session 响应字段无效");
  }
  return { snapshot: source.snapshot, civil_date: source.civil_date, timeline_id: text(source.timeline_id, "Tauri restore_session.timeline_id"), generation: generation(source.generation, "Tauri restore_session.generation") };
}

export async function createTauriHost(setup: SessionSetup, seed: bigint, options: { resumeArchive?: boolean } = {}): Promise<EngineHost> {
  let sessionId: string | null = null;
  const timeline = new TauriTimelineState();
  let callback: ((update: HostUpdate) => void | boolean) | null = null;
  let fatalCallback: ((failure: HostFailure) => void) | null = null;
  let running = false;
  let disposed = false;
  let eventUnlisten: UnlistenFn | null = null;
  let failureUnlisten: UnlistenFn | null = null;
  let npcDiagnosticsEnabled = false;
  let indicatorCapabilities: IndicatorCapabilities = { intradayAverage: false, macd: false, priceKdj: false, candleKdj: false };
  let personalTradeHistory = false;
  let startupContext: EngineHost["startupContext"];
  let deliveredGeneration: string | null = null;

  const fail = (failure: HostFailure) => {
    if (failure.code === "REPORT_CORRECTION_REJECTED" && failure.recoverable === true && reportCorrectionFailureGeneration(failure) !== timeline.currentGeneration()) return;
    running = false;
    fatalCallback?.(failure);
  };

  try {
    eventUnlisten = await listen<unknown>("engine-event", (event) => {
      try {
        const payload = parseTauriEventPayload(event.payload);
        if (payload.session_id !== sessionId || !timeline.matchesTimeline(payload.timeline_id) || disposed) return;
        if (callback?.(createProtocolUpdate(timeline.currentGeneration(), payload.update)) === false) {
          callback = null;
          running = false;
          void invoke("pause_session", { sessionId: requireSession() }).catch((error: unknown) => {
            fail({ code: "TAURI_PROTOCOL_STOP", where: "tauri-host.engine-event", message: error instanceof Error ? error.message : String(error) });
          });
        }
      } catch (error) {
        fail({ code: "TAURI_EVENT_PROTOCOL", where: "tauri-host.engine-event", message: error instanceof Error ? error.message : String(error) });
      }
    });
    failureUnlisten = await listen<unknown>("engine-failure", (event) => {
      try {
        const payload = parseFailurePayload(event.payload);
        if (payload.session_id !== sessionId || !timeline.matchesTimeline(payload.timeline_id) || disposed) return;
        fail(payload);
      } catch (error) {
        fail({ code: "TAURI_FAILURE_PROTOCOL", where: "tauri-host.engine-failure", message: error instanceof Error ? error.message : String(error) });
      }
    });

    const created = record(await invoke<unknown>("create_session", { setup, seed: seed.toString(), resumeArchive: options.resumeArchive === true }), "Tauri create_session");
    if (Object.keys(created).sort().join() !== "resumed,seed,sessionId,setup" || typeof created.resumed !== "boolean") throw new Error("Tauri create_session 响应结构无效");
    sessionId = text(created.sessionId, "Tauri create_session.sessionId");
    startupContext = { setup: parseSetup(created.setup, "Tauri create_session.setup"), seed: decimal(created.seed, "Tauri create_session.seed"), resumed: created.resumed };
    timeline.setInitialTimeline(sessionId);
    const initialBaseline = parseRestore(await invoke<unknown>("engine_baseline", { sessionId, generation: timeline.currentGeneration() }));
    timeline.installInitialBaseline(initialBaseline);
    const hostCapabilities = parseHostCapabilities(await invoke<unknown>("host_capabilities"));
    npcDiagnosticsEnabled = hostCapabilities.npcDecisionDiagnostics;
    indicatorCapabilities = hostCapabilities.indicatorCapabilities;
    personalTradeHistory = hostCapabilities.personalTradeHistory;
  } catch (error) {
    disposed = true;
    const ownedSessionId = sessionId;
    sessionId = null;
    timeline.clearForDispose();
    const cleanup: { name: string; release: () => void | Promise<unknown> }[] = [];
    if (eventUnlisten !== null) cleanup.push({ name: "engine-event listener", release: eventUnlisten });
    if (failureUnlisten !== null) cleanup.push({ name: "engine-failure listener", release: failureUnlisten });
    if (ownedSessionId !== null) cleanup.push({ name: "stop_session", release: () => invoke("stop_session", { sessionId: ownedSessionId }) });
    const results = await Promise.allSettled(cleanup.map(({ release }) => Promise.resolve().then(release)));
    const cleanupErrors = results.flatMap((result, index) => result.status === "rejected"
      ? [new Error(`${cleanup[index]!.name} 清理失败：${result.reason instanceof Error ? result.reason.message : String(result.reason)}`, { cause: result.reason })]
      : []);
    const message = `Tauri 会话初始化失败：${error instanceof Error ? error.message : String(error)}`;
    if (cleanupErrors.length > 0) throw new AggregateError([error, ...cleanupErrors], `${message}；${cleanupErrors.map((failure) => failure.message).join("；")}`, { cause: error });
    throw new Error(message, { cause: error });
  }

  const requireSession = (): string => {
    if (disposed) throw new Error("Tauri 会话已销毁，操作已取消");
    if (sessionId === null) throw new Error("Tauri 会话尚未就绪");
    return sessionId;
  };

  const assertResponseCurrent = (generation: string, operation: string): void => {
    if (disposed) throw new Error(`Tauri ${operation} 响应属于已销毁会话`);
    timeline.assertGeneration(generation, `Tauri ${operation} 响应属于已过期 generation`);
  };

  const deliverInstalledBaseline = async (baseline: HostUpdate, operation: string): Promise<void> => {
    deliveredGeneration = null;
    if (callback === null) return;
    try {
      if (callback(baseline) === false) throw new Error(`Tauri 消费者拒绝 ${operation} baseline，未恢复 actor`);
    } catch (error) {
      callback = null;
      const wasRunning = running;
      running = false;
      if (wasRunning) {
        await invoke("pause_session", { sessionId: requireSession() });
        assertResponseCurrent(baseline.generation, `${operation}.pause`);
      }
      throw error;
    }
    deliveredGeneration = baseline.generation;
  };

  return {
    startupContext,
    archiveStore: createTransportArchiveStore(async (operation, slotId, name) => {
      const currentGeneration = timeline.captureGeneration();
      const result = await invoke<unknown>(`archive_${operation}`, { sessionId: requireSession(), ...(slotId === undefined ? {} : { slotId }), ...(name === undefined ? {} : { name }), ...(["select", "rename", "copy", "delete"].includes(operation) ? { generation: currentGeneration } : {}) });
      assertResponseCurrent(currentGeneration, `archive.${operation}`);
      return result;
    }),
    capabilities: { persistence: "native", deliveryModes: [], targetUiHz: UI_TARGET_HZ, sharedMemory: false, reconnect: false, publicCompanyReports: true, npcDecisionDiagnostics: npcDiagnosticsEnabled, indicatorCapabilities, personalTradeHistory },
    async start(onUpdate, onFatalError) {
      if (disposed) throw new Error("Tauri 会话已经销毁，不能重新启动");
      callback = onUpdate;
      fatalCallback = onFatalError ?? null;
      const baseline = timeline.baselineForDelivery();
      if (baseline !== null && deliveredGeneration !== baseline.generation) {
        if (callback(baseline) === false) {
          callback = null;
          throw new Error("Tauri 消费者拒绝 cached baseline，未恢复 actor");
        }
        deliveredGeneration = baseline.generation;
      }
      const id = requireSession();
      const generation = timeline.captureGeneration();
      await invoke("resume_session", { sessionId: id });
      if (disposed) throw new Error("Tauri start 响应属于已销毁会话");
      timeline.assertGeneration(generation, "Tauri start 响应属于已过期 generation");
      running = true;
    },
    async stop() {
      const id = requireSession();
      const generation = timeline.captureGeneration();
      await invoke("pause_session", { sessionId: id });
      assertResponseCurrent(generation, "stop");
      running = false;
    },
    async dispose() {
      if (disposed) return;
      disposed = true;
      const id = sessionId;
      sessionId = null;
      callback = null;
      timeline.clearForDispose();
      eventUnlisten?.();
      failureUnlisten?.();
      try {
        if (id !== null) await invoke("stop_session", { sessionId: id });
      } catch (error) {
        fail({ code: "TAURI_DISPOSE", where: "tauri-host.dispose", message: error instanceof Error ? error.message : String(error), context: { sessionId: id } });
        throw error;
      } finally {
        fatalCallback = null;
      }
    },
    async setSpeed(multiplier) {
      assertValidSpeedMultiplier(multiplier);
      const speed = multiplier === Infinity ? "Fastest" : { Fixed: multiplier };
      const generation = timeline.captureGeneration();
      await invoke("set_speed", { sessionId: requireSession(), speed });
      assertResponseCurrent(generation, "setSpeed");
    },
    async setPausePreferences(preferences: PausePreferences) {
      const generation = timeline.captureGeneration();
      await invoke("set_pause_preferences", tauriPausePreferenceArgs(requireSession(), preferences));
      assertResponseCurrent(generation, "setPausePreferences");
    },
    setFrameRate() {},
    async readSpeedMetrics() {
      return parseSpeedMetrics(await invoke<unknown>("speed_metrics", { sessionId: requireSession() }));
    },
    async submitIntent(intent) {
      const generation = timeline.captureGeneration();
      await invoke("enqueue", { sessionId: requireSession(), generation, intent });
      assertResponseCurrent(generation, "submitIntent");
    },
    snapshot() {
      return timeline.baselineForRead().snapshot;
    },
    tick() {
      return timeline.baselineForRead().snapshot.tick;
    },
    day() {
      return timeline.baselineForRead().snapshot.day;
    },
    async civilDate() {
      const response = record(await invoke<unknown>("civil_date", { sessionId: requireSession(), generation: timeline.currentGeneration() }), "Tauri civil_date");
      return text(response.value, "Tauri civil_date.value");
    },
    async save(candidate?: { readonly seq: number; readonly settledDate: string }) {
      const queryGeneration = timeline.captureGeneration();
      const result = await invoke<SaveSlot>("save_session", candidate === undefined
        ? { sessionId: requireSession(), generation: queryGeneration }
        : { sessionId: requireSession(), generation: queryGeneration, candidate });
      if (disposed) throw new Error("Tauri 存档响应属于已过期会话 generation");
      timeline.assertGeneration(queryGeneration, "Tauri 存档响应属于已过期会话 generation");
      return result;
    },
    async playerWorkingOrders(): Promise<readonly PlayerWorkingOrder[]> {
      timeline.baselineForRead("Tauri 基线尚未就绪，不能查询玩家活动委托");
      const cursor = timeline.captureQueryCursor();
      const queryGeneration = cursor.generation;
      const response = record(await invoke<unknown>("player_working_orders", { sessionId: requireSession(), generation: queryGeneration }), "Tauri player_working_orders");
      timeline.assertQueryCursor(cursor, generation(response.generation, "Tauri player_working_orders.generation"), "Tauri 玩家活动委托响应属于已过期会话 generation");
      return normalizePlayerWorkingOrders(response.value);
    },
    async queryStockHistory(code: StockCode): Promise<HistoricalStockData> {
      timeline.baselineForRead("Tauri 基线尚未就绪，不能查询股票历史");
      const cursor = timeline.captureQueryCursor();
      const response = record(await invoke<unknown>("query_stock_history", { sessionId: requireSession(), generation: cursor.generation, code }), "Tauri query_stock_history");
      timeline.assertQueryCursor(cursor, generation(response.generation, "Tauri query_stock_history.generation"), "Tauri 股票历史响应属于已过期会话 generation");
      return parseHistoricalStockData(response.value, code);
    },
    async initialAllocation() {
      timeline.baselineForRead("Tauri 基线尚未就绪，不能查询初始分配");
      const cursor = timeline.captureQueryCursor();
      const response = record(await invoke<unknown>("initial_allocation", { sessionId: requireSession(), generation: cursor.generation }), "Tauri 初始分配响应");
      exact(response, ["generation", "value"], "Tauri 初始分配响应");
      if (disposed) throw new Error("Tauri 初始分配响应属于已销毁会话");
      timeline.assertQueryCursor(cursor, generation(response.generation, "Tauri 初始分配响应.generation"), "Tauri 初始分配响应属于已过期会话 generation");
      return parseInitialAllocation(response.value, setup.stocks.map((stock) => stock.code));
    },
    async refreshBaseline() {
      const queryGeneration = timeline.captureGeneration();
      const restored = parseRestore(await invoke<unknown>("engine_baseline", { sessionId: requireSession(), generation: queryGeneration }));
      assertResponseCurrent(queryGeneration, "refreshBaseline");
      const baseline = timeline.replaceRefreshedBaseline(restored, queryGeneration);
      await deliverInstalledBaseline(baseline, "refresh");
    },
    async load(slot, archiveSlotId, onRestored) {
      const id = requireSession();
      const wasRunning = running;
      const requestedGeneration = timeline.captureGeneration();
      if (wasRunning) {
        await invoke("pause_session", { sessionId: id });
        assertResponseCurrent(requestedGeneration, "load.pause");
        running = false;
      }
      const restored = parseRestore(await invoke<unknown>("restore_session", { sessionId: id, generation: requestedGeneration, slot, ...(archiveSlotId === undefined ? {} : { archiveSlotId }) }));
      assertResponseCurrent(requestedGeneration, "load");
      const baseline = timeline.replaceRestoredBaseline(restored);
      onRestored?.();
      await deliverInstalledBaseline(baseline, "restore");
      if (wasRunning) {
        await invoke("resume_session", { sessionId: id });
        assertResponseCurrent(restored.generation, "load.resume");
        running = true;
      }
    },
    async submitReportCorrection(request) {
      const normalized = parseCompanyReportCorrection(request);
      const cursor = timeline.captureQueryCursor();
      const response = record(await invoke<unknown>("submit_report_correction", { sessionId: requireSession(), generation: cursor.generation, request: normalized }), "财报更正确认");
      if (disposed) throw new Error("财报更正确认属于已销毁市场");
      timeline.assertQueryCursor(cursor, generation(response.generation, "财报更正 generation"), "财报更正确认属于已过期 generation");
      parseReportCorrectionResponse(response, cursor.generation, true);
    },
    async cancelReportCorrection(operationId) {
      if (operationId.trim().length === 0) throw new Error("更正 operation_id 必须非空");
      const cursor = timeline.captureQueryCursor();
      const response = record(await invoke<unknown>("cancel_report_correction", { sessionId: requireSession(), generation: cursor.generation, operationId }), "财报更正取消确认");
      if (disposed) throw new Error("取消确认属于已销毁市场");
      timeline.assertQueryCursor(cursor, generation(response.generation, "取消 generation"), "取消财报更正确认属于已过期 generation");
      parseReportCorrectionResponse(response, cursor.generation, true);
    },
    async queryReportCorrections() {
      const cursor = timeline.captureQueryCursor();
      const response = record(await invoke<unknown>("query_report_corrections", { sessionId: requireSession(), generation: cursor.generation }), "财报更正查询");
      if (disposed) throw new Error("财报更正查询属于已销毁市场");
      timeline.assertQueryCursor(cursor, generation(response.generation, "更正查询 generation"), "财报更正查询属于已过期 generation");
      return parseReportCorrectionResponse(response, cursor.generation, false);
    },
    async queryPublicReports(query: PublicReportQuery): Promise<PublicReportPage> {
      const response = record(await invoke<unknown>("public_reports", { sessionId: requireSession(), generation: timeline.currentGeneration(), query }), "Tauri public_reports");
      return normalizePublicReportPage(response.value);
    },
    async publicReportById(id: string): Promise<PublicReportSummary> {
      const response = record(await invoke<unknown>("public_report_by_id", { sessionId: requireSession(), generation: timeline.currentGeneration(), id }), "Tauri public_report_by_id");
      return normalizePublicReportById(response.value);
    },
    async queryPublicReportAvailability(query: PublicReportAvailabilityQuery): Promise<PublicReportAvailability> {
      const normalizedQuery = normalizePublicReportAvailabilityQuery(query);
      const cursor = timeline.captureQueryCursor();
      const response = record(await invoke<unknown>("public_report_availability", { sessionId: requireSession(), generation: cursor.generation, query: normalizedQuery }), "Tauri public_report_availability");
      if (disposed) throw new Error("公开报告可用性查询属于已销毁市场");
      timeline.assertQueryCursor(cursor, generation(response.generation, "公开报告可用性 generation"), "公开报告可用性查询属于已过期 generation");
      return normalizePublicReportAvailability(response.value, normalizedQuery);
    },
    async npcDecisionTrace(account: string): Promise<readonly NpcDecisionTraceRecord[]> {
      if (!npcDiagnosticsEnabled) throw new Error("Tauri 后端未协商启用 NPC 决策诊断");
      if (typeof account !== "string" || !/^(0|[1-9]\d*)$/.test(account) || account.length > 20 || BigInt(account) > 18_446_744_073_709_551_615n) throw new Error("NPC 账户 ID 必须是规范 u64 非负十进制字符串");
      const cursor = timeline.captureQueryCursor();
      const queryGeneration = cursor.generation;
      const response = record(await invoke<unknown>("npc_decision_diagnostics", { sessionId: requireSession(), generation: queryGeneration, account }), "Tauri npc_decision_diagnostics");
      timeline.assertQueryCursor(cursor, generation(response.generation, "Tauri NPC diagnostics generation"), "Tauri NPC 诊断响应属于已过期会话 generation");
      const diagnostics = parseNpcDecisionDiagnostics(response.value);
      if (diagnostics.kind === "unsupported") throw new Error("Tauri 后端未提供 NPC 决策诊断数据");
      return diagnostics.records;
    },
    async calculateIndicators(input: IndicatorInput): Promise<IndicatorResults> {
      const normalized = normalizeIndicatorInput(input);
      const result = await invoke<unknown>("calculate_indicators", normalized);
      return normalizeIndicatorResults(result, normalized);
    },
    async calculateIntradayAverage(input: IntradayAverageInput): Promise<IntradayAverageResult | null> {
      const normalized = normalizeIntradayAverageInput(input);
      const result = await invoke<unknown>("calculate_intraday_average", { turnoverCents: normalized.turnoverCents, tradeCount: normalized.tradeCount, volumeShares: normalized.volumeShares });
      return normalizeIntradayAverageResult(result);
    },
    async calculateIntradayAverageCurve(input: IntradayAverageCurveInput): Promise<readonly (IntradayAverageResult | null)[]> {
      const normalized = normalizeIntradayAverageCurveInput(input);
      const cursor = timeline.captureQueryCursor();
      const response = record(await invoke<unknown>("calculate_intraday_average_curve", { seriesKey: normalized.seriesKey, samples: normalized.samples.map(({ turnoverCents, tradeCount, volumeShares }) => ({ turnover_cents: turnoverCents, trade_count: tradeCount, volume_shares: volumeShares })) }), "Tauri VWAP curve response");
      timeline.assertQueryCursor(cursor, cursor.generation, "Tauri VWAP curve 响应属于已过期会话 generation");
      exact(response, ["series_key", "results"], "Tauri VWAP curve response");
      if (response.series_key !== normalized.seriesKey) throw new Error("Tauri VWAP curve 响应 series_key 与请求不匹配");
      return normalizeIntradayAverageCurveResult(response.results, normalized.samples.length, normalized.samples);
    },
    async queryPersonalTradeConfirmations(beforeReceipt: string | null = null): Promise<readonly PersonalTradeConfirmation[]> {
      const cursor = timeline.captureQueryCursor();
      const response = record(await invoke<unknown>("personal_trade_confirmations", { sessionId: requireSession(), generation: cursor.generation, beforeReceipt: normalizeConfirmationCursor(beforeReceipt) }), "Tauri personal_trade_confirmations");
      exact(response, ["generation", "confirmations"], "Tauri personal_trade_confirmations");
      timeline.assertQueryCursor(cursor, generation(response.generation, "Tauri confirmations generation"), "Tauri 交割单响应属于已过期会话 generation");
      return normalizePersonalTradeConfirmations(response.confirmations);
    },
    async queryCurrentMinuteHistory(request: CurrentMinuteHistoryRequest): Promise<CurrentMinuteHistoryResponse> {
      const normalized = normalizeCurrentMinuteHistoryRequest(request), cursor = timeline.captureQueryCursor();
      const response = record(await invoke<unknown>("current_minute_history", { sessionId: requireSession(), generation: cursor.generation, query: normalized }), "Tauri当前分钟");
      exact(response, ["generation", "response"], "Tauri当前分钟");
      timeline.assertQueryCursor(cursor, generation(response.generation, "Tauri当前分钟generation"), "Tauri当前分钟属于过期generation");
      return normalizeCurrentMinuteHistoryResponse(response.response, normalized);
    },
    async queryMarketHistory(request: MarketHistoryRequest): Promise<MarketHistoryPage> {
      const normalized = normalizeMarketHistoryRequest(request);
      const cursor = timeline.captureQueryCursor();
      const response = record(await invoke<unknown>("market_history", { sessionId: requireSession(), generation: cursor.generation, query: normalized }), "Tauri永久量价历史");
      exact(response, ["generation", "page"], "Tauri永久量价历史");
      timeline.assertQueryCursor(cursor, generation(response.generation, "Tauri量价历史generation"), "Tauri量价历史属于过期generation");
      return normalizeMarketHistoryPage(response.page, normalized);
    },
    async queryPersonalTradeHistory(request: PersonalTradeHistoryRequest): Promise<PersonalTradeHistoryPage> {
      const normalized = normalizePersonalTradeHistoryRequest(request);
      const cursor = timeline.captureQueryCursor();
      const response = record(await invoke<unknown>("personal_trade_history", { sessionId: requireSession(), generation: cursor.generation, query: normalized }), "Tauri本人日期历史");
      exact(response, ["generation", "page"], "Tauri本人日期历史");
      timeline.assertQueryCursor(cursor, generation(response.generation, "Tauri本人日期历史generation"), "Tauri日期交割历史属于过期generation");
      return normalizePersonalTradeHistoryPage(response.page, normalized);
    },
  };
}
