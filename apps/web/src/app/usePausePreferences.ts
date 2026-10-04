import { useEffect, useRef, useState, type MutableRefObject } from "react";
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
  let pending = Promise.resolve();
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
    synchronize(host: Pick<EngineHost, "setPausePreferences"> | null, preferences: PausePreferences, source: StorageSource, skip: boolean, isCurrent: () => boolean = () => true): Promise<void> {
      if (skip || host === null) return Promise.resolve();
      const synchronization = pending.then(async () => {
        if (!isCurrent()) return;
        await host.setPausePreferences(preferences);
        if (!isCurrent()) return;
        ports.apply(preferences);
        if (loaded) {
          const storage = readStorage(source);
          if (storage !== null) savePausePreferences(storage, preferences);
        }
      }).catch((failure) => { if (isCurrent()) report(failure); });
      pending = synchronization;
      return synchronization;
    },
  };
}

interface Options extends Ports {
  hostRef: MutableRefObject<EngineHost | null>;
  pauseAfterClose: boolean;
  pauseBeforeOpen: boolean;
  tradingE2EMode: boolean;
}

export function usePausePreferences(options: Options) {
  const [pendingCount, setPendingCount] = useState(0);
  const lifecycleRef = useRef<ReturnType<typeof createPausePreferencesLifecycle> | null>(null);
  if (lifecycleRef.current === null) lifecycleRef.current = createPausePreferencesLifecycle(options);
  useEffect(() => {
    lifecycleRef.current!.loadOnce(() => typeof window === "undefined" ? null : window.sessionStorage);
  }, []);
  async function changePreferences(preferences: PausePreferences) {
    const host = options.hostRef.current;
    if (host === null) { options.onError("宿主尚未就绪，无法修改暂停偏好"); return; }
    setPendingCount((count) => count + 1);
    try {
      await lifecycleRef.current!.synchronize(host, preferences,
        () => typeof window === "undefined" ? null : window.sessionStorage, options.tradingE2EMode, () => host === options.hostRef.current);
    } finally { setPendingCount((count) => count - 1); }
  }
  return { pending: pendingCount > 0, changePreferences };
}
