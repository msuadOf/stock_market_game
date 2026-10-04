import { useEffect, useState, type ReactElement, type ReactNode } from "react";

export type DesktopView = "quotes" | "stock" | "company" | "trading" | "settings";
interface Props {
  view: DesktopView;
  onViewChange: (view: DesktopView) => void;
  stockList: ReactNode;
  panels: ReactElement<{ id: string }>[];
  onTradeCurrent?: () => void;
}
const NAVIGATION = [
  ["quotes", "行情", "▤"], ["stock", "个股", "⌁"], ["trading", "交易", "⇄"],
  ["settings", "游戏", "▣"],
] as const;

/** 桌面入口只负责内容层次；委托、公司查询与行情仍由原面板持有。 */
export function DesktopTerminal({ view, onViewChange, stockList, panels, onTradeCurrent }: Props) {
  const [dock, setDock] = useState<"section-positions" | "section-trades">("section-positions");
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const target = event.target;
      if (event.key !== "F10" || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey || (target instanceof HTMLElement && target.closest("input, textarea, select, [contenteditable=true]"))) return;
      event.preventDefault();
      onViewChange("company");
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onViewChange]);
  const openTradeCurrent = () => { onTradeCurrent?.(); onViewChange("trading"); };
  const detail = view === "stock" || view === "company";
  return <div className="desktop-terminal" data-view={view}>
    <nav className="terminal-rail" aria-label="桌面主导航">
      {NAVIGATION.map(([id, label, icon]) => <button key={id} type="button" aria-current={view === id || (id === "stock" && view === "company") ? "page" : undefined} onClick={() => onViewChange(id)}><span aria-hidden="true">{icon}</span>{label}</button>)}
    </nav>
    <div className="terminal-workspace">
      <div className="terminal-viewbar">
        {detail ? <><button onClick={() => onViewChange("quotes")}>返回行情</button><span className="terminal-separator" /><button aria-pressed={view === "stock"} onClick={() => onViewChange("stock")}>走势图</button><button aria-keyshortcuts="F10" aria-pressed={view === "company"} onClick={() => onViewChange("company")}>公司资料 F10</button><button onClick={openTradeCurrent}>交易此股票</button></>
          : <><strong>{{ quotes: "沪深行情", trading: "模拟交易", settings: "游戏管理", stock: "个股", company: "公司资料" }[view]}</strong><span className="terminal-view-description">{{ quotes: "全部模拟股票", trading: "委托 · 持仓 · 市场成交", settings: "进度与运行设置", stock: "", company: "" }[view]}</span></>}
      </div>
      <div className="terminal-body">
        <aside className="terminal-stocklist" hidden={!detail}>{stockList}</aside>
        {panels.map(panel => {
          const id = panel.props.id;
          const visible = id === "section-market" ? view === "quotes"
            : id === "section-trade" ? view === "quotes" || view === "stock"
            : id === "section-company" ? view === "company"
            : id === "section-order" ? view === "trading"
            : id === "section-user" ? view === "settings"
            : view === "trading" && id === dock;
          return <section key={id} className={`terminal-slot terminal-slot-${id}`} hidden={!visible}>
            {id === "section-market" && <div className="terminal-sectionbar"><strong>全部股票</strong><span>单击预览 · 双击 / Enter 进入个股</span></div>}
            {id === "section-trade" && view === "quotes" && <div className="terminal-sectionbar"><strong>个股预览</strong><button onClick={() => onViewChange("stock")}>进入个股 →</button></div>}
            {panel}
          </section>;
        })}
        {view === "trading" && <div className="terminal-ledger-tabs" role="group" aria-label="交易查询"><button aria-pressed={dock === "section-positions"} onClick={() => setDock("section-positions")}>资金持仓</button><button aria-pressed={dock === "section-trades"} onClick={() => setDock("section-trades")}>市场逐笔成交</button></div>}
        {view === "stock" && <div className="terminal-contextbar"><button onClick={() => onViewChange("company")}>公司资料与公告</button><button onClick={openTradeCurrent}>委托下单 / 查看持仓</button><span>行情数量：手 · 委托数量：股</span></div>}
      </div>
    </div>
  </div>;
}
