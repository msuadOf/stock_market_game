import { createContext, useContext, useSyncExternalStore } from "react";
import type { QuickTrading, TradeSide, BookField } from "./quick-trading.ts";

export const QuickTradingContext = createContext<{ trading: QuickTrading; open(code: string, side: TradeSide): void; notice(message: string): void } | null>(null);
export function useBookTrading() { return useContext(QuickTradingContext); }
export function useTradeRevision(trading: QuickTrading) { return useSyncExternalStore(trading.subscribe, trading.version, trading.version); }
export function handleBookPick(context: NonNullable<ReturnType<typeof useBookTrading>>, code: string, side: TradeSide, index: number, field: BookField) { try { context.trading.pick(code, side, index, field); context.open(code, side); } catch (error) { context.notice(error instanceof Error ? error.message : String(error)); } }
