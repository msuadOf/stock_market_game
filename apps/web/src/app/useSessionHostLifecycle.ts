import { useEffect, type MutableRefObject } from "react";
import type { DeliveryMode, EngineHost } from "../host/engine-host.ts";
import type { HostFailure, HostUpdate } from "../host/host-update.ts";
import { createProtocolUpdate } from "../host/host-update.ts";
import { createTauriHost } from "../host/tauri-host.ts";
import { createRemoteHost } from "../host/remote-host.ts";
import type { WorkerE2EHost } from "../host/worker-host.ts";
import { assertWasmEnvironment, browserWasmEnvironment, fatalDesktopInitializationMessage, fatalRemoteInitializationMessage, fatalWasmInitializationMessage, type StartupTarget } from "../host/startup-policy.ts";
import { parseSessionSeed } from "../config/seed-draft.ts";
import type { SessionSetup } from "../types/engine.ts";
import { AutoOrderManager } from "../components/auto-order-manager.ts";
import type { IndicatorCalculator } from "../components/indicator-results.ts";
import type { PlayerOrderRefreshGate } from "../components/player-orders.ts";
import type { StrictSaveEnvelope } from "../save/schema/root.ts";
import { validateDayEndArchive } from "../save/day-end-candidate.ts";
import type { InitialSaveSource, SessionReplacementGate } from "../save/session-replacement.ts";
import type { DayEndPersistence } from "../save/day-end-persistence.ts";
import type { InitialAllocation } from "../host/initial-allocation.ts";

export interface SessionHostLifecyclePorts {
  hostRef: MutableRefObject<EngineHost | null>;
  initialSaveSourceRef: MutableRefObject<InitialSaveSource<StrictSaveEnvelope>>;
  dayEndPersistenceRef: MutableRefObject<DayEndPersistence>;
  autoOrderMgrRef: MutableRefObject<AutoOrderManager | null>;
  sessionReplacementGateRef: MutableRefObject<SessionReplacementGate>;
  saveSelectionGenerationRef: MutableRefObject<number>;
  playerOrderRefreshGateRef: MutableRefObject<PlayerOrderRefreshGate>;
  hostUpdateRef: MutableRefObject<(update: HostUpdate) => void | boolean>;
  fatalHostErrorRef: MutableRefObject<(failure: string | HostFailure) => void>;
  startupTarget: StartupTarget;
  sessionSetup: SessionSetup;
  speed: number;
  pauseAfterClose: boolean;
  pauseBeforeOpen: boolean;
  TRADING_E2E_MODE: boolean;
  malformedProtocolFixture(): boolean;
  setIndicatorCalculator(calculator: IndicatorCalculator): () => void;
  connectProtocol(host: EngineHost): void;
  disconnectProtocol(): void;
  createHost(setup: SessionSetup, seed: bigint, target: StartupTarget, resumeArchive?: boolean): Promise<EngineHost>;
  consumeChosenSessionSeed(): bigint;
  setSeedDraft(seed: string, preserveOrigin?: boolean): void;
  checkWasmEnvironment(): void;
  isDocumentHidden(): boolean;
  getBrowserSaveRepository(): { load(): Promise<StrictSaveEnvelope | null>; cancelPending(): void };
  setActiveSetup(setup: SessionSetup): void;
  configureMarketTiming(setup: SessionSetup): void;
  setStartDateDraft(date: string): void;
  setPriceCageEnabledDraft(enabled: boolean): void;
  setFloatAllocationDraft(allocation: SessionSetup["float_allocation"]): void;
  setReportFrequencyDraft(frequency: SessionSetup["report_frequency"]): void;
  setCompanySystemDraft(config: string): void;
  setInitialAllocation(allocation: InitialAllocation | null): void;
  setDeliveryModes(modes: readonly DeliveryMode[]): void;
  setDeliveryModeState(mode: DeliveryMode | null): void;
  setNotice(notice: string): void;
  setReady(ready: boolean): void;
  setError(error: string | HostFailure): void;
  onRunning(running: boolean): void;
  onAutoTriggered(id: string): void;
}

/** 单次 effect 的资源 owner；hostRef 只是 AppShell 借用的当前宿主别名。 */
export function createSessionHostLifecycle(ports: SessionHostLifecyclePorts) {
  const {
    hostRef, initialSaveSourceRef, dayEndPersistenceRef, autoOrderMgrRef, sessionReplacementGateRef,
    saveSelectionGenerationRef, playerOrderRefreshGateRef, startupTarget, sessionSetup, speed, pauseAfterClose,
    pauseBeforeOpen, TRADING_E2E_MODE, setIndicatorCalculator, setActiveSetup, configureMarketTiming, setStartDateDraft,
    setPriceCageEnabledDraft, setFloatAllocationDraft, setInitialAllocation, setDeliveryModes, setDeliveryModeState, setNotice, setReady, setError,
    hostUpdateRef, fatalHostErrorRef, connectProtocol, disconnectProtocol, createHost, checkWasmEnvironment,
    isDocumentHidden, getBrowserSaveRepository, onRunning, onAutoTriggered, malformedProtocolFixture,
    consumeChosenSessionSeed,
  } = ports;
  let cancelled = false;
  let ownedHost: EngineHost | null = null;
  let unsetIndicatorCalculator: (() => void) | null = null;
  let releasingHost = Promise.resolve();
  const releaseOwnedHost = async () => {
    unsetIndicatorCalculator?.();
    unsetIndicatorCalculator = null;
    if (hostRef.current === ownedHost) hostRef.current = null;
    disconnectProtocol();
    autoOrderMgrRef.current?.clear();
    autoOrderMgrRef.current = null;
    const host = ownedHost;
    ownedHost = null;
    if (host) releasingHost = host.dispose();
    await releasingHost;
  };
  const stopCurrentSession = async () => {
    cancelled = true;
    saveSelectionGenerationRef.current += 1;
    sessionReplacementGateRef.current.invalidate();
    playerOrderRefreshGateRef.current.invalidate();
    let stopFailure: unknown;
    try { if (ownedHost?.capabilities.persistence !== "remote") await ownedHost?.stop(); } catch (failure) { stopFailure = failure; }
    try { await releaseOwnedHost(); } catch (failure) {
      if (stopFailure !== undefined) throw new AggregateError([stopFailure, failure], "停止宿主与释放资源均失败");
      throw failure;
    }
    if (stopFailure !== undefined) throw stopFailure;
  };
  const playerOrderRefreshGate = playerOrderRefreshGateRef.current;
  const start = async () => {
    try {
      setInitialAllocation(null);
      if (startupTarget.kind === "wasm") checkWasmEnvironment();
      const resumeArchive = initialSaveSourceRef.current.shouldResume();
      const initialSlot = startupTarget.kind === "remote" ? null : await initialSaveSourceRef.current.read(async () => {
        if (TRADING_E2E_MODE || startupTarget.kind !== "wasm") return null;
        const slot = await getBrowserSaveRepository().load();
        return slot === null ? null : validateDayEndArchive(slot);
      });
      if (cancelled) return;
      const setup = initialSlot === null ? sessionSetup : initialSlot.setup;
      const seed = initialSlot === null ? consumeChosenSessionSeed() : parseSessionSeed(initialSlot.seed);
      const host = await createHost(setup, seed, startupTarget, resumeArchive);
      ownedHost = host;
      if (cancelled) {
        // React StrictMode 会执行一次探测性挂载；异步创建完成后必须停掉该宿主，避免泄漏 Worker/线程池。
        await releaseOwnedHost();
        return;
      }
      if (initialSlot !== null) await host.load(initialSlot);
      if (cancelled) return;
      const actualSetup = initialSlot === null && host.startupContext !== undefined ? host.startupContext.setup : setup;
      const actualSeed = initialSlot === null && host.startupContext !== undefined ? host.startupContext.seed : seed.toString();
      parseSessionSeed(actualSeed);
      if (initialSlot === null && host.startupContext?.resumed !== true) {
        const allocation = await host.initialAllocation();
        if (cancelled) return;
        setInitialAllocation(allocation);
      }
      setActiveSetup(actualSetup);
      configureMarketTiming(actualSetup);
      setStartDateDraft(actualSetup.start_date);
      setPriceCageEnabledDraft(actualSetup.config.price_cage_enabled);
      setFloatAllocationDraft(actualSetup.float_allocation);
      ports.setReportFrequencyDraft(actualSetup.report_frequency);
      ports.setCompanySystemDraft(JSON.stringify(actualSetup.company_system, null, 2));
      ports.setSeedDraft(actualSeed, initialSlot === null && host.startupContext?.resumed !== true);
      hostRef.current = host;
      unsetIndicatorCalculator = setIndicatorCalculator(host.calculateIndicators);
      connectProtocol(host);
      const supportedDeliveryModes = host.capabilities.deliveryModes;
      setDeliveryModes(supportedDeliveryModes);
      if (supportedDeliveryModes.length > 0) {
        if (!host.getDeliveryMode || !host.setDeliveryMode) {
          throw new Error("宿主声明支持刷新模式，但没有提供对应的读取或切换接口");
        }
        const supportedDeliveryMode = host.getDeliveryMode();
        if (!supportedDeliveryModes.includes(supportedDeliveryMode)) {
          throw new Error(`宿主当前刷新模式 ${supportedDeliveryMode} 不在其能力声明中`);
        }
        setDeliveryModeState(supportedDeliveryMode);
      } else {
        setDeliveryModeState(null);
      }
      // 初始化 AutoOrderManager
      autoOrderMgrRef.current = new AutoOrderManager(async (intent) => {
        const currentHost = hostRef.current;
        if (!currentHost) throw new Error("游戏引擎尚未就绪");
        await currentHost.submitIntent(intent);
      }, (id) => { if (!cancelled) onAutoTriggered(id); }, (_id, submitError) => {
        if (!cancelled) setNotice(`条件单提交失败：${submitError instanceof Error ? submitError.message : String(submitError)}`);
      });
      // 同步 RTK autoOrders → Manager
      if (host.capabilities.persistence !== "remote") await host.setSpeed(speed);
      if (cancelled) return;
      if (host.capabilities.persistence !== "remote") await host.setPausePreferences({ pause_after_close: pauseAfterClose, pause_before_open: pauseBeforeOpen });
      if (cancelled) return;
      await host.start(
        (update) => !cancelled && host === hostRef.current ? hostUpdateRef.current(update) : false,
        (failure) => { if (!cancelled && host === hostRef.current) fatalHostErrorRef.current(failure); },
      );
      if (cancelled) return;
      if (TRADING_E2E_MODE) {
        const controlledHost = host as EngineHost & Partial<WorkerE2EHost>;
        if (typeof controlledHost.stepOnceForE2E !== "function") {
          throw new Error("交易 E2E 模式需要本地 Worker 单步能力");
        }
        let controlledTick = host.tick();
        window.__STOCK_GAME_E2E__ = {
          async pause() {
            await host.stop();
            if (cancelled || host !== hostRef.current) return;
            onRunning(false);
          },
          async advanceToTick(target) {
            if (!Number.isSafeInteger(target) || target < controlledTick) {
              throw new Error(`E2E target tick ${target} is before current tick ${controlledTick}`);
            }
            while (controlledTick < target) controlledTick = await controlledHost.stepOnceForE2E!();
            return controlledTick;
          },
          snapshot: () => host.snapshot(),
        };
      }
      if (malformedProtocolFixture()) {
        queueMicrotask(() => hostUpdateRef.current(createProtocolUpdate("1", { Malformed: {} })));
      }
      // 初始化是异步的：页面可能已在宿主创建期间转入后台，而当时的
      // visibilitychange 监听器还拿不到 host。就绪后必须补做一次同步，
      // 避免隐藏页持续以 720x/最快占满 CPU。
      if (host.capabilities.persistence !== "remote" && isDocumentHidden()) await host.stop();
      if (cancelled) return;
      if (TRADING_E2E_MODE) {
        await host.stop();
        if (cancelled) return;
        onRunning(false);
      } else {
        onRunning(host.capabilities.persistence === "remote" ? (await host.readSpeedMetrics()).running : true);
      }
      if (!cancelled) {
        initialSaveSourceRef.current.complete();
        setReady(true);
      }
    } catch (e) {
      if (cancelled) setNotice(`已结束会话的启动或资源释放失败：${e instanceof Error ? e.message : String(e)}；请反馈此错误。`);
      if (!cancelled) {
        try { await releaseOwnedHost(); } catch (cleanupFailure) {
          setNotice(`初始化资源释放失败：${cleanupFailure instanceof Error ? cleanupFailure.message : String(cleanupFailure)}；请反馈此错误。`);
        }
        onRunning(false);
        setError(startupTarget.kind === "tauri"
          ? fatalDesktopInitializationMessage(e)
          : startupTarget.kind === "remote"
            ? fatalRemoteInitializationMessage(e)
            : fatalWasmInitializationMessage(e));
      }
    }
  };
  const dispose = async () => {
    cancelled = true;
    dayEndPersistenceRef.current.invalidate();
    if (startupTarget.kind === "wasm") getBrowserSaveRepository().cancelPending();
    saveSelectionGenerationRef.current += 1;
    sessionReplacementGateRef.current.invalidate();
    if (TRADING_E2E_MODE) delete window.__STOCK_GAME_E2E__;
    playerOrderRefreshGate.invalidate();
    await releaseOwnedHost();
  };
  return { start, stopCurrentSession, releaseOwnedHost, dispose };
}
async function createSessionHost(setup: SessionSetup, seed: bigint, startupTarget: StartupTarget, tradingE2EMode: boolean, resumeArchive: boolean): Promise<EngineHost> {
  if (startupTarget.kind === "tauri") return createTauriHost(setup, seed, { resumeArchive });
  if (startupTarget.kind === "remote") return createRemoteHost(setup, seed, { baseUrl: startupTarget.baseUrl, token: startupTarget.token, context: startupTarget.context });
  const { createWorkerHost } = await import("../host/worker-host.ts");
  return createWorkerHost(setup, seed, { enableE2EStepping: tradingE2EMode, threadCount: tradingE2EMode ? 2 : undefined });
}

interface Options extends Omit<SessionHostLifecyclePorts, "createHost" | "checkWasmEnvironment" | "isDocumentHidden"> {
  stopStartupRef: MutableRefObject<() => Promise<void>>;
  returningToStartupRef: MutableRefObject<boolean>;
  pausePreferencesReady: boolean;
  setHostBaselineReady(ready: boolean): void;
  refreshPlayerOrders(): Promise<void>;
}

export function useSessionHostLifecycle(options: Options): void {
  const { sessionSetup, startupTarget, pausePreferencesReady, refreshPlayerOrders, setIndicatorCalculator } = options;
  useEffect(() => {
    const lifecycle = createSessionHostLifecycle({ ...options,
      createHost: (setup, seed, target, resumeArchive) => createSessionHost(setup, seed, target, options.TRADING_E2E_MODE, resumeArchive === true),
      checkWasmEnvironment: () => assertWasmEnvironment(browserWasmEnvironment()),
      isDocumentHidden: () => document.hidden,
    });
    options.stopStartupRef.current = lifecycle.stopCurrentSession;
    if (!pausePreferencesReady || options.returningToStartupRef.current) return undefined;
    options.setHostBaselineReady(false);
    void lifecycle.start();
    return () => { void lifecycle.dispose().catch((failure) => options.setError(`释放宿主失败：${failure instanceof Error ? failure.message : String(failure)}；请反馈此错误。`)); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionSetup, startupTarget, pausePreferencesReady, refreshPlayerOrders, setIndicatorCalculator]);
}
