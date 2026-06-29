/**
 * RTK store + slice 装配：snapshot / settings / trades / priceHistory / selectedStock。
 *
 * 事件流：host 产出 EngineEvent[] → dispatch(applyEvents)。
 * applyEvents 把 PriceTick 写回 markets[].last_price，把 Trade 追加进交易日志（上限 100 条），
 * 并刷新 snapshot 的 seq/tick/day。
 *
 * 各 slice 拆到独立文件，这里只做装配与统一导出。
 */
import { configureStore, createSlice, type PayloadAction } from "@reduxjs/toolkit";
import type {
  EngineEvent,
  Snapshot,
  TradeEvent,
} from "../types/engine";
import { priceHistoryReducer } from "./priceHistorySlice";
import { selectedStockReducer } from "./selectedStockSlice";

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
    /** 处理一批事件：更新 last_price / seq / tick / day，并把成交事件转发给 trades。 */
    applyEvents(state, action: PayloadAction<EngineEvent[]>) {
      const events = action.payload;
      const snap = state.snapshot;
      for (const ev of events) {
        if ("PriceTick" in ev) {
          const p = ev.PriceTick;
          if (snap) {
            const m = snap.markets[p.code];
            if (m) m.last_price = p.last_price;
          }
          if (p.seq > state.lastSeq) state.lastSeq = p.seq;
        } else if ("Trade" in ev) {
          const t = ev.Trade;
          if (snap) {
            const m = snap.markets[t.code];
            if (m) m.last_price = t.price;
          }
          if (t.seq > state.lastSeq) state.lastSeq = t.seq;
        } else if ("DayBoundary" in ev) {
          const d = ev.DayBoundary;
          if (snap) snap.day = d.day;
          if (d.seq > state.lastSeq) state.lastSeq = d.seq;
        } else {
          // IntentRejected / SettlementError / VError：取 seq，具体内容交给调用方决定如何展示。
          const seq = (ev as { seq?: number }).seq ?? 0;
          if (seq > state.lastSeq) state.lastSeq = seq;
        }
      }
      // 同步 tick：以最新事件序列里能反映出的进度为准；snapshot.tick 由 host 直接读 wasm 时刷新。
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
      const merged = [...incoming.reverse(), ...state.items];
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
}

const initialSettingsState: SettingsState = {
  speed: 1,
  running: false,
  theme: "light",
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
  },
});

export const { setSnapshot, applyEvents } = snapshotSlice.actions;
export const { appendTrades, clearTrades } = tradesSlice.actions;
export const { setSpeed, setRunning, setTheme } = settingsSlice.actions;

export const store = configureStore({
  reducer: {
    snapshot: snapshotSlice.reducer,
    trades: tradesSlice.reducer,
    settings: settingsSlice.reducer,
    priceHistory: priceHistoryReducer,
    selectedStock: selectedStockReducer,
  },
});

export type RootState = ReturnType<typeof store.getState>;
export type AppDispatch = typeof store.dispatch;
