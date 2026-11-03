/** 受控浏览器验收查询参数；仅在 e2e 构建中读取 window，保持普通 SSR 测试无 DOM 依赖。 */
export const TRADING_E2E_MODE = import.meta.env.MODE === "e2e"
  && new URLSearchParams(window.location.search).get("tradingE2E") === "1";
