import { useEffect, useState, type ReactElement, type ReactNode } from "react";
import { SECURITY_LIST_VIEW_LABELS, type SecurityListView } from "./security-browser-model.ts";

export type DesktopView = "quotes" | "stock" | "company" | "trading" | "settings";
interface Props {
  view: DesktopView;
  securityView: SecurityListView;
  onViewChange: (view: DesktopView) => void;
  stockList: ReactNode;
  panels: ReactElement<{ id: string }>[];
  onTradeCurrent?: () => void;
  tradingOpen?: boolean;
  onTradingOpenChange?: (open: boolean) => void;
}
const NAVIGATION = [
  ["quotes", "行情", "▤"], ["stock", "个股", "⌁"], ["trading", "交易", "⇄"],
  ["settings", "游戏", "▣"],
] as const;

/** 桌面入口只负责内容层次；委托、公司查询与行情仍由原面板持有。 */
export function DesktopTerminal({ view, securityView, onViewChange, stockList, panels, onTradeCurrent, tradingOpen, onTradingOpenChange }: Props) {
  const [dock, setDock] = useState<"section-positions" | "section-trades">("section-positions");
  const [localTradingOpen, setLocalTradingOpen] = useState(false);
  const dockOpen = tradingOpen === undefined ? localTradingOpen : tradingOpen;
  const setDockOpen = onTradingOpenChange ?? setLocalTradingOpen;
  const fullTrading = view === "trading";
  const dockVisible = fullTrading || (dockOpen && view !== "settings");
  const tradingPanels = panels.filter(panel => ["section-order", "section-positions", "section-trades"].includes(panel.props.id));
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const target = event.target;
      if (event.key === "Escape" && !event.defaultPrevented && dockVisible && !fullTrading) { event.preventDefault(); setDockOpen(false); return; }
      if (event.key !== "F10" || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey || (target instanceof HTMLElement && target.closest("input, textarea, select, [contenteditable=true]"))) return;
      event.preventDefault();
      onViewChange("company");
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onViewChange, dockVisible, fullTrading, setDockOpen]);
  const openTradeCurrent = () => { onTradeCurrent?.(); setDockOpen(true); };
  const detail = view === "stock" || view === "company";
  return <div className="desktop-terminal" data-view={view} data-trading-open={dockVisible ? "true" : "false"}>
    <nav className="terminal-rail" aria-label="桌面主导航">
      {NAVIGATION.map(([id, label, icon]) => <button key={id} type="button" aria-current={view === id || (id === "stock" && view === "company") ? "page" : undefined} onClick={() => onViewChange(id)}><span aria-hidden="true">{icon}</span>{label}</button>)}
    </nav>
    <div className="terminal-workspace">
      <div className="terminal-viewbar">
        {detail ? <><button onClick={() => onViewChange("quotes")}>返回行情</button><span className="terminal-separator" /><button aria-pressed={view === "stock"} onClick={() => onViewChange("stock")}>走势图</button><button aria-keyshortcuts="F10" aria-pressed={view === "company"} onClick={() => onViewChange("company")}>公司资料 F10</button><button onClick={openTradeCurrent}>交易此股票</button></>
          : <><strong>{{ quotes: "沪深行情", trading: "模拟交易", settings: "游戏管理", stock: "个股", company: "公司资料" }[view]}</strong><span className="terminal-view-description">{{ quotes: `${SECURITY_LIST_VIEW_LABELS[securityView]}模拟股票`, trading: "委托 · 持仓 · 市场成交", settings: "进度与运行设置", stock: "", company: "" }[view]}</span></>}
      </div>
      <div className="terminal-body" hidden={fullTrading}>
        <aside className="terminal-stocklist" hidden={!detail}>{stockList}</aside>
        {panels.filter(panel => !tradingPanels.includes(panel)).map(panel => {
          const id = panel.props.id;
          const visible = id === "section-market" ? view === "quotes"
            : id === "section-trade" ? view === "quotes" || view === "stock"
            : id === "section-company" ? view === "company"
            : id === "section-user" && view === "settings";
          return <section key={id} className={`terminal-slot terminal-slot-${id}`} hidden={!visible}>
            {id === "section-market" && <div className="terminal-sectionbar"><strong>{SECURITY_LIST_VIEW_LABELS[securityView]}股票</strong><span>单击预览 · 双击 / Enter 进入个股</span></div>}
            {id === "section-trade" && view === "quotes" && <div className="terminal-sectionbar"><strong>个股预览</strong><button onClick={() => onViewChange("stock")}>进入个股 →</button></div>}
            {panel}
          </section>;
        })}

        {view === "stock" && <div className="terminal-contextbar"><button onClick={() => onViewChange("company")}>公司资料与公告</button><button onClick={openTradeCurrent}>委托下单 / 查看持仓</button><span>行情数量：手 · 委托数量：股</span></div>}
      </div>
      <section className="terminal-trading-region" aria-label="看盘交易栏" hidden={!dockVisible} data-expanded={fullTrading ? "full" : "dock"}>
        <div className="terminal-dockbar"><strong>{fullTrading ? "模拟交易" : "看盘交易"}</strong><div className="terminal-ledger-tabs" role="group" aria-label="交易查询"><button aria-pressed={dock === "section-positions"} onClick={() => setDock("section-positions")}>资金持仓</button><button aria-pressed={dock === "section-trades"} onClick={() => setDock("section-trades")}>市场逐笔成交</button></div>
          {!fullTrading && <button className="terminal-dock-close" type="button" onClick={() => setDockOpen(false)}>收起交易栏 <span aria-hidden="true">⌄</span></button>}
        </div>
        <div className="terminal-dock-content">
          {tradingPanels.map(panel => <section key={panel.props.id} className={`terminal-slot terminal-slot-${panel.props.id}`} hidden={panel.props.id !== "section-order" && panel.props.id !== dock}>{panel}</section>)}
        </div>
      </section>
    </div>
  </div>;
}
