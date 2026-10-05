import { createSlice, type PayloadAction } from "@reduxjs/toolkit";
import { CHART_INDICATORS, type ChartIndicator } from "../components/chart-display-options.ts";
import { KLINE_MOVING_AVERAGES } from "../components/kline-moving-averages.ts";
import { klineWindow, MOBILE_KLINE_DEFAULT_CAPACITY, reduceKlineViewport, type KlineViewport, type KlineViewportAction } from "../mobile/market-model.ts";

interface ChartSettingsState {
  selectedAverages: number[];
  indicator: ChartIndicator;
  viewports: Record<string, KlineViewport>;
}

const defaultViewport: Readonly<KlineViewport> = Object.freeze({ capacity: MOBILE_KLINE_DEFAULT_CAPACITY, offsetFromEnd: 0 });
const initialState: ChartSettingsState = { selectedAverages: KLINE_MOVING_AVERAGES.map(item => item.days), indicator: "kdj", viewports: {} };

/** 没有操作过的证券使用初始窗口；其他证券窗口不随当前选择改变。 */
export function selectChartViewport(state: ChartSettingsState, code: string): Readonly<KlineViewport> {
  return Object.hasOwn(state.viewports, code) ? state.viewports[code] : defaultViewport;
}

const chartSettingsSlice = createSlice({
  name: "chartSettings",
  initialState,
  reducers: {
    toggleChartAverage(state, action: PayloadAction<number>) {
      const days = action.payload;
      if (!KLINE_MOVING_AVERAGES.some(item => item.days === days)) throw new RangeError(`不支持的均线周期：${days}`);
      state.selectedAverages = state.selectedAverages.includes(days) ? state.selectedAverages.filter(day => day !== days) : [...state.selectedAverages, days];
    },
    setChartIndicator(state, action: PayloadAction<ChartIndicator>) {
      if (!CHART_INDICATORS.some(([value]) => value === action.payload)) throw new RangeError(`不支持的副图指标：${action.payload}`);
      state.indicator = action.payload;
    },
    changeChartViewport(state, action: PayloadAction<{ code: string; total: number; action: KlineViewportAction }>) {
      const { code, total, action: viewportAction } = action.payload;
      const stored = selectChartViewport(state, code);
      const visible = klineWindow(total, stored.capacity, stored.offsetFromEnd);
      state.viewports[code] = reduceKlineViewport(visible, total, viewportAction);
    },
  },
});

export const { toggleChartAverage, setChartIndicator, changeChartViewport } = chartSettingsSlice.actions;
export const chartSettingsReducer = chartSettingsSlice.reducer;
