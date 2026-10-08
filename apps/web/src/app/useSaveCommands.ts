import type { MutableRefObject } from "react";
import type { EngineHost, SpeedMetrics } from "../host/engine-host.ts";
import type { HostFailure } from "../host/host-update.ts";
import type { FloatAllocation, SessionSetup, Snapshot } from "../types/engine.ts";
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
import type { InitialAllocation } from "../host/initial-allocation.ts";
import type { BrowserArchiveStore } from "../save/archive-store.ts";
import type { ReportFrequencyDraft } from "../components/ReportFrequencyInput.tsx";
import { parseReportFrequency } from "../save/schema/report-frequency.ts";
import { parseSessionSeed } from "../config/seed-draft.ts";
import { parseCompanySystemConfig } from "../save/schema/company/system-config.ts";
import { DEFAULT_SETUP } from "../config/defaults.ts";

export interface SaveCommandPorts {
  browserLocal: boolean;
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
  floatAllocationDraft: FloatAllocation;
  reportFrequencyDraft: ReportFrequencyDraft;
  companySystemDraft: string;
  dividendTaxModeDraft: SessionSetup["dividend_tax_mode"];
  flatWithholdingBpDraft: number;
  rightsOfferingEnabledDraft: boolean;
  issuerRepurchaseEnabledDraft: boolean;
  parValuePerShareDraft: string;
  seedDraft: string;
  loadFromFile(beforeRead?: () => Promise<void>): Promise<StrictSaveEnvelope | null>;
  selectDayEndFileTarget(): Promise<DayEndFileTarget | null>;
  getBrowserSaveRepository(): Pick<BrowserArchiveStore, "load" | "select" | "cancelPending" | "newSlot">;
  resetMarketHistory(snapshot: Snapshot): void;
  configureMarketTiming(setup: SessionSetup): void;
  refreshPlayerOrders(): Promise<void>;
  clearPlayerOrders(): void;
  setNotice(notice: string): void;
  setError(error: string | HostFailure | null): void;
  setReady(ready: boolean): void;
  setSessionCreation(setup: SessionSetup, seed: string): void;
  setSeedDraft(seed: string): void;
  setActiveSetup(setup: SessionSetup): void;
  setStartDateDraft(date: string): void;
  setPriceCageEnabledDraft(enabled: boolean): void;
  setFloatAllocationDraft(allocation: FloatAllocation): void;
  setReportFrequencyDraft(frequency: SessionSetup["report_frequency"]): void;
  setCompanySystemDraft(config: string): void;
  setDividendTaxModeDraft(mode: SessionSetup["dividend_tax_mode"]): void;
  setFlatWithholdingBpDraft(rateBp: number): void;
  setRightsOfferingEnabledDraft(enabled: boolean): void;
  setIssuerRepurchaseEnabledDraft(enabled: boolean): void;
  setParValuePerShareDraft(cents: string): void;
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
    speedMetricsRequestGateRef, fatalHostErrorRef, activeSetup, startDateDraft, priceCageEnabledDraft, floatAllocationDraft, reportFrequencyDraft, companySystemDraft, dividendTaxModeDraft, flatWithholdingBpDraft, rightsOfferingEnabledDraft, issuerRepurchaseEnabledDraft, parValuePerShareDraft,
    loadFromFile, selectDayEndFileTarget, getBrowserSaveRepository, resetMarketHistory, configureMarketTiming, refreshPlayerOrders,
    clearPlayerOrders, setNotice, setError, setReady, setSessionCreation, setActiveSetup, setStartDateDraft,
    setPriceCageEnabledDraft, setFloatAllocationDraft, setReportFrequencyDraft, setCompanySystemDraft, setDividendTaxModeDraft, setFlatWithholdingBpDraft, setRightsOfferingEnabledDraft, setIssuerRepurchaseEnabledDraft, setParValuePerShareDraft, setInitialAllocation, setStartDateError, setSpeedMetricsPollingGeneration, setSpeedMetrics,
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
      if (ports.browserLocal) getBrowserSaveRepository().cancelPending();
      saveSelectionGenerationRef.current += 1;
      await dayEndPersistenceRef.current.idle();
      if (!sessionReplacementGateRef.current.isCurrent(recoveryGeneration)) return;
      if (ports.browserLocal) getBrowserSaveRepository().newSlot();
      initialSaveSourceRef.current.select(slot);
      autoOrderMgrRef.current?.clear();
      store.dispatch(clearAutoOrders());
      clearPlayerOrders();
      setError(null);
      setReady(false);
      setSessionCreation({ ...slot.setup }, slot.seed);
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
  async function handleLoad(slotId?: string) {
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
      const repository = host.capabilities.persistence === "browser" ? getBrowserSaveRepository() : host.archiveStore;
      if (!repository) throw new Error("宿主未提供存档管理接口，请反馈错误");
      const slot = await repository.load(slotId);
      if (!isCurrent()) return;
      if (!slot) { setNotice("无存档"); return; }
      validateDayEndArchive(slot);
      dayEndPersistenceRef.current.invalidate();
      if (host.capabilities.persistence === "browser") getBrowserSaveRepository().cancelPending();
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
        setFloatAllocationDraft(slot.setup.float_allocation);
        setReportFrequencyDraft(slot.setup.report_frequency);
        setCompanySystemDraft(JSON.stringify(slot.setup.company_system, null, 2));
        setDividendTaxModeDraft(slot.setup.dividend_tax_mode);
        setFlatWithholdingBpDraft(slot.setup.flat_withholding_bp ?? 1000);
        setParValuePerShareDraft(slot.setup.par_value_per_share);
        setRightsOfferingEnabledDraft(slot.setup.rights_offering_enabled);
        setIssuerRepurchaseEnabledDraft(slot.setup.issuer_repurchase_enabled);
        ports.setSeedDraft(slot.seed);
        setInitialAllocation(null);
        restored = true;
      };
      try {
        await host.load(slot, slotId, installRestoredSetup);
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
      if (slotId !== undefined) {
        try {
          const selected = await repository.select(slotId, isCurrent);
          if (!isCurrent() || !selected) return;
        } catch (selectionError) {
          if (isCurrent()) setNotice(`市场已加载；启动槽选择保存失败：${selectionError instanceof Error ? selectionError.message : String(selectionError)}。原启动槽保持不变，当前局及下一日终写入仍使用已加载槽。请检查存储权限并反馈错误。`);
          return;
        }
      }
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
      if (host.capabilities.persistence === "browser") getBrowserSaveRepository().cancelPending();
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
        setFloatAllocationDraft(slot.setup.float_allocation);
        setReportFrequencyDraft(slot.setup.report_frequency);
        setCompanySystemDraft(JSON.stringify(slot.setup.company_system, null, 2));
        setDividendTaxModeDraft(slot.setup.dividend_tax_mode);
        setFlatWithholdingBpDraft(slot.setup.flat_withholding_bp ?? 1000);
        setParValuePerShareDraft(slot.setup.par_value_per_share);
        ports.setSeedDraft(slot.seed);
        setInitialAllocation(null);
        restored = true;
      };
      try {
        await host.load(slot, undefined, installRestoredSetup);
        if (host.capabilities.persistence === "browser") getBrowserSaveRepository().newSlot();
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
    let seed: bigint;
    try { seed = parseSessionSeed(ports.seedDraft); } catch (failure) {
      setNotice(`新局 seed 无效：${failure instanceof Error ? failure.message : String(failure)}；请修正后再创建新游戏。`);
      return;
    }
    let companySystem: SessionSetup["company_system"];
    try {
      companySystem = parseCompanySystemConfig(JSON.parse(companySystemDraft));
    } catch (failure) {
      setNotice(`公司基本面设置无效：${failure instanceof Error ? failure.message : String(failure)}；请修正后再创建新游戏。`);
      return;
    }
    let reportFrequency: SessionSetup["report_frequency"];
    try {
      reportFrequency = parseReportFrequency(reportFrequencyDraft);
    } catch (failure) {
      setNotice(`财报公开设置无效：${failure instanceof Error ? failure.message : String(failure)}`);
      return;
    }
    const newGameGeneration = sessionReplacementGateRef.current.begin();
    if (newGameGeneration === null) { setNotice("上一项读档或新局操作尚未结束，请稍后再试"); return; }
    try {
      const nextSetup = {
        ...setupWithStartDate(activeSetup, result.value),
        config: {
          ...activeSetup.config,
          price_cage_enabled: priceCageEnabledDraft,
          // 印花税按模式门禁（2026-10-08 决策）：不扣税模式连印花税也免；
          // 其余模式保持正式 A 股基线（引擎 setup 校验强制该配对）。
          stamp_tax_rate: dividendTaxModeDraft === "Exempt" ? 0 : DEFAULT_SETUP.config.stamp_tax_rate,
        },
        float_allocation: floatAllocationDraft,
        report_frequency: reportFrequency,
        company_system: companySystem,
        dividend_tax_mode: dividendTaxModeDraft,
        flat_withholding_bp: dividendTaxModeDraft === "FlatWithholding" ? flatWithholdingBpDraft : null,
        rights_offering_enabled: rightsOfferingEnabledDraft,
        issuer_repurchase_enabled: issuerRepurchaseEnabledDraft,
        par_value_per_share: parValuePerShareDraft,
        // 开局自动装配随新局显式回产品默认开（独立复核 minor-3 修复）：
        // {...activeSetup} 展开会把读入档的 auto=false 静默带进新局——该开关
        // 无 UI 草稿层，跨局粘滞不可见；新局创建界面无关闭入口，统一回 true。
        // 读档仍按档内值恢复（restore 路不经此组装）。
        auto_corporate_foundation: true,
      };
      const host = hostRef.current;
      if (host?.capabilities.persistence === "remote") {
        if (host.resetMarket === undefined) throw new Error("远程宿主没有共享市场重置接口");
        await host.resetMarket(nextSetup, seed);
        if (host !== hostRef.current || !sessionReplacementGateRef.current.isCurrent(newGameGeneration)) return;
        autoOrderMgrRef.current?.clear();
        store.dispatch(clearAutoOrders());
        clearPlayerOrders();
        setInitialAllocation(null);
        setNotice(`已按 ${result.value} 重置全市场；所有玩家切换至同一时间线，登录身份和控制授权不从存档恢复。`);
        return;
      }
      dayEndPersistenceRef.current.invalidate();
      if (ports.browserLocal) getBrowserSaveRepository().cancelPending();
      saveSelectionGenerationRef.current += 1;
      await dayEndPersistenceRef.current.idle();
      if (!sessionReplacementGateRef.current.isCurrent(newGameGeneration)) return;
      if (ports.browserLocal) getBrowserSaveRepository().newSlot();
      initialSaveSourceRef.current.reset();
      autoOrderMgrRef.current?.clear();
      store.dispatch(clearAutoOrders());
      clearPlayerOrders();
      setError(null);
      setReady(false);
      setSessionCreation(nextSetup, seed.toString());
      setNotice(`已按 ${result.value} 创建新模拟会话`);
    } catch (failure) {
      setNotice(`新游戏失败：${failure instanceof Error ? failure.message : String(failure)}；请反馈此错误。`);
    } finally {
      sessionReplacementGateRef.current.finish(newGameGeneration);
    }
  }
  return { recoverFromFile, noticeSavePolicy: handleSave, load: handleLoad, selectFile: handleSaveFile, loadFile: handleLoadFile, newGame: handleNewGame };
}

export function useSaveCommands(ports: SaveCommandPorts) { return createSaveCommands(ports); }
