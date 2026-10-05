/* oxlint-disable react/only-export-components -- 两端读取同一低频 TradingTimeline Context */
import { createContext, useContext } from "react";
import { DEFAULT_TRADING_TIMELINE } from "./trading-timeline.ts";

/** 独立图表使用标准交易日；应用 Provider 在建立会话时显式安装实际配置。 */
export const TradingTimelineContext = createContext(DEFAULT_TRADING_TIMELINE);
export function useTradingTimeline() { return useContext(TradingTimelineContext); }
