import type { Dispatch, MutableRefObject, SetStateAction } from "react";
import type { EngineHost, SpeedMetrics } from "../host/engine-host.ts";
import type { HostFailure } from "../host/host-update.ts";
import type { SessionSetup, Snapshot } from "../types/engine.ts";
import type { AutoOrderManager } from "../components/auto-order-manager.ts";
import type { PlayerOrderRefreshGate } from "../components/player-orders.ts";
import type { DayEndFileTarget } from "../save/save-file.ts";
import type { StrictSaveEnvelope } from "../save/schema/root.ts";
import type { InitialSaveSource, SessionReplacementGate } from "../save/session-replacement.ts";
import { synchronizeCurrentBaseline } from "../save/session-replacement.ts";
import type { DayEndPersistence } from "../save/day-end-persistence.ts";
import { validateDayEndArchive } from "../save/day-end-candidate.ts";
import type { SpeedMetricsRequestGate } from "../host/speed.ts";
import { store, clearAutoOrders } from "../store/store.ts";
import { parseStartDate, setupWithStartDate } from "../components/start-date.ts";

export interface SaveCommandPorts {
  hostRef: MutableRefObject<EngineHost | null>;
  initialSaveSourceRef: MutableRefObject<InitialSaveSource<StrictSaveEnvelope>>;
  dayEndPersistenceRef: MutableRefObject<DayEndPersistence>;
  autoOrderMgrRef: MutableRefObject<AutoOrderManager | null>;
  sessionReplacementGateRef: MutableRefObject<SessionReplacementGate>;
  saveSelectionGenerationRef: MutableRefObject<number>;
  dayEndFileTargetRef: MutableRefObject<DayEndFileTarget | null>;
  playerOrderRefreshGateRef: MutableRefObject<PlayerOrderRefreshGate>;
  speedMetricsLoadInProgressRef: MutableRefObject<boolean>;
  speedMetricsRequestGateRef: MutableRefObject<SpeedMetricsRequestGate>;
  fatalHostErrorRef: MutableRefObject<(failure: string | HostFailure) => void>;
  activeSetup: SessionSetup;
  startDateDraft: string;
  priceCageEnabledDraft: boolean;
  loadFromFile(beforeRead?: () => Promise<void>): Promise<StrictSaveEnvelope | null>;
  selectDayEndFileTarget(): Promise<DayEndFileTarget | null>;
  getBrowserSaveRepository(): { load(): Promise<StrictSaveEnvelope | null> };
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
    speedMetricsRequestGateRef, fatalHostErrorRef, activeSetup, startDateDraft, priceCageEnabledDraft,
    loadFromFile, selectDayEndFileTarget, getBrowserSaveRepository, resetMarketHistory, configureMarketTiming, refreshPlayerOrders,
    clearPlayerOrders, setNotice, setError, setReady, setSessionSetup, setActiveSetup, setStartDateDraft,
    setPriceCageEnabledDraft, setStartDateError, setSpeedMetricsPollingGeneration, setSpeedMetrics,
    setSpeedMetricsError,
  } = ports;
  // 存档/读档
  async function recoverFromFile() {
    const recoveryGeneration = sessionReplacementGateRef.current.begin();
    if (recoveryGeneration === null) { setNotice("上一项读档或新局操作尚未结束，请稍后再试"); return; }
    try {
      const slot = await loadFromFile(() => dayEndPersistenceRef.current.beforeRead());
      if (!sessionReplacementGateRef.current.isCurrent(recoveryGeneration)) return;
      if (slot === null) { setNotice("已取消读档"); return; }
      validateDayEndArchive(slot);
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
    const loadGeneration = sessionReplacementGateRef.current.begin();
    if (loadGeneration === null) { setNotice("上一项读档或新局操作尚未结束，请稍后再试"); return; }
    saveSelectionGenerationRef.current += 1;
    const isCurrent = () => host === hostRef.current && sessionReplacementGateRef.current.isCurrent(loadGeneration);
    try {
      setNotice("正在读取日终快速存档，等待已提交的写入完成…");
      await dayEndPersistenceRef.current.beforeRead();
      if (!isCurrent()) return;
      const slot = await getBrowserSaveRepository().load();
      if (!isCurrent()) return;
      if (!slot) { setNotice("无存档"); return; }
      validateDayEndArchive(slot);
      dayEndPersistenceRef.current.invalidate();
      await dayEndPersistenceRef.current.idle();
      if (!isCurrent()) return;
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
      const result = await synchronizeCurrentBaseline(() => host.refreshBaseline(), isCurrent);
      if (!isCurrent() || result.kind === "stale") return;
      if (result.kind === "failed") { fatalHostErrorRef.current(`读档后权威基线同步失败：${result.error instanceof Error ? result.error.message : String(result.error)}`); return; }
      setNotice(`读档失败：${e}`);
    } finally {
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
      dayEndFileTargetRef.current = target;
      setNotice("已授权日终存档文件；仅在后续完整自然日日结更新，日内不写档。");
    } catch (e) {
      if (host === hostRef.current && selectionGeneration === saveSelectionGenerationRef.current) setNotice(`文件存档失败：${e}`);
    }
  }
  // 从文件读档
  async function handleLoadFile() {
    const host = hostRef.current;
    if (host === null) return;
    const loadGeneration = sessionReplacementGateRef.current.begin();
    if (loadGeneration === null) { setNotice("上一项读档或新局操作尚未结束，请稍后再试"); return; }
    saveSelectionGenerationRef.current += 1;
    const isCurrent = () => host === hostRef.current && sessionReplacementGateRef.current.isCurrent(loadGeneration);
    try {
      const slot = await loadFromFile(() => dayEndPersistenceRef.current.beforeRead());
      if (!isCurrent()) return;
      if (slot === null) { setNotice("已取消读档"); return; }
      validateDayEndArchive(slot);
      dayEndPersistenceRef.current.invalidate();
      await dayEndPersistenceRef.current.idle();
      if (!isCurrent()) return;
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
      const result = await synchronizeCurrentBaseline(() => host.refreshBaseline(), isCurrent);
      if (!isCurrent() || result.kind === "stale") return;
      if (result.kind === "failed") { fatalHostErrorRef.current(`文件读档后权威基线同步失败：${result.error instanceof Error ? result.error.message : String(result.error)}`); return; }
      setNotice(`文件读档失败：${e}`);
    } finally {
      sessionReplacementGateRef.current.finish(loadGeneration);
    }
  }

  async function handleNewGame() {
    const result = parseStartDate(startDateDraft);
    if (result.kind === "invalid") {
      setStartDateError(result.message);
      return;
    }
    setStartDateError(null);
    const newGameGeneration = sessionReplacementGateRef.current.begin();
    if (newGameGeneration === null) { setNotice("上一项读档或新局操作尚未结束，请稍后再试"); return; }
    try {
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
      });
      setNotice(`已按 ${result.value} 创建新模拟会话`);
    } finally {
      sessionReplacementGateRef.current.finish(newGameGeneration);
    }
  }
  return { recoverFromFile, noticeSavePolicy: handleSave, load: handleLoad, selectFile: handleSaveFile, loadFile: handleLoadFile, newGame: handleNewGame };
}

export function useSaveCommands(ports: SaveCommandPorts) { return createSaveCommands(ports); }
