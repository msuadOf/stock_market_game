import type { PausePreferences } from "../types/generated/PausePreferences.ts";

export const PAUSE_PREFERENCES_STORAGE_KEY = "stock-game-pause-preferences";

interface KeyValueStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

export function defaultPausePreferences(): PausePreferences {
  return { pause_after_close: false, pause_before_open: false };
}

export function loadPausePreferences(storage: KeyValueStorage): PausePreferences {
  let raw: string | null;
  try {
    raw = storage.getItem(PAUSE_PREFERENCES_STORAGE_KEY);
  } catch (error) {
    throw new Error(`读取暂停偏好失败：${error instanceof Error ? error.message : String(error)}`);
  }
  if (raw === null) return defaultPausePreferences();
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch (error) {
    throw new Error(`暂停偏好不是合法 JSON：${error instanceof Error ? error.message : String(error)}`);
  }
  if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) throw new Error("暂停偏好必须是对象");
  const source = parsed as Readonly<Record<string, unknown>>;
  if (Object.keys(source).length !== 2 || typeof source.pause_after_close !== "boolean" || typeof source.pause_before_open !== "boolean") {
    throw new Error("暂停偏好必须包含两个布尔值");
  }
  return { pause_after_close: source.pause_after_close, pause_before_open: source.pause_before_open };
}

export function savePausePreferences(storage: KeyValueStorage, preferences: PausePreferences): void {
  try {
    storage.setItem(PAUSE_PREFERENCES_STORAGE_KEY, JSON.stringify(preferences));
  } catch (error) {
    throw new Error(`写入暂停偏好失败：${error instanceof Error ? error.message : String(error)}`);
  }
}
