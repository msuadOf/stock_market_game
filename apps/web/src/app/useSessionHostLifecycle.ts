import type { ChunkedArchiveRestore } from "./chunked-archive-restore.ts";
import { getInitialSaveValueConfiguration, isLeaseInitialSave, type InitialSaveValue } from "../save/initial-save-selection.ts";
import type { ValidatedArchiveLease } from "../save/validated-archive-lease.ts";
import { describeValidatedArchiveLease, type ChunkedSourceDescriptor, type PinnedArchiveSource } from "../save/chunked-handover-source.ts";
import { prepareDocumentWasmRestoreCapability } from "../host/wasm-restore-capability-source.ts";
import { store } from "../store/store.ts";
import { useEffect, useState, type MutableRefObject } from "react";
import { createMemoryHandoverSelection } from "./memory-handover-selection.ts";
import type { MemoryHandoverPanelProps } from "./MemoryHandoverPanel.tsx";
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
import { describeInitialSaveReadFailure } from "../save/session-replacement.ts";
import type { InitialSaveSource, SessionReplacementGate } from "../save/session-replacement.ts";
import type { DayEndPersistence } from "../save/day-end-persistence.ts";
import type { InitialAllocation } from "../host/initial-allocation.ts";

// 这里只记录宿主释放的排序事实；真实 WASM 终值由 document Memory owner 独立确认。
export type HostTransitionOutcome = { readonly kind: "released" } | { readonly kind: "failed"; readonly error: unknown };

export interface SessionHostLifecyclePorts {
  hostRef: MutableRefObject<EngineHost | null>;
  hostTransitionRef: MutableRefObject<Promise<HostTransitionOutcome>>;
  wasmMemoryOwner?: WasmDocumentMemoryOwner;
  chunkedRestore?: ChunkedArchiveRestore;
  prepareWasmRestoreCapability?(archive: StrictSaveEnvelope): Promise<void>;
  confirmMemoryHandover?(handover: WasmMemoryHandover, explicitIntent?: Exclude<WasmMemoryPendingIntent, null>): Promise<WasmMemoryPendingIntent>;
  confirmMemoryReloadRetry?(handover: WasmMemoryHandover & { kind: "frozen" }): Promise<boolean>;
  onMemoryHandoverCancelled?(): void | Promise<void>;
  reloadAfterMemoryNotice?(degraded: boolean, isCurrent: () => boolean): Promise<void>;
  initialSaveSourceRef: MutableRefObject<InitialSaveSource<InitialSaveValue>>;
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
  createHost(setup: SessionSetup, seed: bigint, target: StartupTarget, onSpawnFailure: (failure: HostFailure) => void, initialArchive: StrictSaveEnvelope | null, memoryAdmissionTicket?: WasmMemoryAdmissionTicket, onInitializationProgress?: (message: string) => void): Promise<EngineHost>;
  onSpawnFailure(failure: HostFailure): void;
  createSeed?(): bigint;
  checkWasmEnvironment(): void;
  isDocumentHidden(): boolean;
  getBrowserSaveRepository(): { load(): Promise<StrictSaveEnvelope | null>; loadFrozen(): Promise<StrictSaveEnvelope | null>; loadValidatedLease?(signal?: AbortSignal): Promise<ValidatedArchiveLease | null> };
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

function includesFailure(error: unknown, target: unknown, seen = new Set<unknown>()): boolean {
  if (error === target) return true;
  if (!(error instanceof AggregateError) || seen.has(error)) return false;
  seen.add(error);
  return error.errors.some(cause => includesFailure(cause, target, seen));
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
  let selectionSignal: AbortSignal | undefined;
  let removeSelectionAbort: (() => void) | undefined;
  const attemptController = new AbortController();
  const isStartupCancelled = () => cancelled || selectionSignal?.aborted === true;
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
    if (finishTransition === undefined) return;
    const readReleaseFailure = initialSaveSourceRef.current.readReleaseFailure();
    if (readReleaseFailure !== undefined) {
      if (outcome.kind === "released") outcome = { kind: "failed", error: readReleaseFailure };
      else if (!includesFailure(outcome.error, readReleaseFailure)) outcome = {
        kind: "failed", error: new AggregateError([outcome.error, readReleaseFailure], "宿主与存档读取资源释放均失败"),
      };
    }
    finishTransition(outcome); finishTransition = undefined;
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
    attemptController.abort(new Error("宿主启动尝试已结束"));
    removeSelectionAbort?.(); removeSelectionAbort = undefined;
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
  const cancelMemoryHandover = async () => {
    ending = true;
    onRunning(false);
    await releaseOwnedHost();
    await ports.onMemoryHandoverCancelled?.();
  };
  const reloadWithNotice = async (admitted: { degraded: boolean; diagnostics: unknown }) => {
    // 已冻结的旧目标保留供确认，但不能覆盖其间产生的新选择。
    if (isStartupCancelled()) { ending = true; return; }
    onRunning(false);
    setNotice(admitted.degraded ? "灾备台账写入失败，当前重载交接仅由单通道保护；详情已保留，重载后请确认目标。" : "Memory 残留已达到回收阈值，已冻结本次选择；重载后请确认目标。");
    if (ports.reloadAfterMemoryNotice === undefined) throw new WasmDocumentMemoryError("MEMORY_RESIDUAL_RELOAD_HANDOVER", "重载交接已保存；请重载后确认冻结目标。", { diagnostics: admitted.diagnostics });
    if (!isStartupCancelled()) await ports.reloadAfterMemoryNotice(admitted.degraded, () => !isStartupCancelled());
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
      if (isStartupCancelled()) return;
      ownerEntered = true;
      setInitialAllocation(null);
      if (startupTarget.kind === "wasm") checkWasmEnvironment();
      selectionSignal = initialSaveSourceRef.current.selectionSignal();
      let confirmedIntent: Exclude<WasmMemoryPendingIntent, null> | undefined;
      let explicitLeaseChoice: { descriptor: ChunkedSourceDescriptor; lease: ValidatedArchiveLease } | undefined;
      let confirmedHandover = false;
      if (startupTarget.kind === "wasm" && ports.wasmMemoryOwner !== undefined) {
        const ready = ports.wasmMemoryOwner.pendingReloadReady();
        if (ready !== undefined) {
          if (!initialSaveSourceRef.current.isSelectionFrozen()) initialSaveSourceRef.current.freezeSelection(selectionSignal);
          await reloadWithNotice(ready);
          return;
        }
        const retry = ports.wasmMemoryOwner.pendingReloadRetry();
        if (retry !== undefined) {
          if (ports.confirmMemoryReloadRetry === undefined) throw new WasmDocumentMemoryError("MEMORY_RELOAD_RETRY_CONFIRMATION_REQUIRED", "尚未提供原冻结交接重试确认，未分配游戏 Memory。");
          if (!await ports.confirmMemoryReloadRetry(retry)) { if (!isStartupCancelled()) await cancelMemoryHandover(); return; }
          if (isStartupCancelled()) return;
          const retryDescriptor = retry.pendingIntent?.kind === "load-chunked-source" ? retry.pendingIntent : undefined;
          const sources = ports.chunkedRestore?.handoverSources;
          if (retryDescriptor !== undefined && sources === undefined) throw new Error("重载重试缺少固定源适配器");
          const releaseFreeze = initialSaveSourceRef.current.freezeSelection(selectionSignal);
          let published = false;
          try {
            const retried = retryDescriptor === undefined ? ports.wasmMemoryOwner.retryReloadHandover()
              : await initialSaveSourceRef.current.runSelectionRead(signal => sources!.withDescriptorPublication(retryDescriptor, signal, () => {
                if (isStartupCancelled()) throw new Error("原冻结交接的重试已取消");
                return ports.wasmMemoryOwner!.retryReloadHandover();
              }));
            published = true;
            await reloadWithNotice(retried);
          } finally { if (!published) releaseFreeze(); }
          return;
        }
        const pending = ports.wasmMemoryOwner.pendingHandover();
        if (pending !== undefined) {
          if (ports.confirmMemoryHandover === undefined) throw new WasmDocumentMemoryError("MEMORY_HANDOVER_CONFIRMATION_REQUIRED", "请先确认冻结的存档或新局目标，未分配游戏 Memory。");
          let explicitIntent: Exclude<WasmMemoryPendingIntent, null> | undefined;
          if (initialSaveSourceRef.current.hasExplicitSelection()) {
            const selected = await initialSaveSourceRef.current.read(async () => { throw new Error("明确选择不允许读取快速槽"); });
            if (isStartupCancelled()) return;
            if (selected !== null && isLeaseInitialSave(selected)) {
              const descriptor = describeValidatedArchiveLease(selected.lease);
              explicitLeaseChoice = { descriptor, lease: selected.lease };
              explicitIntent = descriptor;
            } else explicitIntent = selected === null ? { kind: "new", setup: sessionSetup, seed: createSeed().toString() } : { kind: "load", archive: selected };
          }
          const choice = await ports.confirmMemoryHandover(pending, explicitIntent);
          if (isStartupCancelled()) return;
          if (choice === null) { await cancelMemoryHandover(); return; }
          confirmedIntent = choice; confirmedHandover = true;
          if (choice.kind === "load") initialSaveSourceRef.current.select(choice.archive);
          else if (choice.kind === "new") initialSaveSourceRef.current.reset();
        }
      }
      selectionSignal = initialSaveSourceRef.current.selectionSignal();
      const initialValue: InitialSaveValue | null = confirmedIntent === undefined ? await initialSaveSourceRef.current.read(async () => {
        if (TRADING_E2E_MODE) return null;
        const repository = getBrowserSaveRepository();
        if (startupTarget.kind === "wasm" && ports.chunkedRestore !== undefined) {
          if (repository.loadValidatedLease === undefined) throw new Error("已注入分块恢复能力，但 repository 缺少显式租约读取入口");
          const loadLease = repository.loadValidatedLease;
          const lease = await initialSaveSourceRef.current.runSelectionRead(signal => loadLease.call(repository, signal));
          return lease === null ? null : { kind: "lease", lease };
        }
        const slot = startupTarget.kind === "wasm" ? await repository.loadFrozen() : await repository.load();
        return slot === null ? null : validateDayEndArchive(slot);
      }) : confirmedIntent.kind === "load" ? confirmedIntent.archive : confirmedIntent.kind === "load-chunked-source" ? await (async () => {
        const sources = ports.chunkedRestore?.handoverSources;
        if (sources === undefined) throw new Error("分块重载缺少固定源适配器");
        const descriptor = confirmedIntent;
        const lease = explicitLeaseChoice !== undefined && JSON.stringify(explicitLeaseChoice.descriptor) === JSON.stringify(descriptor)
          ? explicitLeaseChoice.lease
          : await initialSaveSourceRef.current.runSelectionRead(signal => sources.resolve(descriptor, signal));
        if (isStartupCancelled()) return null;
        const selected = { kind: "lease" as const, lease };
        initialSaveSourceRef.current.select(selected);
        selectionSignal = initialSaveSourceRef.current.selectionSignal();
        return selected;
      })() : null;
      if (isStartupCancelled()) return;
      const capturedSignal = selectionSignal;
      const abortSelection = () => { attemptController.abort(capturedSignal.reason); };
      capturedSignal.addEventListener("abort", abortSelection, { once: true });
      removeSelectionAbort = () => { capturedSignal.removeEventListener("abort", abortSelection); };
      await initialSaveSourceRef.current.waitReadRelease();
      if (isStartupCancelled()) return;
      let initialLease = initialValue !== null && isLeaseInitialSave(initialValue) ? initialValue.lease : null;
      let initialSlot = initialValue === null || isLeaseInitialSave(initialValue) ? null : initialValue;
      let pinnedSource: PinnedArchiveSource | undefined;
      if (initialLease !== null) {
        if (startupTarget.kind !== "wasm" || ports.chunkedRestore === undefined) throw new Error("当前宿主未配置分块租约恢复能力，不能将租约回退为 String 存档");
        if (ports.wasmMemoryOwner === undefined) throw new Error("分块租约恢复必须由 document Memory owner 签票");
        const sources = ports.chunkedRestore.handoverSources;
        if (sources !== undefined && ports.wasmMemoryOwner.chunkedHandoverNeeded()) {
          const lease = initialLease;
          pinnedSource = await initialSaveSourceRef.current.runSelectionRead(signal => sources.pin(lease, signal));
          if (isStartupCancelled()) return;
        }
        await ports.chunkedRestore.prepareCapability(initialLease);
        if (isStartupCancelled()) return;
      } else if (startupTarget.kind === "wasm" && initialSlot !== null && ports.prepareWasmRestoreCapability !== undefined) {
        await ports.prepareWasmRestoreCapability(initialSlot);
        if (isStartupCancelled()) return;
      }
      const configuration = initialValue === null ? null : getInitialSaveValueConfiguration(initialValue);
      let setup = configuration === null ? sessionSetup : configuration.setup;
      let seed = confirmedIntent?.kind === "new" ? BigInt(confirmedIntent.seed)
        : configuration === null ? (TRADING_E2E_MODE ? DEFAULT_SEED : createSeed()) : BigInt(configuration.seed);
      if (confirmedIntent?.kind === "new") setup = confirmedIntent.setup;
      if (startupTarget.kind === "wasm" && ports.wasmMemoryOwner !== undefined) {
        await dayEndPersistenceRef.current.idle();
        await initialSaveSourceRef.current.waitReadRelease();
        if (isStartupCancelled()) return;
        if (initialLease !== null) {
          const lease = initialLease;
          const releaseFreeze = pinnedSource === undefined ? undefined : initialSaveSourceRef.current.freezeSelection(capturedSignal);
          let published = false;
          let admitted;
          try {
            admitted = pinnedSource === undefined ? (confirmedHandover ? ports.wasmMemoryOwner.admitChunked(lease, { confirmedHandover }) : ports.wasmMemoryOwner.admitChunked(lease))
              : await initialSaveSourceRef.current.runSelectionRead(signal => ports.chunkedRestore!.handoverSources!.withPinnedPublication(pinnedSource!, lease, signal, () => {
                if (isStartupCancelled()) throw new Error("分块存档的发布已取消");
                return ports.wasmMemoryOwner!.admitChunked(lease, { pinnedSource, confirmedHandover });
              }));
            published = admitted.kind === "reload";
          } finally { if (!published) releaseFreeze?.(); }
          if (admitted.kind === "reload") { await reloadWithNotice(admitted); return; }
          if (admitted.intent.kind !== "load-chunked" || admitted.intent.lease !== initialLease) throw new Error("分块 Memory ticket 与初始租约不一致");
          memoryTicket = admitted.ticket;
          if (isStartupCancelled()) {
            memoryTicket.registerRelease(Promise.resolve({ kind: "not-allocated" }));
            await releaseOwnedHost(); return;
          }
          const admittedConfig = getInitialSaveValueConfiguration({ kind: "lease", lease: admitted.intent.lease });
          setup = admittedConfig.setup; seed = BigInt(admittedConfig.seed);
        } else {
          const releaseFreeze = ports.wasmMemoryOwner.chunkedHandoverNeeded() ? initialSaveSourceRef.current.freezeSelection(capturedSignal) : undefined;
          let published = false;
          let admitted;
          try {
            admitted = ports.wasmMemoryOwner.admit(initialSlot === null ? { kind: "new", setup, seed: seed.toString() } : { kind: "load", archive: initialSlot }, { confirmedHandover });
            published = admitted.kind === "reload";
          } finally { if (!published) releaseFreeze?.(); }
          if (admitted.kind === "reload") { await reloadWithNotice(admitted); return; }
          memoryTicket = admitted.ticket;
          if (admitted.intent.kind === "load") { initialSlot = admitted.intent.archive; setup = initialSlot.setup; seed = BigInt(initialSlot.seed); }
          else { setup = admitted.intent.setup; seed = BigInt(admitted.intent.seed); }
        }
      }
      // 此 sink 属于本次 lifecycle；构造期尚无 hostRef，也必须立即交付。
      createPending = true;
      let host: EngineHost;
      try {
        const onSpawnFailure = (failure: HostFailure) => { if (!isStartupCancelled()) ports.onSpawnFailure(failure); };
        const onInitializationProgress = (message: string) => { if (!isStartupCancelled()) ports.setNotice(message); };
        if (initialLease !== null) {
          if (ports.chunkedRestore === undefined || memoryTicket === undefined) throw new Error("分块恢复工厂或 Memory ticket 不可用");
          host = await ports.chunkedRestore.createHost({ setup, seed, lease: initialLease, ticket: memoryTicket, signal: attemptController.signal, onSpawnFailure, onInitializationProgress });
        } else if (memoryTicket === undefined) host = await createHost(setup, seed, startupTarget, onSpawnFailure, initialSlot, undefined, onInitializationProgress);
        else host = await createHost(setup, seed, startupTarget, onSpawnFailure, initialSlot, memoryTicket, onInitializationProgress);
      } finally { createPending = false; }
      ownedHost = host;
      if (isStartupCancelled()) {
        // React StrictMode 会执行一次探测性挂载；异步创建完成后必须停掉该宿主，避免泄漏 Worker/线程池。
        await releaseOwnedHost();
        return;
      }
      if (initialSlot !== null && startupTarget.kind !== "wasm") await host.load(initialSlot);
      if (isStartupCancelled()) return;
      if (initialSlot === null && initialLease === null) {
        const allocation = await host.initialAllocation();
        if (isStartupCancelled()) return;
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
      }, (id) => { if (!isStartupCancelled()) onAutoTriggered(id); }, (_id, submitError) => {
        if (!isStartupCancelled()) setNotice(`条件单提交失败：${submitError instanceof Error ? submitError.message : String(submitError)}`);
      });
      // 同步 RTK autoOrders → Manager
      await host.setSpeed(speed);
      if (isStartupCancelled()) return;
      await host.setPausePreferences({ pause_after_close: pauseAfterClose, pause_before_open: pauseBeforeOpen });
      if (isStartupCancelled()) return;
      await host.start(
        (update) => {
          // 关闭旧宿主时仍须 ACK 已在途的 transport；取消的 UI 不再消费任何旧消息。
          if ((closingTransportHost === host || (isStartupCancelled() && host === hostRef.current))
            && (update.type === "baseline" || update.type === "protocol")) return true;
          return !isStartupCancelled() && host === hostRef.current ? hostUpdateRef.current(update) : false;
        },
        (failure) => { if (!isStartupCancelled() && host === hostRef.current) fatalHostErrorRef.current(failure); },
      );
      if (isStartupCancelled()) return;
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
          snapshot: () => {
            const snapshot = store.getState().snapshot.snapshot;
            if (snapshot === null) throw new Error("E2E 实时快照尚未由当前会话交付");
            return snapshot;
          },
        };
      }
      if (malformedProtocolFixture()) {
        queueMicrotask(() => hostUpdateRef.current(createProtocolUpdate("1", { Malformed: {} })));
      }
      // 初始化是异步的：页面可能已在宿主创建期间转入后台，而当时的
      // visibilitychange 监听器还拿不到 host。就绪后必须补做一次同步，
      // 避免隐藏页持续以 720x/最快占满 CPU。
      if (isDocumentHidden()) await host.stop();
      if (isStartupCancelled()) return;
      if (TRADING_E2E_MODE) {
        await host.stop();
        if (isStartupCancelled()) return;
        onRunning(false);
      } else {
        onRunning(true);
      }
      if (!isStartupCancelled()) {
        removeSelectionAbort?.(); removeSelectionAbort = undefined; selectionSignal = undefined;
        initialSaveSourceRef.current.complete();
        if (initialSlot !== null || initialLease !== null) setNotice(`已读档（第 ${host.day() + 1} 个交易日）`);
        setReady(true);
      }
    } catch (e) {
      if (!cancelled && selectionSignal?.aborted) {
        if (e !== selectionSignal.reason) setNotice(`过期存档读取或清理失败：${describeInitialSaveReadFailure(e)}；请反馈此错误。`);
        ending = true;
        return;
      }
      if (isStartupCancelled()) {
        setNotice(`已结束会话的启动或资源释放失败：${describeInitialSaveReadFailure(e)}；请反馈此错误。`);
        try { await releaseOwnedHost(); } catch (error) { setNotice(`旧会话 Memory 释放未确认：${error instanceof Error ? error.message : String(error)}；请反馈此错误。`); }
      }
      if (!isStartupCancelled()) {
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
    } finally {
      removeSelectionAbort?.(); removeSelectionAbort = undefined;
      if (!cancelled && selectionSignal?.aborted) {
        ending = true;
        try { await releaseOwnedHost(); }
        catch (error) { setNotice(`过期选择的宿主释放失败：${error instanceof Error ? error.message : String(error)}；请反馈此错误。`); }
      }
      publishReleased();
    }
  };
  const dispose = () => {
    if (teardownTask !== undefined) return teardownTask;
    ending = true;
    if (ownedHost !== null) closingTransportHost = ownedHost;
    cancelled = true;
    attemptController.abort(new Error("宿主启动尝试已结束"));
    removeSelectionAbort?.(); removeSelectionAbort = undefined;
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
export function createSessionHost(setup: SessionSetup, seed: bigint, startupTarget: StartupTarget, tradingE2EMode: boolean, onSpawnFailure: (failure: HostFailure) => void, initialArchive: StrictSaveEnvelope | null, memoryAdmissionTicket?: WasmMemoryAdmissionTicket, onInitializationProgress?: (message: string) => void): Promise<EngineHost> {
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
  const options = { enableE2EStepping: tradingE2EMode, onSpawnFailure, initialArchive, memoryAdmissionTicket, onInitializationProgress, ...wasmRuntimeOptions };
  return createWorkerHost(setup, seed, options);
}

interface Options extends Omit<SessionHostLifecyclePorts, "createHost" | "checkWasmEnvironment" | "isDocumentHidden" | "hostTransitionRef" | "wasmMemoryOwner" | "prepareWasmRestoreCapability" | "confirmMemoryHandover" | "confirmMemoryReloadRetry" | "reloadAfterMemoryNotice"> {
  stopStartupRef: MutableRefObject<() => Promise<void>>;
  returningToStartupRef: MutableRefObject<boolean>;
  pausePreferencesReady: boolean;
  setHostBaselineReady(ready: boolean): void;
  refreshPlayerOrders(): Promise<void>;
  loadMemoryHandoverFile(): Promise<StrictSaveEnvelope | null>;
}

export function useSessionHostLifecycle(options: Options): MemoryHandoverPanelProps | null {
  const [memoryHandoverPanel, setMemoryHandoverPanel] = useState<MemoryHandoverPanelProps | null>(null);
  const { sessionSetup, startupTarget, pausePreferencesReady, refreshPlayerOrders, setIndicatorCalculator } = options;
  useEffect(() => {
    const selection = createMemoryHandoverSelection({
      loadFromFile: options.loadMemoryHandoverFile,
      createNewIntent: () => ({ kind: "new", setup: options.sessionSetup, seed: (options.createSeed === undefined ? createNewSessionSeed() : options.createSeed()).toString() }),
      onChange: setMemoryHandoverPanel,
    });
    setMemoryHandoverPanel(null);
    const documentOwner = getDocumentWasmMemoryOwner(document);
    const lifecycle = createSessionHostLifecycle({ ...options,
      hostTransitionRef: documentOwner.hostTransitionRef,
      wasmMemoryOwner: startupTarget.kind === "wasm" ? documentOwner : undefined,
      prepareWasmRestoreCapability: async () => {
        const runtime = parseWasmRuntimeOptions(window.location.search);
        await prepareDocumentWasmRestoreCapability(document, { runtimeOverride: runtime.runtimeOverride });
      },
      confirmMemoryHandover: selection.request,
      confirmMemoryReloadRetry: async (handover) => {
        const intent = handover.pendingIntent;
        const target = intent === null ? "原冻结选择" : intent.kind === "load" ? `原日终存档（seed ${intent.archive.seed}）` : intent.kind === "load-chunked-source" ? `原日终存档（seed ${intent.seed}）` : `原新局（seed ${intent.seed}）`;
        return window.confirm(`上次交接尚未持久化。是否明确重试${target}？本次其他选择尚未受理。`);
      },
      reloadAfterMemoryNotice: async (degraded, isCurrent) => {
        if (degraded) window.alert("灾备台账写入失败，本次交接仅由单通道保护。确认此提示后将重载，并再次确认已冻结目标。");
        await new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
        if (isCurrent()) window.location.reload();
      },
      createHost: (setup, seed, target, onSpawnFailure, initialArchive, memoryAdmissionTicket, onInitializationProgress) => createSessionHost(setup, seed, target, options.TRADING_E2E_MODE, onSpawnFailure, initialArchive, memoryAdmissionTicket, onInitializationProgress),
      checkWasmEnvironment: () => assertWasmEnvironment(browserWasmEnvironment()),
      isDocumentHidden: () => document.hidden,
    });
    options.stopStartupRef.current = lifecycle.stopCurrentSession;
    if (!pausePreferencesReady || options.returningToStartupRef.current) return undefined;
    options.setHostBaselineReady(false);
    void lifecycle.start();
    return () => {
      selection.dispose();
      void lifecycle.dispose().catch((failure) => options.setError(`释放宿主失败：${failure instanceof Error ? failure.message : String(failure)}；请反馈此错误。`));
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionSetup, startupTarget, pausePreferencesReady, refreshPlayerOrders, setIndicatorCalculator]);
  return memoryHandoverPanel;
}
