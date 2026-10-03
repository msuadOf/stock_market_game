import { useEffect, useRef, type MutableRefObject } from "react";
import type { EngineHost } from "../host/engine-host.ts";
import type { PausePreferences } from "../types/generated/PausePreferences.ts";
import { loadPausePreferences, savePausePreferences } from "../config/pause-preferences.ts";

interface Storage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}
type StorageSource = Storage | null | (() => Storage | null);
const readStorage = (source: StorageSource) => typeof source === "function" ? source() : source;

interface Ports {
  apply(preferences: PausePreferences): void;
  onReady(): void;
  onError(error: string): void;
}

/** 只拥有首次读取进度；偏好事实始终由 Redux settings 提供。 */
export function createPausePreferencesLifecycle(ports: Ports) {
  let loaded = false;
  const report = (error: unknown) => ports.onError(error instanceof Error ? error.message : String(error));
  return {
    loadOnce(source: StorageSource) {
      if (loaded) return;
      try {
        const storage = readStorage(source);
        if (storage === null) return;
        const preferences = loadPausePreferences(storage);
        ports.apply(preferences);
        loaded = true;
        ports.onReady();
      } catch (preferenceError) { report(preferenceError); }
    },
    synchronize(host: Pick<EngineHost, "setPausePreferences"> | null, preferences: PausePreferences, source: StorageSource, skip: boolean) {
      if (skip || host === null) return;
      void host.setPausePreferences(preferences).catch(report);
      if (loaded) {
        try {
          const storage = readStorage(source);
          if (storage !== null) savePausePreferences(storage, preferences);
        } catch (preferenceError) { report(preferenceError); }
      }
    },
  };
}

interface Options extends Ports {
  hostRef: MutableRefObject<EngineHost | null>;
  pauseAfterClose: boolean;
  pauseBeforeOpen: boolean;
  tradingE2EMode: boolean;
}

export function usePausePreferences(options: Options): void {
  const lifecycleRef = useRef<ReturnType<typeof createPausePreferencesLifecycle> | null>(null);
  if (lifecycleRef.current === null) lifecycleRef.current = createPausePreferencesLifecycle(options);
  useEffect(() => {
    lifecycleRef.current!.loadOnce(() => typeof window === "undefined" ? null : window.sessionStorage);
  }, []);
  const { pauseAfterClose, pauseBeforeOpen } = options;
  useEffect(() => {
    lifecycleRef.current!.synchronize(options.hostRef.current, { pause_after_close: pauseAfterClose, pause_before_open: pauseBeforeOpen },
      () => typeof window === "undefined" ? null : window.sessionStorage, options.tradingE2EMode);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pauseAfterClose, pauseBeforeOpen]);
}
