import type { ChunkedArchiveRestore } from "./chunked-archive-restore.ts";
import { getInitialSaveValueConfiguration, isLeaseInitialSave, type InitialSaveValue } from "../save/initial-save-selection.ts";
import type { ValidatedArchiveLease } from "../save/validated-archive-lease.ts";
import type { Dispatch, MutableRefObject, SetStateAction } from "react";
import type { EngineHost, SpeedMetrics } from "../host/engine-host.ts";
import type { HostFailure } from "../host/host-update.ts";
import type { StartupTarget } from "../host/startup-policy.ts";
import type { FloatAllocation, SessionSetup, Snapshot } from "../types/engine.ts";
import type { AutoOrderManager } from "../components/auto-order-manager.ts";
import type { PlayerOrderRefreshGate } from "../components/player-orders.ts";
import type { DayEndFileTarget } from "../save/save-file.ts";
import type { StrictSaveEnvelope } from "../save/schema/root.ts";
import type { InitialSaveSource, SessionReplacementGate } from "../save/session-replacement.ts";
import { describeInitialSaveReadFailure, synchronizeCurrentBaseline } from "../save/session-replacement.ts";
import type { DayEndPersistence } from "../save/day-end-persistence.ts";
import { validateDayEndArchive } from "../save/day-end-candidate.ts";
import type { SpeedMetricsRequestGate } from "../host/speed.ts";
import { store, clearAutoOrders } from "../store/store.ts";
import { parseStartDate, setupWithStartDate } from "../components/start-date.ts";
import type { InitialAllocation } from "../host/initial-allocation.ts";
import type { AuthorizedDayEndFileTarget } from "./day-end-file-target.ts";

export interface SaveCommandPorts {
  startupTarget: StartupTarget;
  chunkedRestore?: ChunkedArchiveRestore;
  hostRef: MutableRefObject<EngineHost | null>;
  initialSaveSourceRef: MutableRefObject<InitialSaveSource<InitialSaveValue>>;
  dayEndPersistenceRef: MutableRefObject<DayEndPersistence>;
  autoOrderMgrRef: MutableRefObject<AutoOrderManager | null>;
  sessionReplacementGateRef: MutableRefObject<SessionReplacementGate>;
  saveSelectionGenerationRef: MutableRefObject<number>;
  dayEndFileTargetRef: MutableRefObject<AuthorizedDayEndFileTarget | null>;
  playerOrderRefreshGateRef: MutableRefObject<PlayerOrderRefreshGate>;
  speedMetricsLoadInProgressRef: MutableRefObject<boolean>;
  speedMetricsRequestGateRef: MutableRefObject<SpeedMetricsRequestGate>;
  fatalHostErrorRef: MutableRefObject<(failure: string | HostFailure) => void>;
  activeSetup: SessionSetup;
  startDateDraft: string;
  priceCageEnabledDraft: boolean;
  floatAllocationDraft: FloatAllocation;
  loadFromFile(beforeRead?: () => Promise<void>): Promise<StrictSaveEnvelope | null>;
  selectDayEndFileTarget(): Promise<DayEndFileTarget | null>;
  getBrowserSaveRepository(): { load(): Promise<StrictSaveEnvelope | null>; loadFrozen(): Promise<StrictSaveEnvelope | null>; loadValidatedLease?(signal?: AbortSignal): Promise<ValidatedArchiveLease | null> };
  resetMarketHistory(snapshot: Snapshot): void;
  configureMarketTiming(setup: SessionSetup): void;
  refreshPlayerOrders(): Promise<void>;
  clearPlayerOrders(): void;
  setNotice(notice: string): void;
  setError(error: string | HostFailure | null): void;
  setReady(ready: boolean): void;
  setSessionSetup: Dispatch<SetStateAction<SessionSetup>>;
  setActiveSetup(setup: SessionSetup): void;
  setStartDateDraft(date: string): void;
  setPriceCageEnabledDraft(enabled: boolean): void;
  setFloatAllocationDraft(allocation: FloatAllocation): void;
  setInitialAllocation(allocation: InitialAllocation | null): void;
  setStartDateError(error: string | null): void;
  setSpeedMetricsPollingGeneration(generation: number): void;
  setSpeedMetrics(metrics: SpeedMetrics | null): void;
  setSpeedMetricsError(error: string | null): void;
}

/** 六个存档用例只借用已有 owner，不持有额外存档或会话状态。 */
export function createSaveCommands(ports: SaveCommandPorts) {
  const {
    hostRef, initialSaveSourceRef, dayEndPersistenceRef, autoOrderMgrRef, sessionReplacementGateRef,
    saveSelectionGenerationRef, dayEndFileTargetRef, playerOrderRefreshGateRef, speedMetricsLoadInProgressRef,
    speedMetricsRequestGateRef, fatalHostErrorRef, activeSetup, startDateDraft, priceCageEnabledDraft, floatAllocationDraft,
    loadFromFile, selectDayEndFileTarget, getBrowserSaveRepository, resetMarketHistory, configureMarketTiming, refreshPlayerOrders,
    clearPlayerOrders, setNotice, setError, setReady, setSessionSetup, setActiveSetup, setStartDateDraft,
    setPriceCageEnabledDraft, setFloatAllocationDraft, setInitialAllocation, setStartDateError, setSpeedMetricsPollingGeneration, setSpeedMetrics,
    setSpeedMetricsError,
  } = ports;
  const selectionAvailable = () => {
    try { initialSaveSourceRef.current.assertSelectable(); return true; }
    catch (error) { setNotice(error instanceof Error ? error.message : String(error)); return false; }
  };
  const requestWasmReplacement = (slot: InitialSaveValue) => {
    const configuration = getInitialSaveValueConfiguration(slot);
    initialSaveSourceRef.current.select(slot);
    playerOrderRefreshGateRef.current.invalidate();
    speedMetricsRequestGateRef.current.invalidate();
    setSpeedMetricsPollingGeneration(speedMetricsRequestGateRef.current.capture());
    setSpeedMetrics(null);
    setSpeedMetricsError(null);
    autoOrderMgrRef.current?.clear();
    store.dispatch(clearAutoOrders());
    clearPlayerOrders();
    setError(null);
    setReady(false);
    setSessionSetup({ ...configuration.setup });
    setNotice("已选择日终存档，正在结束旧实例并恢复；新实例就绪后才确认读档成功。");
  };
  // 存档/读档
  async function recoverFromFile() {
    if (!selectionAvailable()) return;
    const recoveryGeneration = sessionReplacementGateRef.current.begin();
    if (recoveryGeneration === null) { setNotice("上一项读档或新局操作尚未结束，请稍后再试"); return; }
    let releaseSelectionChange: (() => void) | undefined;
    try {
      const slot = await loadFromFile(() => dayEndPersistenceRef.current.beforeRead());
      if (!sessionReplacementGateRef.current.isCurrent(recoveryGeneration)) return;
      if (slot === null) { setNotice("已取消读档"); return; }
      validateDayEndArchive(slot);
      releaseSelectionChange = initialSaveSourceRef.current.beginSelectionChange();
      dayEndPersistenceRef.current.invalidate();
      saveSelectionGenerationRef.current += 1;
      await dayEndPersistenceRef.current.idle();
      if (!sessionReplacementGateRef.current.isCurrent(recoveryGeneration)) return;
      initialSaveSourceRef.current.select(slot);
      autoOrderMgrRef.current?.clear();
      store.dispatch(clearAutoOrders());
      clearPlayerOrders();
      setError(null);
      setReady(false);
      setSessionSetup({ ...slot.setup });
    } catch (recoveryError) {
      if (sessionReplacementGateRef.current.isCurrent(recoveryGeneration)) {
        setNotice(`选择日终存档失败：${recoveryError instanceof Error ? recoveryError.message : String(recoveryError)}`);
      }
    } finally {
      releaseSelectionChange?.();
      sessionReplacementGateRef.current.finish(recoveryGeneration);
    }
  }

  async function handleSave() {
    if (!hostRef.current) return;
    setNotice("已启用日终自动存档；日内不写档，首次完整自然日日结前没有可用日终存档。");
  }
  async function handleLoad() {
    const host = hostRef.current;
    if (host === null) return;
    if (!selectionAvailable()) return;
    const loadGeneration = sessionReplacementGateRef.current.begin();
    if (loadGeneration === null) { setNotice("上一项读档或新局操作尚未结束，请稍后再试"); return; }
    saveSelectionGenerationRef.current += 1;
    const isCurrent = () => host === hostRef.current && sessionReplacementGateRef.current.isCurrent(loadGeneration);
    let leaseReadSignal: AbortSignal | undefined;
    let releaseSelectionChange: (() => void) | undefined;
    try {
      setNotice("正在读取日终快速存档，等待已提交的写入完成…");
      await dayEndPersistenceRef.current.beforeRead();
      if (!isCurrent()) return;
      const repository = getBrowserSaveRepository();
      let slot: InitialSaveValue | null;
      if (ports.startupTarget.kind === "wasm" && ports.chunkedRestore !== undefined) {
        if (repository.loadValidatedLease === undefined) throw new Error("已配置分块恢复能力，但 repository 缺少显式租约读取入口");
        const loadLease = repository.loadValidatedLease;
        const lease = await initialSaveSourceRef.current.runSelectionRead(signal => {
          leaseReadSignal = signal;
          return loadLease.call(repository, signal);
        });
        slot = lease === null ? null : { kind: "lease", lease };
      } else slot = await (ports.startupTarget.kind === "wasm" ? repository.loadFrozen() : repository.load());
      if (!isCurrent()) return;
      if (slot === null) { setNotice("无存档"); return; }
      if (isLeaseInitialSave(slot)) getInitialSaveValueConfiguration(slot);
      else validateDayEndArchive(slot);
      releaseSelectionChange = initialSaveSourceRef.current.beginSelectionChange();
      dayEndPersistenceRef.current.invalidate();
      await dayEndPersistenceRef.current.idle();
      if (!isCurrent()) return;
      if (ports.startupTarget.kind === "wasm") { requestWasmReplacement(slot); return; }
      if (isLeaseInitialSave(slot)) throw new Error("远程宿主不支持分块租约，不能回退为 String 存档");
      playerOrderRefreshGateRef.current.invalidate();
      speedMetricsLoadInProgressRef.current = true;
      speedMetricsRequestGateRef.current.invalidate();
      setSpeedMetricsPollingGeneration(speedMetricsRequestGateRef.current.capture());
      setSpeedMetrics(null);
      setSpeedMetricsError(null);
      let restored = false;
      const installRestoredSetup = () => {
        if (restored || !isCurrent()) return;
        configureMarketTiming(slot.setup);
        setActiveSetup(slot.setup);
        setStartDateDraft(slot.setup.start_date);
        setPriceCageEnabledDraft(slot.setup.config.price_cage_enabled);
        setFloatAllocationDraft(slot.setup.float_allocation);
        setInitialAllocation(null);
        restored = true;
      };
      try {
        await host.load(slot, installRestoredSetup);
      } finally {
        speedMetricsLoadInProgressRef.current = false;
        speedMetricsRequestGateRef.current.invalidate();
        setSpeedMetricsPollingGeneration(speedMetricsRequestGateRef.current.capture());
      }
      if (!isCurrent()) return;
      // 成功返回也是恢复确认；幂等安装避免重复改写图表或 setup。
      installRestoredSetup();
      const loadedSnapshot = host.snapshot();
      resetMarketHistory(loadedSnapshot);
      playerOrderRefreshGateRef.current.invalidate();
      clearPlayerOrders();
      void refreshPlayerOrders();
      autoOrderMgrRef.current?.clear();
      store.dispatch(clearAutoOrders());
      setNotice(`已读档（第 ${host.day() + 1} 个交易日）`);
    } catch (e) {
      if (ports.startupTarget.kind === "wasm") {
        if (isCurrent()) setNotice(`读取日终存档失败：${describeInitialSaveReadFailure(e)}`);
        else if (leaseReadSignal !== undefined && e !== leaseReadSignal.reason) {
          setNotice(`过期存档读取或清理失败：${describeInitialSaveReadFailure(e)}；请反馈此错误。`);
        }
        return;
      }
      const result = await synchronizeCurrentBaseline(() => host.refreshBaseline(), isCurrent);
      if (!isCurrent() || result.kind === "stale") return;
      if (result.kind === "failed") { fatalHostErrorRef.current(`读档后权威基线同步失败：${result.error instanceof Error ? result.error.message : String(result.error)}`); return; }
      setNotice(`读档失败：${e}`);
    } finally {
      releaseSelectionChange?.();
      sessionReplacementGateRef.current.finish(loadGeneration);
    }
  }

  // 选择可复用日终文件目标；不在选择时写档，也不以下载冒充文件覆盖。
  async function handleSaveFile() {
    const host = hostRef.current;
    if (host === null) return;
    const selectionGeneration = ++saveSelectionGenerationRef.current;
    try {
      const target = await selectDayEndFileTarget();
      if (host !== hostRef.current || selectionGeneration !== saveSelectionGenerationRef.current) return;
      if (target === null) { setNotice("已取消选择日终存档文件"); return; }
      if (ports.startupTarget.kind === "remote") {
        if (host.subscribeDayEndExports === undefined) throw new Error("远程宿主不支持日终文件订阅，请检查 Server 与 WebUI 版本");
        const previous = dayEndFileTargetRef.current;
        dayEndFileTargetRef.current = null;
        if (previous?.remoteExport !== undefined) await previous.remoteExport.close();
        if (host !== hostRef.current || selectionGeneration !== saveSelectionGenerationRef.current) return;
        let authorized: AuthorizedDayEndFileTarget | null = null;
        let subscriptionFailure: Error | null = null;
        const remoteExport = await host.subscribeDayEndExports(error => {
          subscriptionFailure = error;
          if (authorized !== null && dayEndFileTargetRef.current === authorized) {
            dayEndFileTargetRef.current = null;
            if (host === hostRef.current) setNotice(`日终文件导出已取消：${error.message}；请重新授权文件。`);
          }
        });
        if (host !== hostRef.current || selectionGeneration !== saveSelectionGenerationRef.current) { await remoteExport.close(); return; }
        if (subscriptionFailure !== null) { await remoteExport.close(); throw subscriptionFailure; }
        authorized = { write: (slot, current) => target.write(slot, current), remoteExport };
        dayEndFileTargetRef.current = authorized;
      } else {
        dayEndFileTargetRef.current = target;
      }
      setNotice("已授权日终存档文件；仅在后续完整自然日日结更新，日内不写档。");
    } catch (e) {
      if (host === hostRef.current && selectionGeneration === saveSelectionGenerationRef.current) setNotice(`文件存档失败：${e}`);
    }
  }
  // 从文件读档
  async function handleLoadFile() {
    const host = hostRef.current;
    if (host === null) return;
    if (!selectionAvailable()) return;
    const loadGeneration = sessionReplacementGateRef.current.begin();
    if (loadGeneration === null) { setNotice("上一项读档或新局操作尚未结束，请稍后再试"); return; }
    saveSelectionGenerationRef.current += 1;
    const isCurrent = () => host === hostRef.current && sessionReplacementGateRef.current.isCurrent(loadGeneration);
    let releaseSelectionChange: (() => void) | undefined;
    try {
      const slot = await loadFromFile(() => dayEndPersistenceRef.current.beforeRead());
      if (!isCurrent()) return;
      if (slot === null) { setNotice("已取消读档"); return; }
      validateDayEndArchive(slot);
      releaseSelectionChange = initialSaveSourceRef.current.beginSelectionChange();
      dayEndPersistenceRef.current.invalidate();
      await dayEndPersistenceRef.current.idle();
      if (!isCurrent()) return;
      if (ports.startupTarget.kind === "wasm") { requestWasmReplacement(slot); return; }
      playerOrderRefreshGateRef.current.invalidate();
      speedMetricsLoadInProgressRef.current = true;
      speedMetricsRequestGateRef.current.invalidate();
      setSpeedMetricsPollingGeneration(speedMetricsRequestGateRef.current.capture());
      setSpeedMetrics(null);
      setSpeedMetricsError(null);
      let restored = false;
      const installRestoredSetup = () => {
        if (restored || !isCurrent()) return;
        configureMarketTiming(slot.setup);
        setActiveSetup(slot.setup);
        setStartDateDraft(slot.setup.start_date);
        setPriceCageEnabledDraft(slot.setup.config.price_cage_enabled);
        setFloatAllocationDraft(slot.setup.float_allocation);
        setInitialAllocation(null);
        restored = true;
      };
      try {
        await host.load(slot, installRestoredSetup);
      } finally {
        speedMetricsLoadInProgressRef.current = false;
        speedMetricsRequestGateRef.current.invalidate();
        setSpeedMetricsPollingGeneration(speedMetricsRequestGateRef.current.capture());
      }
      if (!isCurrent()) return;
      // 成功返回也是恢复确认；幂等安装避免重复改写图表或 setup。
      installRestoredSetup();
      const loadedSnapshot = host.snapshot();
      resetMarketHistory(loadedSnapshot);
      playerOrderRefreshGateRef.current.invalidate();
      clearPlayerOrders();
      void refreshPlayerOrders();
      autoOrderMgrRef.current?.clear();
      store.dispatch(clearAutoOrders());
      setNotice(`已从文件读档（第 ${host.day() + 1} 个交易日）`);
    } catch (e) {
      if (ports.startupTarget.kind === "wasm") {
        if (isCurrent()) setNotice(`读取日终存档失败：${e instanceof Error ? e.message : String(e)}`);
        return;
      }
      const result = await synchronizeCurrentBaseline(() => host.refreshBaseline(), isCurrent);
      if (!isCurrent() || result.kind === "stale") return;
      if (result.kind === "failed") { fatalHostErrorRef.current(`文件读档后权威基线同步失败：${result.error instanceof Error ? result.error.message : String(result.error)}`); return; }
      setNotice(`文件读档失败：${e}`);
    } finally {
      releaseSelectionChange?.();
      sessionReplacementGateRef.current.finish(loadGeneration);
    }
  }

  async function handleNewGame() {
    if (!selectionAvailable()) return;
    const result = parseStartDate(startDateDraft);
    if (result.kind === "invalid") {
      setStartDateError(result.message);
      return;
    }
    setStartDateError(null);
    const newGameGeneration = sessionReplacementGateRef.current.begin();
    if (newGameGeneration === null) { setNotice("上一项读档或新局操作尚未结束，请稍后再试"); return; }
    let releaseSelectionChange: (() => void) | undefined;
    try {
      releaseSelectionChange = initialSaveSourceRef.current.beginSelectionChange();
      dayEndPersistenceRef.current.invalidate();
      saveSelectionGenerationRef.current += 1;
      await dayEndPersistenceRef.current.idle();
      if (!sessionReplacementGateRef.current.isCurrent(newGameGeneration)) return;
      initialSaveSourceRef.current.reset();
      autoOrderMgrRef.current?.clear();
      store.dispatch(clearAutoOrders());
      clearPlayerOrders();
      setError(null);
      setReady(false);
      setSessionSetup({
        ...setupWithStartDate(activeSetup, result.value),
        config: { ...activeSetup.config, price_cage_enabled: priceCageEnabledDraft },
        float_allocation: floatAllocationDraft,
      });
      setNotice(`已按 ${result.value} 创建新模拟会话`);
    } catch (error) {
      setNotice(`创建新会话失败：${error instanceof Error ? error.message : String(error)}`);
    } finally {
      releaseSelectionChange?.();
      sessionReplacementGateRef.current.finish(newGameGeneration);
    }
  }
  return { recoverFromFile, noticeSavePolicy: handleSave, load: handleLoad, selectFile: handleSaveFile, loadFile: handleLoadFile, newGame: handleNewGame };
}

export function useSaveCommands(ports: SaveCommandPorts) { return createSaveCommands(ports); }
