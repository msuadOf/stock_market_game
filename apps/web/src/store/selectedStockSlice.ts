/**
 * selectedStockSlice：当前选中的股票代码。
 * 行情表点击行 / 移动端切换详情时写入，图表与委托面板据此联动。
 */
import { createSlice, type PayloadAction } from "@reduxjs/toolkit";
import type { StockCode } from "../types/engine";

interface SelectedStockState {
  code: StockCode | null;
}

const initialState: SelectedStockState = {
  code: null,
};

const selectedStockSlice = createSlice({
  name: "selectedStock",
  initialState,
  reducers: {
    selectStock(state, action: PayloadAction<StockCode>) {
      state.code = action.payload;
    },
    clearSelection(state) {
      state.code = null;
    },
  },
});

export const { selectStock, clearSelection } = selectedStockSlice.actions;
export const selectedStockReducer = selectedStockSlice.reducer;
export type { SelectedStockState };
