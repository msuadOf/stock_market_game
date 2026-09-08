export type MobilePrimaryTab = "market" | "watchlist" | "positions" | "trades" | "user";
export type MobileChartPeriod = "分时" | "日K" | "周K" | "月K" | "五日";
export type MobileInfoTab = "看点" | "资讯" | "盘口" | "资金" | "社区" | "简况";

export const MOBILE_SPEED_OPTIONS = [1, 1.5, 2, 3, 6, 30, 60, 180, 360, 720, Infinity] as const;

export function mobileSpeedLabel(speed: number): string {
  return speed === Infinity ? "最快" : `${speed}x`;
}

export function formatMeasuredSpeed(actualMultiplier: number | null): string {
  if (actualMultiplier === null) return "实测 —";
  if (!Number.isFinite(actualMultiplier) || actualMultiplier < 0) {
    throw new RangeError(`实际倍速必须是非负有限数或 null，收到 ${String(actualMultiplier)}`);
  }
  if (actualMultiplier >= 1_000) {
    return `实测 ${(actualMultiplier / 1_000).toFixed(1).replace(/\.0$/, "")}kx`;
  }
  if (actualMultiplier >= 100) return `实测 ${actualMultiplier.toFixed(0)}x`;
  if (actualMultiplier >= 10) return `实测 ${actualMultiplier.toFixed(1)}x`;
  return `实测 ${actualMultiplier.toFixed(2)}x`;
}

export const MOBILE_PRIMARY_NAV = [
  ["market", "行情"],
  ["watchlist", "自选"],
  ["trades", "交易"],
  ["positions", "持仓"],
  ["user", "我的"],
] as const satisfies readonly (readonly [MobilePrimaryTab, string])[];

export function mobilePrimaryTitle(tab: MobilePrimaryTab): string {
  switch (tab) {
    case "market": return "模拟自选";
    case "watchlist": return "自选";
    case "trades": return "交易";
    case "positions": return "持仓";
    case "user": return "我的";
  }
}

export interface MobileUiState {
  primaryTab: MobilePrimaryTab;
  detailCode: string | null;
  chartPeriod: MobileChartPeriod;
  infoTab: MobileInfoTab;
  tradeSheetOpen: boolean;
}

export const initialMobileUiState: MobileUiState = {
  primaryTab: "market",
  detailCode: null,
  chartPeriod: "分时",
  infoTab: "资金",
  tradeSheetOpen: false,
};

export type MobileUiAction =
  | { type: "switch-primary"; tab: MobilePrimaryTab }
  | { type: "open-detail"; code: string }
  | { type: "select-period"; period: MobileChartPeriod }
  | { type: "select-info"; tab: MobileInfoTab }
  | { type: "open-trade" }
  | { type: "close-top-layer" }
  | { type: "back" };

export function reduceMobileUi(state: MobileUiState, action: MobileUiAction): MobileUiState {
  switch (action.type) {
    case "switch-primary":
      return { ...initialMobileUiState, primaryTab: action.tab };
    case "open-detail":
      return { ...state, primaryTab: "market", detailCode: action.code, tradeSheetOpen: false };
    case "select-period":
      return { ...state, chartPeriod: action.period };
    case "select-info":
      return { ...state, infoTab: action.tab };
    case "open-trade":
      return { ...state, tradeSheetOpen: true };
    case "close-top-layer":
      return state.tradeSheetOpen ? { ...state, tradeSheetOpen: false } : state;
    case "back":
      if (state.tradeSheetOpen) return { ...state, tradeSheetOpen: false };
      if (state.detailCode !== null) return { ...state, detailCode: null };
      return state;
  }
}
