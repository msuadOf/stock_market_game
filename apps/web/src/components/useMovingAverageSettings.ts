import { useEffect, useMemo, useState } from "react";
import { useDispatch, useSelector } from "react-redux";
import { MOVING_AVERAGE_STORAGE_KEY, loadMovingAverageSettings, saveMovingAverageSettings, type MovingAverageSetting } from "../config/moving-average-settings.ts";
import { setChartAverageSettings } from "../store/chart-settings-slice.ts";
import type { AppDispatch, RootState } from "../store/store.ts";

const SETTINGS_EVENT = "stock-game-moving-averages-changed";
export function useMovingAverageSettings() {
  const dispatch = useDispatch<AppDispatch>();
  const periods = useSelector((state: RootState) => state.chartSettings.averagePeriods);
  const selected = useSelector((state: RootState) => state.chartSettings.selectedAverages);
  const settings = useMemo(() => periods.map((period) => ({ period, visible: selected.includes(period) })), [periods, selected]);
  const [error, setError] = useState<string | null>(null);
  const [available, setAvailable] = useState(() => typeof window === "undefined");
  useEffect(() => {
    const refresh = (event?: Event) => {
      if (event instanceof StorageEvent && event.key !== MOVING_AVERAGE_STORAGE_KEY && event.key !== null) return;
      try {
        const next = loadMovingAverageSettings(window.localStorage);
        dispatch(setChartAverageSettings(next)); setAvailable(true); setError(null);
      } catch (failure) {
        setAvailable(false);
        setError(`读取MA显示设置失败：${failure instanceof Error ? failure.message : String(failure)}；未安装默认值或损坏内容，请反馈错误详情或明确保存一组有效设置。`);
      }
    };
    refresh();
    window.addEventListener(SETTINGS_EVENT, refresh);
    window.addEventListener("storage", refresh);
    return () => { window.removeEventListener(SETTINGS_EVENT, refresh); window.removeEventListener("storage", refresh); };
  }, [dispatch]);
  const save = (next: readonly MovingAverageSetting[]) => {
    try {
      const parsed = saveMovingAverageSettings(window.localStorage, next);
      dispatch(setChartAverageSettings(parsed)); setAvailable(true); setError(null);
      window.dispatchEvent(new Event(SETTINGS_EVENT));
      return true;
    } catch (failure) {
      setError(`保存MA显示设置失败：${failure instanceof Error ? failure.message : String(failure)}；原设置保持不变，请反馈错误详情。`);
      return false;
    }
  };
  return { settings, save, error, available };
}
