export const CHART_INDICATORS = [["volume", "量能"], ["macd", "MACD"], ["kdj", "KDJ"], ["none", "无"]] as const;
export type ChartIndicator = typeof CHART_INDICATORS[number][0];
