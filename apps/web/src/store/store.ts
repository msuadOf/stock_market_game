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
import { syncSnapshotTick } from "./snapshot-clock";
import { applyPriceTickMarket } from "./market-depth-sync";

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
      syncSnapshotTick(snap, events);
      for (const ev of events) {
        if ("PriceTick" in ev) {
          const p = ev.PriceTick;
          if (snap) {
            const m = snap.markets[p.code];
            if (m) applyPriceTickMarket(m, p);
          }
          if (p.seq > state.lastSeq) state.lastSeq = p.seq;
        } else if ("AuctionTick" in ev) {
          const auction = ev.AuctionTick;
          if (snap && auction.indicative_price !== null) {
            const market = snap.markets[auction.code];
            if (market) market.last_price = auction.indicative_price;
          }
          if (snap) snap.phase = "CallAuction";
          if (auction.seq > state.lastSeq) state.lastSeq = auction.seq;
        } else if ("AuctionCompleted" in ev) {
          const auction = ev.AuctionCompleted;
          if (snap && auction.opening_price !== null) {
            const market = snap.markets[auction.code];
            if (market) market.last_price = auction.opening_price;
          }
          if (snap) snap.phase = "Continuous";
          if (auction.seq > state.lastSeq) state.lastSeq = auction.seq;
        } else if ("Trade" in ev) {
          const t = ev.Trade;
          if (snap) {
            const m = snap.markets[t.code];
            if (m) m.last_price = t.price;
          }
          if (t.seq > state.lastSeq) state.lastSeq = t.seq;
        } else if ("DayBoundary" in ev) {
          const d = ev.DayBoundary;
          if (snap) {
            snap.day = d.day;
            snap.phase = "CallAuction";
          }
          if (d.seq > state.lastSeq) state.lastSeq = d.seq;
        } else {
          // IntentRejected / SettlementError / VError：取 seq，具体内容交给调用方决定如何展示。
          const seq = (ev as { seq?: number }).seq ?? 0;
          if (seq > state.lastSeq) state.lastSeq = seq;
        }
      }
      // tick 已在批次入口按最新 PriceTick 同步；高倍率压缩仍会保留当前分钟的最后事件。
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
  },
});

export const { addAutoOrder, removeAutoOrder, toggleAutoOrder, markTriggered, clearTriggeredOrders } = autoOrdersSlice.actions;

export const store = configureStore({
  reducer: {
    snapshot: snapshotSlice.reducer,
    trades: tradesSlice.reducer,
    settings: settingsSlice.reducer,
    priceHistory: priceHistoryReducer,
    selectedStock: selectedStockReducer,
    autoOrders: autoOrdersSlice.reducer,
  },
});

export type RootState = ReturnType<typeof store.getState>;
export type AppDispatch = typeof store.dispatch;
