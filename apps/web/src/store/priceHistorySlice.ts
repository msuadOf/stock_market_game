/**
 * priceHistorySlice：每只股票的分时价格序列。
 *
 * 数据来源：PriceTick 事件 + Trade 事件里的成交价（成交同样刷新最新价）。
 * 每只股票维护一个最多 MAX_POINTS 个点的数组，超出则丢弃最旧的。
 */
import { createSlice, type PayloadAction } from "@reduxjs/toolkit";
import type { Cents, StockCode } from "../types/engine";

/** 单个价格采样点。time 为序号（seq），便于排序与去重。 */
export interface PricePoint {
  /** 事件 seq，用作横轴时间键。 */
  time: number;
  /** 价格（分）。 */
  price: Cents;
  /** 该点是否为主动买入（用于成交量柱染色）：true=买红，false=卖绿。 */
  buy?: boolean;
  /** 该点对应的成交量（仅在 Trade 事件时有值），用于量柱。 */
  volume?: number;
}

/** 每只股票一个采样数组。 */
type PriceHistoryState = Record<StockCode, PricePoint[]>;

/** 每只股票最多保留的点数。 */
const MAX_POINTS = 300;

const initialState: PriceHistoryState = {};

interface AppendPayload {
  code: StockCode;
  time: number;
  price: Cents;
  buy?: boolean;
  volume?: number;
}

/** 把一个采样点压入指定股票的序列，并截断到 MAX_POINTS。重复 time（同一事件）会被跳过。 */
function pushPoint(state: PriceHistoryState, p: AppendPayload): void {
  let arr = state[p.code];
  if (!arr) {
    arr = [];
    state[p.code] = arr;
  }
  // 去重：同一 time（同一事件 seq）不重复记录。
  if (arr.length > 0 && arr[arr.length - 1].time === p.time) {
    // 同一事件：保留并更新最后一点（可能补全成交量方向）。
    const last = arr[arr.length - 1];
    last.price = p.price;
    if (p.buy !== undefined) last.buy = p.buy;
    if (p.volume !== undefined) last.volume = (last.volume ?? 0) + p.volume;
    return;
  }
  arr.push({ time: p.time, price: p.price, buy: p.buy, volume: p.volume });
  if (arr.length > MAX_POINTS) {
    state[p.code] = arr.slice(arr.length - MAX_POINTS);
  }
}

const priceHistorySlice = createSlice({
  name: "priceHistory",
  initialState,
  reducers: {
    /** 追加一个采样点（来自 PriceTick 或 Trade）。 */
    appendPoint(state, action: PayloadAction<AppendPayload>) {
      pushPoint(state, action.payload);
    },
    /** 批量追加多个采样点。 */
    appendPoints(state, action: PayloadAction<AppendPayload[]>) {
      for (const p of action.payload) pushPoint(state, p);
    },
    /** 清空全部历史（换会话时用）。 */
    clearHistory(state) {
      for (const key of Object.keys(state)) delete state[key];
    },
  },
});

export const { appendPoint, appendPoints, clearHistory } = priceHistorySlice.actions;
export const priceHistoryReducer = priceHistorySlice.reducer;
export type { PriceHistoryState };
