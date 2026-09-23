/**
 * RTK store + slice 装配：snapshot / settings / trades / priceHistory / selectedStock。
 *
 * 协议流：host 交付 generation-tagged baseline 或 EngineUpdate；协调器验证后以完整
 * timeseries frame 刷新行情/时钟，效果流独立追加有限成交带。
 *
 * 各 slice 拆到独立文件，这里只做装配与统一导出。
 */
import { configureStore, createSlice, type PayloadAction } from "@reduxjs/toolkit";
import type { Snapshot, TradeEvent } from "../types/engine";
import type { DailyCandle } from "../types/generated/DailyCandle.ts";
import type { MarketSnap } from "../types/generated/MarketSnap.ts";
import { priceHistoryReducer } from "./priceHistorySlice.ts";
import { selectedStockReducer } from "./selectedStockSlice.ts";
import { companyReducer } from "./company-slice.ts";

// ── snapshotSlice ──

interface SnapshotState {
  snapshot: Snapshot | null;
  lastSeq: number;
}

const initialSnapshotState: SnapshotState = {
  snapshot: null,
  lastSeq: 0,
};

const snapshotSlice = createSlice({
  name: "snapshot",
  initialState: initialSnapshotState,
  reducers: {
    setSnapshot(state, action: PayloadAction<Snapshot>) {
      state.snapshot = action.payload;
      state.lastSeq = action.payload.seq;
    },
    applyProtocolFrame(state, action: PayloadAction<{
      readonly tick: number;
      readonly seq: number;
      readonly markets: Record<string, MarketSnap>;
      readonly activeDailyCandles: Record<string, DailyCandle>;
    }>) {
      const snap = state.snapshot;
      if (snap === null || action.payload.seq < state.lastSeq) return;
      snap.tick = action.payload.tick;
      snap.seq = action.payload.seq;
      snap.markets = action.payload.markets;
      snap.active_daily_candles = action.payload.activeDailyCandles;
      state.lastSeq = action.payload.seq;
    },
  },
});

// ── tradesSlice ──

interface TradesState {
  items: TradeEvent[];
}

const MAX_TRADES = 100;

const initialTradesState: TradesState = {
  items: [],
};

const tradesSlice = createSlice({
  name: "trades",
  initialState: initialTradesState,
  reducers: {
    appendTrades(state, action: PayloadAction<TradeEvent[]>) {
      const incoming = action.payload;
      if (incoming.length === 0) return;
      const merged = [...[...incoming].reverse(), ...state.items];
      state.items = merged.length > MAX_TRADES ? merged.slice(0, MAX_TRADES) : merged;
    },
    clearTrades(state) {
      state.items = [];
    },
  },
});

// ── settingsSlice ──

type Theme = "light" | "dark";

interface SettingsState {
  speed: number;
  running: boolean;
  theme: Theme;
  pauseAfterClose: boolean;
  pauseBeforeOpen: boolean;
}

const initialSettingsState: SettingsState = {
  speed: 1,
  running: false,
  theme: "light",
  pauseAfterClose: false,
  pauseBeforeOpen: false,
};

const settingsSlice = createSlice({
  name: "settings",
  initialState: initialSettingsState,
  reducers: {
    setSpeed(state, action: PayloadAction<number>) {
      state.speed = action.payload;
    },
    setRunning(state, action: PayloadAction<boolean>) {
      state.running = action.payload;
    },
    setTheme(state, action: PayloadAction<Theme>) {
      state.theme = action.payload;
    },
    setPauseAfterClose(state, action: PayloadAction<boolean>) {
      state.pauseAfterClose = action.payload;
    },
    setPauseBeforeOpen(state, action: PayloadAction<boolean>) {
      state.pauseBeforeOpen = action.payload;
    },
  },
});

export const { setSnapshot, applyProtocolFrame } = snapshotSlice.actions;
export const snapshotReducer = snapshotSlice.reducer;
export const { appendTrades, clearTrades } = tradesSlice.actions;
export const { setSpeed, setRunning, setTheme, setPauseAfterClose, setPauseBeforeOpen } = settingsSlice.actions;

// ── autoOrdersSlice ──

export interface AutoOrderUI {
  id: string;
  code: string;
  type: "stopProfit" | "stopLoss" | "buyTrigger" | "sellTrigger";
  triggerPrice: number;
  qty: number;
  side: "Buy" | "Sell";
  enabled: boolean;
  triggered: boolean;
}

interface AutoOrdersState {
  items: AutoOrderUI[];
}

const initialAutoOrdersState: AutoOrdersState = {
  items: [],
};

const autoOrdersSlice = createSlice({
  name: "autoOrders",
  initialState: initialAutoOrdersState,
  reducers: {
    addAutoOrder(state, action: PayloadAction<AutoOrderUI>) {
      state.items.push(action.payload);
    },
    removeAutoOrder(state, action: PayloadAction<string>) {
      state.items = state.items.filter((o) => o.id !== action.payload);
    },
    toggleAutoOrder(state, action: PayloadAction<string>) {
      const o = state.items.find((x) => x.id === action.payload);
      if (o) o.enabled = !o.enabled;
    },
    markTriggered(state, action: PayloadAction<string>) {
      const o = state.items.find((x) => x.id === action.payload);
      if (o) o.triggered = true;
    },
    clearTriggeredOrders(state) {
      state.items = state.items.filter((o) => !o.triggered);
    },
    clearAutoOrders(state) {
      state.items = [];
    },
  },
});

export const {
  addAutoOrder,
  removeAutoOrder,
  toggleAutoOrder,
  markTriggered,
  clearTriggeredOrders,
  clearAutoOrders,
} = autoOrdersSlice.actions;

export const store = configureStore({
  reducer: {
    snapshot: snapshotReducer,
    trades: tradesSlice.reducer,
    settings: settingsSlice.reducer,
    priceHistory: priceHistoryReducer,
    selectedStock: selectedStockReducer,
    autoOrders: autoOrdersSlice.reducer,
    company: companyReducer,
  },
});

export type RootState = ReturnType<typeof store.getState>;
export type AppDispatch = typeof store.dispatch;
