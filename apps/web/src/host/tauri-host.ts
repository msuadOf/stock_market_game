import { parseHostFailure } from "./protocol-failure.ts";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { HistoricalStockData, PublicReportPage, PublicReportQuery, PublicReportSummary, SaveSlot, SessionSetup, StockCode } from "../types/engine.ts";
import type { PausePreferences } from "../types/generated/PausePreferences.ts";
import type { EngineHost } from "./engine-host.ts";
import { createCompanyReadQueries } from "./company-read.ts";
import { CompanyWriteRejectedError, createCompanyWriteCommands, parseCompanyWriteFailure } from "./company-write.ts";
import { describeWasmFailure } from "./wasm-failure.ts";
import { createProtocolUpdate, type HostFailure, type HostUpdate, UI_TARGET_HZ } from "./host-update.ts";
import { TauriTimelineState, type TauriBaselineResponse } from "./tauri-timeline-state.ts";
import { normalizePublicReportById, normalizePublicReportPage } from "./serde-normalize.ts";
import { assertValidSpeedMultiplier, parseSpeedMetrics } from "./speed.ts";
import { normalizePlayerWorkingOrders, type PlayerWorkingOrder } from "./player-working-orders.ts";
import { parseNpcDecisionDiagnostics, type NpcDecisionTraceRecord } from "./npc-decision-trace.ts";
import type { IndicatorInput, IndicatorResults } from "../components/indicator-results.ts";
import { normalizeIndicatorInput, normalizeIndicatorResults } from "./indicator-transport.ts";
import { parseHistoricalStockData } from "./stock-history.ts";
import { parseInitialAllocation } from "./initial-allocation.ts";
import { exact } from "./protocol/guards.ts";
import { wireIntentOf } from "./protocol/stock-identity.ts";

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

function parseHostCapabilities(value: unknown): boolean {
  const source = record(value, "Tauri host_capabilities");
  if (Object.keys(source).length !== 1 || typeof source.npcDecisionDiagnostics !== "boolean") throw new Error("Tauri 宿主能力响应不符合契约");
  return source.npcDecisionDiagnostics;
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
  if (keys.length !== 6 || !["snapshot", "timeline_id", "generation", "civil_date", "stock_catalog", "company_catalog"].every((key) => Object.hasOwn(source, key))) {
    throw new Error("Tauri restore_session 响应字段无效");
  }
  return { snapshot: source.snapshot, stock_catalog: source.stock_catalog, company_catalog: source.company_catalog, civil_date: source.civil_date, timeline_id: text(source.timeline_id, "Tauri restore_session.timeline_id"), generation: generation(source.generation, "Tauri restore_session.generation") };
}

export async function createTauriHost(setup: SessionSetup, seed: bigint): Promise<EngineHost> {
  let sessionId: string | null = null;
  const timeline = new TauriTimelineState();
  let callback: ((update: HostUpdate) => void | boolean) | null = null;
  let fatalCallback: ((failure: HostFailure) => void) | null = null;
  let baselineUnavailable = false;
  let running = false;
  let disposed = false;
  let eventUnlisten: UnlistenFn | null = null;
  let failureUnlisten: UnlistenFn | null = null;
  let npcDiagnosticsEnabled = false;
  let deliveredGeneration: string | null = null;
  let companyWriteFailure: HostFailure | null = null;

  const fail = (failure: HostFailure) => {
    companyWriteFailure = failure;
    running = false;
    if (failure.code === "CATALOG_BASELINE_FAILED") baselineUnavailable = true;
    fatalCallback?.(failure);
  };

  const invokeBaseline = async (command: string, args: Record<string, unknown>): Promise<unknown> => {
    try { return await invoke<unknown>(command, args); }
    catch (error) {
      if (error !== null && typeof error === "object" && (error as Record<string, unknown>).code === "CATALOG_BASELINE_FAILED") {
        // 已声明目录不变量失败，错误载体损坏也不能恢复旧 authority 的提交权限。
        baselineUnavailable = true;
        running = false;
        let failure: HostFailure;
        try { failure = parseHostFailure(error); }
        catch (protocolError) {
          fail({ code: "TAURI_BASELINE_PROTOCOL", where: "tauri-host.invokeBaseline", message: protocolError instanceof Error ? protocolError.message : String(protocolError), cause: protocolError });
          throw protocolError;
        }
        fail(failure);
        throw new Error(failure.message, { cause: failure });
      }
      throw error;
    }
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

    sessionId = text(await invoke<unknown>("create_session", { setup, seed: seed.toString() }), "Tauri create_session.session_id");
    timeline.setInitialTimeline(sessionId);
    const initialBaseline = parseRestore(await invokeBaseline("engine_baseline", { sessionId, generation: timeline.currentGeneration() }));
    timeline.installInitialBaseline(initialBaseline);
    npcDiagnosticsEnabled = parseHostCapabilities(await invoke<unknown>("host_capabilities"));
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

  const assertBaselineAvailable = (): void => {
    if (baselineUnavailable) throw new Error("Tauri 权威 baseline 已失效，须重新同步或重开会话");
  };

  const requireSession = (): string => {
    if (disposed) throw new Error("Tauri 会话已销毁，操作已取消");
    assertBaselineAvailable();
    if (sessionId === null) throw new Error("Tauri 会话尚未就绪");
    return sessionId;
  };

  const assertResponseCurrent = (generation: string, operation: string): void => {
    if (disposed) throw new Error(`Tauri ${operation} 响应属于已销毁会话`);
    assertBaselineAvailable();
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
    ...createCompanyWriteCommands(async command => {
      if (companyWriteFailure !== null) throw new Error("Tauri 公司写命令的当前 authority 已失效，须正常恢复新会话后再核对", { cause: companyWriteFailure });
      const id = requireSession();
      const cursor = timeline.captureQueryCursor();
      const assertCurrent = (responseGeneration = cursor.generation) => {
        if (companyWriteFailure !== null) throw new Error("Tauri 公司写命令确认属于已失效 authority；结果未知，请核对权威状态", { cause: companyWriteFailure });
        assertResponseCurrent(cursor.generation, "company_write");
        timeline.assertQueryCursor(cursor, responseGeneration, "Tauri 公司写命令确认属于已过期 baseline/generation；结果未知，请核对权威状态");
      };
      const [name, payload] = command.kind === "ProposePlayerCompanyAction"
        ? ["propose_player_company_action", { proposal: command.proposal }] as const
        : command.kind === "EnqueueReportCorrection"
          ? ["enqueue_report_correction", { request: command.request }] as const
          : ["cancel_report_correction", { operationId: command.operation_id }] as const;
      let raw: unknown;
      try { raw = await invoke<unknown>(name, { sessionId: id, generation: cursor.generation, ...payload }); }
      catch (original) {
        let rejection: Error;
        const failureKind = original !== null && typeof original === "object" && Object.hasOwn(original, "kind") ? (original as Record<string, unknown>).kind : undefined;
        if (failureKind === "PlayerProposal" || failureKind === "ReportCorrection") {
          try { rejection = new CompanyWriteRejectedError(parseCompanyWriteFailure(original)); }
          catch (decoderError) { rejection = new AggregateError([decoderError], "Tauri 公司写命令 typed 拒绝不符合协议", { cause: original }); }
        } else rejection = original instanceof Error ? original : new Error(`Tauri 公司写命令失败：${describeWasmFailure(original)}`, { cause: original });
        try { assertCurrent(); }
        catch (stale) { throw new Error(stale instanceof Error ? stale.message : "Tauri 公司写命令确认已过期", { cause: rejection }); }
        throw rejection;
      }
      assertCurrent();
      const response = record(raw, `Tauri ${name}`);
      exact(response, command.kind === "ProposePlayerCompanyAction" ? ["generation", "receipt"] : ["generation", "operation_id"], `Tauri ${name}`);
      assertCurrent(generation(response.generation, `Tauri ${name}.generation`));
      return command.kind === "ProposePlayerCompanyAction" ? { receipt: response.receipt } : { operation_id: response.operation_id };
    }, () => timeline.baselineForRead().catalogResolver, "0"),
    ...createCompanyReadQueries(async query => {
      assertBaselineAvailable();
      const cursor = timeline.captureQueryCursor();
      let raw: unknown;
      try { raw = await invoke<unknown>("company_read_query", { sessionId: requireSession(), generation: cursor.generation, query }); }
      catch (error) { throw new Error(`Tauri 公司查询被拒绝：${describeWasmFailure(error)}`, { cause: error }); }
      const response = record(raw, "Tauri company_read_query");
      exact(response, ["generation", "owner", "value"], "Tauri company_read_query");
      assertResponseCurrent(cursor.generation, "company_read_query");
      timeline.assertQueryCursor(cursor, generation(response.generation, "Tauri company_read_query.generation"), "Tauri 公司查询响应属于已过期会话 generation");
      return { owner: response.owner, value: response.value };
    }, () => timeline.baselineForRead().catalogResolver, "0"),
    capabilities: { deliveryModes: [], targetUiHz: UI_TARGET_HZ, sharedMemory: false, reconnect: false, publicCompanyReports: true, npcDecisionDiagnostics: npcDiagnosticsEnabled },
    async start(onUpdate, onFatalError) {
      if (disposed) throw new Error("Tauri 会话已经销毁，不能重新启动");
      const id = requireSession();
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
      const generation = timeline.captureGeneration();
      await invoke("resume_session", { sessionId: id });
      assertResponseCurrent(generation, "start");
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
      const response = await invoke<unknown>("speed_metrics", { sessionId: requireSession() });
      assertBaselineAvailable();
      return parseSpeedMetrics(response);
    },
    async submitIntent(intent) {
      const baseline = timeline.baselineForRead();
      const generation = baseline.generation;
      const wiredIntent = wireIntentOf(intent, baseline.catalogResolver);
      await invoke("enqueue", { sessionId: requireSession(), generation, intent: wiredIntent });
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
      assertBaselineAvailable();
      return text(response.value, "Tauri civil_date.value");
    },
    async save(candidate?: { readonly seq: number; readonly settledDate: string }) {
      const queryGeneration = timeline.captureGeneration();
      const result = await invoke<SaveSlot>("save_session", candidate === undefined
        ? { sessionId: requireSession(), generation: queryGeneration }
        : { sessionId: requireSession(), generation: queryGeneration, candidate });
      if (disposed) throw new Error("Tauri 存档响应属于已过期会话 generation");
      assertBaselineAvailable();
      timeline.assertGeneration(queryGeneration, "Tauri 存档响应属于已过期会话 generation");
      return result;
    },
    async playerWorkingOrders(): Promise<readonly PlayerWorkingOrder[]> {
      timeline.baselineForRead("Tauri 基线尚未就绪，不能查询玩家活动委托");
      const cursor = timeline.captureQueryCursor();
      const queryGeneration = cursor.generation;
      const response = record(await invoke<unknown>("player_working_orders", { sessionId: requireSession(), generation: queryGeneration }), "Tauri player_working_orders");
      assertResponseCurrent(cursor.generation, "query");
      timeline.assertQueryCursor(cursor, generation(response.generation, "Tauri player_working_orders.generation"), "Tauri 玩家活动委托响应属于已过期会话 generation");
      return normalizePlayerWorkingOrders(response.value, timeline.baselineForRead().catalogResolver);
    },
    async queryStockHistory(code: StockCode): Promise<HistoricalStockData> {
      timeline.baselineForRead("Tauri 基线尚未就绪，不能查询股票历史");
      const cursor = timeline.captureQueryCursor();
      // Tauri 命令参数 code 为数值 StockId（ADR 0041）。
      const response = record(await invoke<unknown>("query_stock_history", { sessionId: requireSession(), generation: cursor.generation, code: timeline.baselineForRead().catalogResolver.stockId(code) }), "Tauri query_stock_history");
      assertResponseCurrent(cursor.generation, "query");
      timeline.assertQueryCursor(cursor, generation(response.generation, "Tauri query_stock_history.generation"), "Tauri 股票历史响应属于已过期会话 generation");
      return parseHistoricalStockData(response.value, code, timeline.baselineForRead().catalogResolver);
    },
    async initialAllocation() {
      timeline.baselineForRead("Tauri 基线尚未就绪，不能查询初始分配");
      const cursor = timeline.captureQueryCursor();
      const response = record(await invoke<unknown>("initial_allocation", { sessionId: requireSession(), generation: cursor.generation }), "Tauri 初始分配响应");
      exact(response, ["generation", "value"], "Tauri 初始分配响应");
      if (disposed) throw new Error("Tauri 初始分配响应属于已销毁会话");
      assertResponseCurrent(cursor.generation, "query");
      timeline.assertQueryCursor(cursor, generation(response.generation, "Tauri 初始分配响应.generation"), "Tauri 初始分配响应属于已过期会话 generation");
      return parseInitialAllocation(response.value, timeline.baselineForRead().stockCatalog.map(stock => stock.code), timeline.baselineForRead().catalogResolver);
    },
    async refreshBaseline() {
      const queryGeneration = timeline.captureGeneration();
      const restored = parseRestore(await invokeBaseline("engine_baseline", { sessionId: requireSession(), generation: queryGeneration }));
      assertResponseCurrent(queryGeneration, "refreshBaseline");
      const baseline = timeline.replaceRefreshedBaseline(restored, queryGeneration);
      await deliverInstalledBaseline(baseline, "refresh");
    },
    async load(slot, onRestored) {
      const id = requireSession();
      const wasRunning = running;
      const requestedGeneration = timeline.captureGeneration();
      if (wasRunning) {
        await invoke("pause_session", { sessionId: id });
        assertResponseCurrent(requestedGeneration, "load.pause");
        running = false;
      }
      const response = await invokeBaseline("restore_session", { sessionId: id, generation: requestedGeneration, slot });
      let restored: TauriBaselineResponse;
      let baseline: Extract<HostUpdate, { type: "baseline" }>;
      try {
        // invoke 成功后 engine 已可能提交；完整头部、目录与快照都在同一失效边界。
        restored = parseRestore(response);
        assertResponseCurrent(requestedGeneration, "load");
        baseline = timeline.replaceRestoredBaseline(restored);
        companyWriteFailure = null;
      } catch (error) {
        baselineUnavailable = true;
        fail({ code: "TAURI_RESTORE_BASELINE", where: "tauri-host.load", message: error instanceof Error ? error.message : String(error), cause: error });
        throw error;
      }
      onRestored?.();
      await deliverInstalledBaseline(baseline, "restore");
      if (wasRunning) {
        await invoke("resume_session", { sessionId: id });
        assertResponseCurrent(restored.generation, "load.resume");
        running = true;
      }
    },
    async queryPublicReports(query: PublicReportQuery): Promise<PublicReportPage> {
      const response = record(await invoke<unknown>("public_reports", { sessionId: requireSession(), generation: timeline.currentGeneration(), query }), "Tauri public_reports");
      assertBaselineAvailable();
      return normalizePublicReportPage(response.value);
    },
    async publicReportById(id: string): Promise<PublicReportSummary> {
      const response = record(await invoke<unknown>("public_report_by_id", { sessionId: requireSession(), generation: timeline.currentGeneration(), id }), "Tauri public_report_by_id");
      assertBaselineAvailable();
      return normalizePublicReportById(response.value);
    },
    async npcDecisionTrace(account: number): Promise<readonly NpcDecisionTraceRecord[]> {
      if (!npcDiagnosticsEnabled) throw new Error("Tauri 后端未协商启用 NPC 决策诊断");
      if (!Number.isSafeInteger(account) || account < 0) throw new Error("NPC 账户 ID 必须是非负安全整数");
      const cursor = timeline.captureQueryCursor();
      const queryGeneration = cursor.generation;
      const response = record(await invoke<unknown>("npc_decision_diagnostics", { sessionId: requireSession(), generation: queryGeneration, account }), "Tauri npc_decision_diagnostics");
      assertResponseCurrent(cursor.generation, "query");
      timeline.assertQueryCursor(cursor, generation(response.generation, "Tauri NPC diagnostics generation"), "Tauri NPC 诊断响应属于已过期会话 generation");
      const diagnostics = parseNpcDecisionDiagnostics(response.value, timeline.baselineForRead().catalogResolver);
      if (diagnostics.kind === "unsupported") throw new Error("Tauri 后端未提供 NPC 决策诊断数据");
      return diagnostics.records;
    },
    async calculateIndicators(input: IndicatorInput): Promise<IndicatorResults> {
      const normalized = normalizeIndicatorInput(input);
      const result = await invoke<unknown>("calculate_indicators", normalized);
      return normalizeIndicatorResults(result, normalized);
    },
    // W3 查询面：Tauri 命令通道尚未接线对应命令（desktop 批次），显式失败
    // 不静默回退（铁律二）——面板将错误原文展示给用户。
    async queryMarketHistory(): Promise<never> { throw new Error("Tauri 宿主尚未接入保留历史查询命令"); },
    async queryCurrentMinuteHistory(): Promise<never> { throw new Error("Tauri 宿主尚未接入当日分钟K查询命令"); },
    async queryPersonalTradeHistory(): Promise<never> { throw new Error("Tauri 宿主尚未接入本人交割历史查询命令"); },
    async queryPersonalTradeConfirmations(): Promise<never> { throw new Error("Tauri 宿主尚未接入本人交割单查询命令"); },
    async calculateIntradayAverage(): Promise<never> { throw new Error("Tauri 宿主尚未接入分时均价计算命令"); },
    async calculateIntradayAverageCurve(): Promise<never> { throw new Error("Tauri 宿主尚未接入分时均价曲线计算命令"); },
  };
}
