import { prepareDocumentWasmRestoreCapability } from "../host/wasm-restore-capability-source.ts";
import { useEffect, type MutableRefObject } from "react";
import { getDocumentWasmMemoryOwner, WasmDocumentMemoryError } from "../host/wasm-document-memory-owner.ts";
import type { WasmDocumentMemoryOwner, WasmMemoryAdmissionTicket } from "../host/wasm-document-memory-owner.ts";
import type { WasmMemoryHandover, WasmMemoryPendingIntent } from "../host/wasm-memory-handover.ts";
import type { DeliveryMode, EngineHost } from "../host/engine-host.ts";
import type { HostFailure, HostUpdate } from "../host/host-update.ts";
import { createProtocolUpdate } from "../host/host-update.ts";
import { createTauriHost } from "../host/tauri-host.ts";
import { createRemoteHost } from "../host/remote-host.ts";
import { createWorkerHost, type WorkerE2EHost } from "../host/worker-host.ts";
import { WasmHostInitializationError } from "../host/wasm-host-initialization-error.ts";
import { WasmMemoryStorageError } from "../host/wasm-memory-storage.ts";
import { parseWasmRuntimeOptions } from "../host/wasm-runtime-options.ts";
import { assertWasmEnvironment, browserWasmEnvironment, fatalDesktopInitializationMessage, fatalRemoteInitializationMessage, fatalWasmInitializationMessage, type StartupTarget } from "../host/startup-policy.ts";
import { DEFAULT_SEED } from "../config/defaults.ts";
import { createNewSessionSeed } from "../config/session-seed.ts";
import type { SessionSetup } from "../types/engine.ts";
import { AutoOrderManager } from "../components/auto-order-manager.ts";
import type { IndicatorCalculator } from "../components/indicator-results.ts";
import type { PlayerOrderRefreshGate } from "../components/player-orders.ts";
import type { StrictSaveEnvelope } from "../save/schema/root.ts";
import { validateDayEndArchive } from "../save/day-end-candidate.ts";
import type { InitialSaveSource, SessionReplacementGate } from "../save/session-replacement.ts";
import type { DayEndPersistence } from "../save/day-end-persistence.ts";
import type { InitialAllocation } from "../host/initial-allocation.ts";

// 这里只记录宿主释放的排序事实；真实 WASM 终值由 document Memory owner 独立确认。
export type HostTransitionOutcome = { readonly kind: "released" } | { readonly kind: "failed"; readonly error: unknown };

export interface SessionHostLifecyclePorts {
  hostRef: MutableRefObject<EngineHost | null>;
  hostTransitionRef: MutableRefObject<Promise<HostTransitionOutcome>>;
  wasmMemoryOwner?: WasmDocumentMemoryOwner;
  prepareWasmRestoreCapability?(archive: StrictSaveEnvelope): Promise<void>;
  confirmMemoryHandover?(handover: WasmMemoryHandover, explicitIntent?: Exclude<WasmMemoryPendingIntent, null>): Promise<WasmMemoryPendingIntent>;
  confirmMemoryReloadRetry?(handover: WasmMemoryHandover & { kind: "frozen" }): Promise<boolean>;
  reloadAfterMemoryNotice?(degraded: boolean): Promise<void>;
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
  createHost(setup: SessionSetup, seed: bigint, target: StartupTarget, onSpawnFailure: (failure: HostFailure) => void, initialArchive: StrictSaveEnvelope | null, memoryAdmissionTicket?: WasmMemoryAdmissionTicket): Promise<EngineHost>;
  onSpawnFailure(failure: HostFailure): void;
  createSeed?(): bigint;
  checkWasmEnvironment(): void;
  isDocumentHidden(): boolean;
  getBrowserSaveRepository(): { load(): Promise<StrictSaveEnvelope | null> };
  setActiveSetup(setup: SessionSetup): void;
  configureMarketTiming(setup: SessionSetup): void;
  setStartDateDraft(date: string): void;
  setPriceCageEnabledDraft(enabled: boolean): void;
  setFloatAllocationDraft(allocation: SessionSetup["float_allocation"]): void;
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
    createSeed = createNewSessionSeed,
  } = ports;
  let cancelled = false;
  let ownedHost: EngineHost | null = null;
  let unsetIndicatorCalculator: (() => void) | null = null;
  let releasingHost = Promise.resolve();
  let started = false, predecessorSettled = false, createPending = false, ending = false;
  let ownerEntered = false, sharedCleanupDone = false, blockedByPredecessor = false;
  let releases = 0;
  let outcome: HostTransitionOutcome = { kind: "released" };
  let finishTransition: ((value: HostTransitionOutcome) => void) | undefined;
  let teardownTask: Promise<void> | undefined;
  let memoryTicket: WasmMemoryAdmissionTicket | undefined;
  let closingTransportHost: EngineHost | null = null;
  const publishReleased = () => {
    if (!ending || !predecessorSettled || createPending || releases !== 0 || (ownedHost !== null && outcome.kind !== "failed")) return;
    finishTransition?.(outcome); finishTransition = undefined;
  };
  const releaseOwnedHost = async () => {
    releases++;
    const transportHost = ownedHost;
    if (transportHost !== null) closingTransportHost = transportHost;
    try {
      let hostReleaseFailure: { error: unknown } | undefined;
      const recordCleanupFailure = (error: unknown) => {
        hostReleaseFailure = { error: hostReleaseFailure === undefined ? error : new AggregateError([hostReleaseFailure.error, error], "多个宿主资源清理失败") };
      };
      // 一项 UI registration 解绑失败也不能越过真实宿主终结；各项错误仍原样报告。
      if (ownerEntered && !sharedCleanupDone) {
        sharedCleanupDone = true;
        try { unsetIndicatorCalculator?.(); } catch (error) { recordCleanupFailure(error); }
        unsetIndicatorCalculator = null;
        if (hostRef.current === ownedHost) hostRef.current = null;
        try { disconnectProtocol(); } catch (error) { recordCleanupFailure(error); }
        try { autoOrderMgrRef.current?.clear(); } catch (error) { recordCleanupFailure(error); }
        autoOrderMgrRef.current = null;
      }
      const host = ownedHost;
      ownedHost = null;
      try {
        if (host) {
          if (memoryTicket !== undefined) {
            try {
              // protocol delta 不保证更新宿主的 baseline 缓存；先停权威推进，再读取最终已提交位置。
              await host.stop();
              await host.refreshBaseline();
              ports.wasmMemoryOwner!.recordCommittedBoundary(memoryTicket, { tick: host.tick(), day: host.day() });
            } catch (error) { recordCleanupFailure(error); }
          }
          releasingHost = host.dispose();
        }
        await releasingHost;
      } catch (error) {
        hostReleaseFailure = { error: hostReleaseFailure === undefined ? error : new AggregateError([hostReleaseFailure.error, error], "读取提交边界与宿主释放均失败") };
      }
      // 未结束的 create 仍可能晚到一个 host；该次 cleanup 不提前确认 Memory 释放。
      try {
        if (memoryTicket !== undefined && !createPending) await ports.wasmMemoryOwner!.finish(memoryTicket);
      } catch (error) {
        if (hostReleaseFailure !== undefined) throw new AggregateError([hostReleaseFailure.error, error], "宿主释放与 Memory 终结回执均失败");
        throw error;
      }
      if (hostReleaseFailure !== undefined) throw hostReleaseFailure.error;
    } catch (error) {
      if (outcome.kind === "released") outcome = { kind: "failed", error };
      throw error;
    } finally {
      if (transportHost !== null && closingTransportHost === transportHost) closingTransportHost = null;
      releases--; publishReleased();
    }
  };
  const stopCurrentSession = () => {
    if (teardownTask !== undefined) return teardownTask;
    ending = true;
    if (ownedHost !== null) closingTransportHost = ownedHost;
    cancelled = true;
    if (ownerEntered) {
      saveSelectionGenerationRef.current += 1;
      sessionReplacementGateRef.current.invalidate();
      playerOrderRefreshGateRef.current.invalidate();
    }
    teardownTask = (async () => {
      let stopFailure: { readonly error: unknown } | null = null;
      try { if (memoryTicket === undefined) await ownedHost?.stop(); } catch (failure) {
        stopFailure = { error: failure };
        if (outcome.kind === "released") outcome = { kind: "failed", error: failure };
      }
      try { await releaseOwnedHost(); } catch (failure) {
        if (stopFailure !== null) throw new AggregateError([stopFailure.error, failure], "停止宿主与释放资源均失败");
        throw failure;
      }
      if (stopFailure !== null) throw stopFailure.error;
    })();
    return teardownTask;
  };
  const playerOrderRefreshGate = playerOrderRefreshGateRef.current;
  const reloadWithNotice = async (admitted: { degraded: boolean; diagnostics: unknown }) => {
    onRunning(false);
    setNotice(admitted.degraded ? "灾备台账写入失败，当前重载交接仅由单通道保护；详情已保留，重载后请确认目标。" : "Memory 残留已达到回收阈值，已冻结本次选择；重载后请确认目标。");
    if (ports.reloadAfterMemoryNotice === undefined) throw new WasmDocumentMemoryError("MEMORY_RESIDUAL_RELOAD_HANDOVER", "重载交接已保存；请重载后确认冻结目标。", { diagnostics: admitted.diagnostics });
    if (!cancelled) await ports.reloadAfterMemoryNotice(admitted.degraded);
    ending = true;
  };
  const start = async () => {
    if (started) throw new Error("同一 lifecycle 只允许 start 一次");
    started = true;
    const predecessor = ports.hostTransitionRef.current;
    ports.hostTransitionRef.current = new Promise((resolve) => { finishTransition = resolve; });
    try {
      let previous: HostTransitionOutcome;
      try { previous = await predecessor; }
      catch (error) {
        predecessorSettled = true; blockedByPredecessor = true; outcome = { kind: "failed", error };
        throw error;
      }
      predecessorSettled = true;
      if (previous.kind === "failed") { outcome = previous; blockedByPredecessor = true; ending = true; throw previous.error; }
      if (cancelled) return;
      ownerEntered = true;
      setInitialAllocation(null);
      if (startupTarget.kind === "wasm") checkWasmEnvironment();
      let confirmedIntent: Exclude<WasmMemoryPendingIntent, null> | undefined;
      let confirmedHandover = false;
      if (startupTarget.kind === "wasm" && ports.wasmMemoryOwner !== undefined) {
        const retry = ports.wasmMemoryOwner.pendingReloadRetry();
        if (retry !== undefined) {
          if (ports.confirmMemoryReloadRetry === undefined || !await ports.confirmMemoryReloadRetry(retry)) throw new WasmDocumentMemoryError("MEMORY_RELOAD_RETRY_CONFIRMATION_REQUIRED", "尚未确认重试原冻结交接，本次其他选择未受理，未分配游戏 Memory。");
          if (cancelled) return;
          await reloadWithNotice(ports.wasmMemoryOwner.retryReloadHandover());
          return;
        }
        const pending = ports.wasmMemoryOwner.pendingHandover();
        if (pending !== undefined) {
          if (ports.confirmMemoryHandover === undefined) throw new WasmDocumentMemoryError("MEMORY_HANDOVER_CONFIRMATION_REQUIRED", "请先确认冻结的存档或新局目标，未分配游戏 Memory。");
          let explicitIntent: Exclude<WasmMemoryPendingIntent, null> | undefined;
          if (initialSaveSourceRef.current.hasExplicitSelection()) {
            const selected = await initialSaveSourceRef.current.read(async () => { throw new Error("明确选择不允许读取快速槽"); });
            if (cancelled) return;
            explicitIntent = selected === null ? { kind: "new", setup: sessionSetup, seed: createSeed().toString() } : { kind: "load", archive: selected };
          }
          const choice = await ports.confirmMemoryHandover(pending, explicitIntent);
          if (cancelled) return;
          if (choice === null) throw new WasmDocumentMemoryError("MEMORY_HANDOVER_CONFIRMATION_REQUIRED", "尚未确认冻结的存档或新局目标，未分配游戏 Memory。");
          confirmedIntent = choice; confirmedHandover = true;
          if (choice.kind === "load") initialSaveSourceRef.current.select(choice.archive);
          else initialSaveSourceRef.current.reset();
        }
      }
      let initialSlot = confirmedIntent === undefined ? await initialSaveSourceRef.current.read(async () => {
        if (TRADING_E2E_MODE) return null;
        const slot = await getBrowserSaveRepository().load();
        return slot === null ? null : validateDayEndArchive(slot);
      }) : confirmedIntent.kind === "load" ? confirmedIntent.archive : null;
      if (cancelled) return;
      if (startupTarget.kind === "wasm" && initialSlot !== null && ports.prepareWasmRestoreCapability !== undefined) {
        await ports.prepareWasmRestoreCapability(initialSlot);
        if (cancelled) return;
      }
      let setup = initialSlot === null ? sessionSetup : initialSlot.setup;
      let seed = confirmedIntent?.kind === "new" ? BigInt(confirmedIntent.seed)
        : initialSlot === null ? (TRADING_E2E_MODE ? DEFAULT_SEED : createSeed()) : BigInt(initialSlot.seed);
      if (confirmedIntent?.kind === "new") setup = confirmedIntent.setup;
      if (startupTarget.kind === "wasm" && ports.wasmMemoryOwner !== undefined) {
        await dayEndPersistenceRef.current.idle();
        if (cancelled) return;
        const admitted = ports.wasmMemoryOwner.admit(initialSlot === null ? { kind: "new", setup, seed: seed.toString() } : { kind: "load", archive: initialSlot }, { confirmedHandover });
        if (admitted.kind === "reload") {
          await reloadWithNotice(admitted);
          return;
        }
        memoryTicket = admitted.ticket;
        if (admitted.intent.kind === "load") { initialSlot = admitted.intent.archive; setup = initialSlot.setup; seed = BigInt(initialSlot.seed); }
        else { setup = admitted.intent.setup; seed = BigInt(admitted.intent.seed); }
      }
      // 此 sink 属于本次 lifecycle；构造期尚无 hostRef，也必须立即交付。
      createPending = true;
      let host: EngineHost;
      try {
        const onSpawnFailure = (failure: HostFailure) => { if (!cancelled) ports.onSpawnFailure(failure); };
        if (memoryTicket === undefined) host = await createHost(setup, seed, startupTarget, onSpawnFailure, initialSlot);
        else host = await createHost(setup, seed, startupTarget, onSpawnFailure, initialSlot, memoryTicket);
      } finally { createPending = false; }
      ownedHost = host;
      if (cancelled) {
        // React StrictMode 会执行一次探测性挂载；异步创建完成后必须停掉该宿主，避免泄漏 Worker/线程池。
        await releaseOwnedHost();
        return;
      }
      if (initialSlot !== null && startupTarget.kind !== "wasm") await host.load(initialSlot);
      if (cancelled) return;
      if (initialSlot === null) {
        const allocation = await host.initialAllocation();
        if (cancelled) return;
        setInitialAllocation(allocation);
      }
      setActiveSetup(setup);
      configureMarketTiming(setup);
      setStartDateDraft(setup.start_date);
      setPriceCageEnabledDraft(setup.config.price_cage_enabled);
      setFloatAllocationDraft(setup.float_allocation);
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
      await host.setSpeed(speed);
      if (cancelled) return;
      await host.setPausePreferences({ pause_after_close: pauseAfterClose, pause_before_open: pauseBeforeOpen });
      if (cancelled) return;
      await host.start(
        (update) => {
          // 关闭旧宿主时仍须 ACK 已在途的 transport；取消的 UI 不再消费任何旧消息。
          if (closingTransportHost === host && (update.type === "baseline" || update.type === "protocol")) return true;
          return !cancelled && host === hostRef.current ? hostUpdateRef.current(update) : false;
        },
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
      if (isDocumentHidden()) await host.stop();
      if (cancelled) return;
      if (TRADING_E2E_MODE) {
        await host.stop();
        if (cancelled) return;
        onRunning(false);
      } else {
        onRunning(true);
      }
      if (!cancelled) {
        initialSaveSourceRef.current.complete();
        setReady(true);
      }
    } catch (e) {
      if (cancelled) {
        setNotice(`已结束会话的启动或资源释放失败：${e instanceof Error ? e.message : String(e)}；请反馈此错误。`);
        try { await releaseOwnedHost(); } catch (error) { setNotice(`旧会话 Memory 释放未确认：${error instanceof Error ? error.message : String(error)}；请反馈此错误。`); }
      }
      if (!cancelled) {
        try { await releaseOwnedHost(); } catch (cleanupFailure) {
          setNotice(`初始化资源释放失败：${cleanupFailure instanceof Error ? cleanupFailure.message : String(cleanupFailure)}；请反馈此错误。`);
        }
        onRunning(false);
        setError(blockedByPredecessor ? {
          code: "HOST_PREVIOUS_RELEASE_FAILED", where: "session-host-lifecycle.predecessor",
          message: "上一宿主资源释放失败，禁止创建下一宿主；请保留详情反馈并重新打开应用。",
          cause: e, recoverable: false, recoveryActions: ["复制错误详情反馈", "重新打开应用"],
        } : startupTarget.kind === "tauri"
          ? fatalDesktopInitializationMessage(e)
          : startupTarget.kind === "remote"
            ? fatalRemoteInitializationMessage(e)
            : e instanceof WasmHostInitializationError ? e.failure : (e instanceof WasmDocumentMemoryError || e instanceof WasmMemoryStorageError) ? e : fatalWasmInitializationMessage(e));
      }
      ending = true;
    } finally { publishReleased(); }
  };
  const dispose = () => {
    if (teardownTask !== undefined) return teardownTask;
    ending = true;
    if (ownedHost !== null) closingTransportHost = ownedHost;
    cancelled = true;
    if (ownerEntered) {
      dayEndPersistenceRef.current.invalidate();
      saveSelectionGenerationRef.current += 1;
      sessionReplacementGateRef.current.invalidate();
      if (TRADING_E2E_MODE) delete window.__STOCK_GAME_E2E__;
      playerOrderRefreshGate.invalidate();
    }
    teardownTask = releaseOwnedHost();
    return teardownTask;
  };
  return { start, stopCurrentSession, releaseOwnedHost, dispose };
}
export function createSessionHost(setup: SessionSetup, seed: bigint, startupTarget: StartupTarget, tradingE2EMode: boolean, onSpawnFailure: (failure: HostFailure) => void, initialArchive: StrictSaveEnvelope | null, memoryAdmissionTicket?: WasmMemoryAdmissionTicket): Promise<EngineHost> {
  if (startupTarget.kind === "tauri") return createTauriHost(setup, seed);
  if (startupTarget.kind === "remote") return createRemoteHost(setup, seed, { baseUrl: startupTarget.baseUrl });
  let wasmRuntimeOptions;
  try { wasmRuntimeOptions = parseWasmRuntimeOptions(window.location.search); }
  catch (error) {
    // query 校验尚未调用 Worker 工厂，只有这里能明确证明本票据没有启动分配。
    try { memoryAdmissionTicket?.registerRelease(Promise.resolve({ kind: "not-allocated" })); }
    catch (registrationError) {
      throw new AggregateError([error, registrationError], `WASM runtime query 校验失败：${error instanceof Error ? error.message : String(error)}；零分配回执登记也失败：${registrationError instanceof Error ? registrationError.message : String(registrationError)}`);
    }
    throw error;
  }
  const options = { enableE2EStepping: tradingE2EMode, onSpawnFailure, initialArchive, memoryAdmissionTicket, ...wasmRuntimeOptions };
  return createWorkerHost(setup, seed, options);
}

interface Options extends Omit<SessionHostLifecyclePorts, "createHost" | "checkWasmEnvironment" | "isDocumentHidden" | "hostTransitionRef" | "wasmMemoryOwner" | "prepareWasmRestoreCapability" | "confirmMemoryHandover" | "confirmMemoryReloadRetry" | "reloadAfterMemoryNotice"> {
  stopStartupRef: MutableRefObject<() => Promise<void>>;
  returningToStartupRef: MutableRefObject<boolean>;
  pausePreferencesReady: boolean;
  setHostBaselineReady(ready: boolean): void;
  refreshPlayerOrders(): Promise<void>;
}

export function useSessionHostLifecycle(options: Options): void {
  const { sessionSetup, startupTarget, pausePreferencesReady, refreshPlayerOrders, setIndicatorCalculator } = options;
  useEffect(() => {
    const documentOwner = getDocumentWasmMemoryOwner(document);
    const lifecycle = createSessionHostLifecycle({ ...options,
      hostTransitionRef: documentOwner.hostTransitionRef,
      wasmMemoryOwner: startupTarget.kind === "wasm" ? documentOwner : undefined,
      prepareWasmRestoreCapability: async () => {
        const runtime = parseWasmRuntimeOptions(window.location.search);
        await prepareDocumentWasmRestoreCapability(document, { runtimeOverride: runtime.runtimeOverride });
      },
      confirmMemoryHandover: async (handover, explicitIntent) => {
        const intent = explicitIntent === undefined ? handover.kind === "frozen" ? handover.pendingIntent : null : explicitIntent;
        if (intent !== null) {
          const target = intent.kind === "load" ? `日终存档（seed ${intent.archive.seed}）` : `新局（seed ${intent.seed}）`;
          const selection = explicitIntent === undefined ? "上次明确选择" : "本次明确选择";
          return window.confirm(`是否继续${selection}的${target}？确认后才分配游戏 Memory。`) ? intent : null;
        }
        return window.confirm("重载交接目标不可用。是否明确选择按当前配置创建新局？原日终存档不会被改写。")
          ? { kind: "new", setup: options.sessionSetup, seed: createNewSessionSeed().toString() } : null;
      },
      confirmMemoryReloadRetry: async (handover) => {
        const intent = handover.pendingIntent;
        const target = intent === null ? "原冻结选择" : intent.kind === "load" ? `原日终存档（seed ${intent.archive.seed}）` : `原新局（seed ${intent.seed}）`;
        return window.confirm(`上次交接尚未持久化。是否明确重试${target}？本次其他选择尚未受理。`);
      },
      reloadAfterMemoryNotice: async (degraded) => {
        if (degraded) window.alert("灾备台账写入失败，本次交接仅由单通道保护。确认此提示后将重载，并再次确认已冻结目标。");
        await new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
        window.location.reload();
      },
      createHost: (setup, seed, target, onSpawnFailure, initialArchive, memoryAdmissionTicket) => createSessionHost(setup, seed, target, options.TRADING_E2E_MODE, onSpawnFailure, initialArchive, memoryAdmissionTicket),
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
