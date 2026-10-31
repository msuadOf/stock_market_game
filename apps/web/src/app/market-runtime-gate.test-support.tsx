import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import type { EngineHost } from "../host/engine-host.ts";
import { MarketRuntimeProvider, useMarketRuntimeActions } from "./MarketRuntimeProvider.tsx";

export function actionsFor(host: EngineHost): ReturnType<typeof useMarketRuntimeActions> {
  let actions: ReturnType<typeof useMarketRuntimeActions> | undefined;
  function Probe() { actions = useMarketRuntimeActions(); return null; }
  renderToStaticMarkup(createElement(MarketRuntimeProvider, { autoOrderManagerRef: { current: null }, setNotice: () => {}, hostRef: { current: host }, children: createElement(Probe) }));
  if (actions === undefined) throw new Error("应用查询入口未安装");
  return actions;
}
