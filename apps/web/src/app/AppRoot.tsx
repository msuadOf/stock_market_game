import { useCallback, useRef, useState, type Dispatch, type MutableRefObject, type ReactElement, type SetStateAction } from "react";
import { DEFAULT_SETUP } from "../config/defaults";
import type { AutoOrderManager } from "../components/auto-order-manager.ts";
import type { EngineHost } from "../host/engine-host.ts";
import type { HostFailure } from "../host/host-update.ts";
import { browserWasmEnvironment, initialStartupTarget, resolveStartupTarget, type StartupMode, type StartupTarget } from "../host/startup-policy.ts";
import { DayEndPersistence } from "../save/day-end-persistence.ts";
import { InitialSaveSource } from "../save/session-replacement.ts";
import type { InitialSaveValue } from "../save/initial-save-selection.ts";
import { clearAutoOrders, setRunning, store } from "../store/store.ts";
import type { SessionSetup } from "../types/engine.ts";
import { HostSpawnFailureReport } from "./HostStatusViews.tsx";
import { TRADING_E2E_MODE } from "./runtime-mode.ts";
import { StartupScreen } from "./StartupScreen.tsx";

const INITIAL_SESSION_SETUP: SessionSetup = TRADING_E2E_MODE
  ? {
      ...DEFAULT_SETUP,
      npcs: { ...DEFAULT_SETUP.npcs, retail_count: 0, inst_count: 0, hot_count: 0 },
      float_allocation: { between_kinds: "Random", within_kind: "Random" },
      ticks_per_day: 30,
      auction_ticks: 9,
      closing_auction_ticks: 3,
    }
  : DEFAULT_SETUP;

export interface AppRootShellProps {
  startupTarget: StartupTarget;
  initialSaveSourceRef: MutableRefObject<InitialSaveSource<InitialSaveValue>>;
  dayEndPersistenceRef: MutableRefObject<DayEndPersistence>;
  sessionSetup: SessionSetup;
  setSessionSetup: Dispatch<SetStateAction<SessionSetup>>;
  returningToStartup: boolean;
  returningToStartupRef: MutableRefObject<boolean>;
  startupReturnError: string | null;
  onSpawnFailure: (failure: HostFailure) => void;
  onSelectHost: (stopSession: () => Promise<void>) => Promise<void>;
  autoOrderMgrRef: MutableRefObject<AutoOrderManager | null>;
  hostRef: MutableRefObject<EngineHost | null>;
  notice: string | null;
  setNotice: Dispatch<SetStateAction<string | null>>;
}

/** 启动选择与重选状态机；游戏 Shell 通过 renderShell 注入，避免外层测试装载整棵 UI 模块图。 */
export function AppRoot({ renderShell }: { renderShell: (props: AppRootShellProps) => ReactElement }) {
  const [spawnFailures, setSpawnFailures] = useState<readonly HostFailure[]>([]);
  const onSpawnFailure = useCallback((failure: HostFailure) => {
    setSpawnFailures((visible) => [...visible, failure]);
  }, []);
  const dismissSpawnFailures = useCallback(() => setSpawnFailures([]), []);
  const initialSaveSourceRef = useRef(new InitialSaveSource<InitialSaveValue>());
  const dayEndPersistenceRef = useRef(new DayEndPersistence());
  const [sessionSetup, setSessionSetup] = useState<SessionSetup>(INITIAL_SESSION_SETUP);
  const returningToStartupRef = useRef(false);
  const [returningToStartup, setReturningToStartup] = useState(false);
  const [startupReturnError, setStartupReturnError] = useState<string | null>(null);
  const [startupTarget, setStartupTarget] = useState<StartupTarget | null>(() => initialStartupTarget(import.meta.env.MODE));
  const [startupMode, setStartupMode] = useState<StartupMode>(import.meta.env.DEV && import.meta.env.VITE_ENGINE_HOST === "remote" ? "remote" : "local");
  const [remoteAddress, setRemoteAddress] = useState(import.meta.env.DEV ? import.meta.env.VITE_REMOTE_BASE_URL ?? "" : "");
  const [startupError, setStartupError] = useState<string | null>(null);
  const autoOrderMgrRef = useRef<AutoOrderManager | null>(null);
  const hostRef = useRef<EngineHost | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const returnToStartup = useCallback(async (stopSession: () => Promise<void>) => {
    if (returningToStartupRef.current) return;
    returningToStartupRef.current = true;
    setReturningToStartup(true);
    setStartupReturnError(null);
    try {
      await stopSession();
      store.dispatch(setRunning(false));
      dayEndPersistenceRef.current.invalidate();
      await dayEndPersistenceRef.current.idle();
      store.dispatch(clearAutoOrders());
      setNotice(null);
      setStartupTarget(null);
    } catch (failure) {
      setStartupReturnError(`返回启动选择失败：${failure instanceof Error ? failure.message : String(failure)}；尚未开始下一局，请反馈此错误或重新打开应用。`);
    } finally {
      returningToStartupRef.current = false;
      setReturningToStartup(false);
    }
  }, []);
  if (startupTarget === null) {
    return <HostSpawnFailureReport failures={spawnFailures} onDismiss={dismissSpawnFailures}><StartupScreen mode={startupMode} remoteAddress={remoteAddress} error={startupError}
      developmentHint={import.meta.env.DEV && (import.meta.env.VITE_ENGINE_HOST !== undefined || import.meta.env.VITE_REMOTE_BASE_URL !== undefined)}
      onModeChange={(mode) => { setStartupMode(mode); setStartupError(null); }}
      onAddressChange={(address) => { setRemoteAddress(address); setStartupError(null); }}
      onStart={() => {
        try {
          const target = resolveStartupTarget(startupMode, remoteAddress, "__TAURI_INTERNALS__" in window, browserWasmEnvironment);
          setStartupError(null);
          setStartupTarget(target);
        } catch (startupFailure) {
          setStartupError(startupFailure instanceof Error ? startupFailure.message : String(startupFailure));
        }
      }} /></HostSpawnFailureReport>;
  }
  return (
    <HostSpawnFailureReport failures={spawnFailures} onDismiss={dismissSpawnFailures}>
      {renderShell({
        onSpawnFailure, startupTarget, initialSaveSourceRef, onSelectHost: returnToStartup,
        dayEndPersistenceRef, sessionSetup, setSessionSetup,
        returningToStartup, returningToStartupRef, startupReturnError,
        autoOrderMgrRef, hostRef, notice, setNotice,
      })}
    </HostSpawnFailureReport>
  );
}
